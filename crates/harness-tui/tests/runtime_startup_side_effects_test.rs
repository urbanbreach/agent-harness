//! Regression test: production TUI startup must not write synthetic probe artifacts
//! to the workspace.
//!
//! `seed_operator_host_probes` writes `harness.json`, `.agent-harness/plans/`,
//! `.harness-cow-probe/`, `.harness-sessions-probe/`, `.harness-foreign-probe-root/`,
//! `.jj/`, cron journals, team mailboxes, plugins, code graph fixtures, and
//! edit-attribution data. These are test/diagnostic fixtures and must never be
//! created by the production startup path (`run_tui_with_options`). Tests that
//! need them must call `seed_operator_host_probes` explicitly.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration owner tests use fail-fast asserts"
)]

use std::path::PathBuf;
use std::sync::mpsc;

use harness_tui::{run_tui_with_options, TuiMode, TuiOptions};
use tempfile::TempDir;

/// Probe artifacts that `seed_operator_host_probes` writes to the workspace root.
/// Production TUI startup must NOT create any of these.
const PROBE_ARTIFACT_RELATIVE_PATHS: &[&str] = &[
    "harness.json",
    ".agent-harness/plans",
    ".harness-cow-probe",
    ".harness-sessions-probe",
    ".harness-foreign-probe-root",
    ".jj",
];

/// Given an empty temp workspace, when production TUI startup runs, then no
/// synthetic operator-host probe artifacts are written to the workspace.
#[test]
fn production_tui_startup_does_not_write_synthetic_probe_artifacts() {
    // arrange
    let workspace = TempDir::new().expect("create temp workspace");
    let workspace_root = workspace.path();

    // act
    let (_tx, rx) = mpsc::channel();
    let _ = run_tui_with_options(TuiOptions {
        mode: TuiMode::Startup {
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx: rx,
        },
        exit_on_finish: true,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        workspace_root: Some(workspace_root.to_path_buf()),
    });

    // assert
    let created_artifacts: Vec<&str> = PROBE_ARTIFACT_RELATIVE_PATHS
        .iter()
        .filter(|relative| workspace_root.join(relative).exists())
        .copied()
        .collect();

    assert!(
        created_artifacts.is_empty(),
        "production TUI startup wrote synthetic probe artifacts to the workspace: {created_artifacts:?}\n\
         These should only be created by explicit `seed_operator_host_probes` calls in tests, \
         not by the production startup path `run_tui_with_options`."
    );
}

/// Given an empty temp workspace, when production TUI live initialization runs,
/// then no synthetic operator-host probe artifacts are written to the workspace.
#[test]
fn production_tui_live_init_does_not_write_synthetic_probe_artifacts() {
    // arrange
    let workspace = TempDir::new().expect("create temp workspace");
    let workspace_root = workspace.path();
    let run_dir = TempDir::new().expect("create temp run dir");

    // act
    let (_tx, rx) = mpsc::channel();
    let _ = run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx: rx,
            compact_session_supported: false,
        },
        exit_on_finish: true,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        workspace_root: Some(workspace_root.to_path_buf()),
    });

    // assert: no probe artifacts in workspace
    let created_artifacts: Vec<&str> = PROBE_ARTIFACT_RELATIVE_PATHS
        .iter()
        .filter(|relative| workspace_root.join(relative).exists())
        .copied()
        .collect();

    assert!(
        created_artifacts.is_empty(),
        "production TUI live init wrote synthetic probe artifacts to the workspace: {created_artifacts:?}\n\
         These should only be created by explicit `seed_operator_host_probes` calls in tests, \
         not by the production live init path `run_tui_with_options`."
    );

    // assert: no probe artifacts in run_dir
    let created_in_run_dir: Vec<&str> = PROBE_ARTIFACT_RELATIVE_PATHS
        .iter()
        .filter(|relative| run_dir.path().join(relative).exists())
        .copied()
        .collect();

    assert!(
        created_in_run_dir.is_empty(),
        "production TUI live init wrote synthetic probe artifacts to the run dir: {created_in_run_dir:?}"
    );
}
