//! Shell timeout-boundary regressions through the public `bash` tool.
//!
//! These lock the tool-visible lifecycle guarantees implemented in
//! `shell_run.rs`: the configured timeout bounds the whole command lifecycle,
//! and both deadline expiry and future cancellation tear the process group
//! down so nothing can keep mutating the workspace after the call returns.

#![cfg(unix)]

use std::time::Duration;

use common::{expect_execution_error, test_context};
use harness_core::config::ShellAllowlist;
use harness_core::tool::Tool;
use harness_tools::{coordinator_registry, UnwrapOrAbort};
use serde_json::json;

mod common;

#[tokio::test]
async fn shell_timeout_terminates_command_before_delayed_marker_write() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let bash = coordinator_registry(ShellAllowlist::default())
        .get("bash")
        .unwrap_or_abort();

    // act
    let started = tokio::time::Instant::now();
    let error = bash
        .call(
            test_context(
                temp.path(),
                "run-shell-timeout-boundary",
                "toolcall-shell-timeout",
            ),
            json!({
                "command": "sleep 1; touch late-marker.txt",
                "workdir": ".",
                "timeout": 200,
            }),
        )
        .await
        .expect_err("sleeping command must time out");
    let elapsed = started.elapsed();

    // assert
    expect_execution_error(error, "timed out");
    assert!(
        elapsed < Duration::from_millis(800),
        "tool call ran past the configured timeout: {elapsed:?}"
    );
    // Wait longer than the sleep so a survivor would have created its marker
    // by now.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert!(
        !temp.path().join("late-marker.txt").exists(),
        "timed-out command tree must not create a delayed marker"
    );
}

#[tokio::test]
async fn shell_cancellation_racing_timeout_terminates_command_tree() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let bash = coordinator_registry(ShellAllowlist::default())
        .get("bash")
        .unwrap_or_abort();
    let context = test_context(
        temp.path(),
        "run-shell-cancel-race",
        "toolcall-shell-cancel-race",
    );

    // act
    // The external cancellation deliberately races the internal 400ms deadline.
    let spawned = tokio::spawn(async move {
        bash.call(
            context,
            json!({
                "command": "sleep 2; touch cancel-marker.txt",
                "workdir": ".",
                "timeout": 400,
            }),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(380)).await;
    let cancelling = !spawned.is_finished();
    if cancelling {
        spawned.abort();
    }
    let join = spawned.await;

    // assert
    if cancelling {
        assert!(
            join.expect_err("aborted tool call must not finish normally")
                .is_cancelled(),
            "cancellation must drop the tool future, not panic"
        );
    } else {
        expect_execution_error(
            join.unwrap_or_abort()
                .expect_err("sleep must outlive the configured timeout"),
            "timed out",
        );
    }
    // Wait longer than the sleep so a survivor would have created its marker
    // by now.
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert!(
        !temp.path().join("cancel-marker.txt").exists(),
        "command tree must be terminated on both timeout and cancellation"
    );
}
