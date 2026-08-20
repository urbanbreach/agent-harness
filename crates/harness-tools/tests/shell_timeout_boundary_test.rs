//! Shell timeout-boundary regressions through the public `bash` tool.
//!
//! These lock the tool-visible lifecycle guarantees implemented in
//! `shell_run.rs`: the configured timeout bounds the whole command lifecycle,
//! and both deadline expiry and future cancellation tear the process group
//! down so nothing can keep mutating the workspace after the call returns.

#![cfg(unix)]

use std::path::Path;
use std::time::Duration;

use common::{expect_execution_error, test_context};
use harness_core::config::ShellAllowlist;
use harness_core::tool::Tool;
use harness_tools::{coordinator_registry, UnwrapOrAbort};
use rustix::io::Errno;
use rustix::process::{test_kill_process, Pid};
use serde_json::json;

mod common;

async fn started_process(pid_path: &Path) -> Pid {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(raw_pid) = std::fs::read_to_string(pid_path) {
                if let Ok(raw_pid) = raw_pid.trim().parse::<i32>() {
                    if let Some(pid) = Pid::from_raw(raw_pid) {
                        return pid;
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort()
}

async fn assert_process_exited(process: Pid) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match test_kill_process(process) {
                Err(Errno::SRCH) => return,
                Err(error) => {
                    assert_eq!(error, Errno::SRCH, "failed to inspect shell process");
                    return;
                }
                Ok(()) if process_is_zombie(process) => return,
                Ok(()) => tokio::task::yield_now().await,
            }
        }
    })
    .await
    .unwrap_or_abort();
}

#[cfg(target_os = "linux")]
fn process_is_zombie(process: Pid) -> bool {
    let stat_path = format!("/proc/{}/stat", process.as_raw_pid());
    std::fs::read_to_string(stat_path).ok().is_some_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, state)| state.trim_start().starts_with('Z'))
    })
}

#[cfg(not(target_os = "linux"))]
const fn process_is_zombie(_process: Pid) -> bool {
    false
}

#[tokio::test]
async fn shell_timeout_terminates_started_command() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let pid_path = temp.path().join("shell-process.pid");
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
                "command": "python3 -c 'import os,pathlib; pathlib.Path(\"shell-process.pid\").write_text(str(os.getppid()))'; sleep 60; touch late-marker.txt",
                "workdir": ".",
                "timeout": 200,
            }),
        )
        .await
        .expect_err("sleeping command must time out");
    let elapsed = started.elapsed();
    let process = started_process(&pid_path).await;

    // assert
    expect_execution_error(error, "timed out");
    assert!(
        elapsed < Duration::from_millis(800),
        "tool call ran past the configured timeout: {elapsed:?}"
    );
    assert_process_exited(process).await;
    assert!(
        !temp.path().join("late-marker.txt").exists(),
        "timed-out shell must not create its delayed marker"
    );
}

#[tokio::test]
async fn shell_cancellation_terminates_command_after_process_start() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let pid_path = temp.path().join("shell-process.pid");
    let bash = coordinator_registry(ShellAllowlist::default())
        .get("bash")
        .unwrap_or_abort();
    let context = test_context(
        temp.path(),
        "run-shell-cancel-race",
        "toolcall-shell-cancel-race",
    );

    // act
    let spawned = tokio::spawn(async move {
        bash.call(
            context,
            json!({
                "command": "python3 -c 'import os,pathlib; pathlib.Path(\"shell-process.pid\").write_text(str(os.getppid()))'; sleep 60; touch cancel-marker.txt",
                "workdir": ".",
                "timeout": 5_000,
            }),
        )
        .await
    });
    let process = started_process(&pid_path).await;
    spawned.abort();
    let join = spawned.await;

    // assert
    assert!(
        join.expect_err("aborted tool call must not finish normally")
            .is_cancelled(),
        "cancellation must drop the tool future, not panic"
    );
    assert_process_exited(process).await;
    assert!(
        !temp.path().join("cancel-marker.txt").exists(),
        "cancelled shell must not create its delayed marker"
    );
}
