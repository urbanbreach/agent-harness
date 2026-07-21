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

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use harness_tui::{run_tui_with_options, TuiMode, TuiOptions};
use tempfile::TempDir;

/// Probe artifacts that `seed_operator_host_probes` writes to the workspace root.
/// Retained as documentation of the concrete artifacts that must never appear.
const PROBE_ARTIFACT_RELATIVE_PATHS: &[&str] = &[
    "harness.json",
    ".agent-harness/plans",
    ".harness-cow-probe",
    ".harness-sessions-probe",
    ".harness-foreign-probe-root",
    ".jj",
];

/// Returns true when `dir` contains no files or subdirectories at all.
/// A full-tree emptiness check is strictly stronger than checking named probe
/// paths: it proves zero filesystem writes of any kind during TUI init.
fn dir_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

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

    // assert: full-tree check — workspace must be completely empty after startup
    assert!(
        dir_is_empty(workspace_root),
        "production TUI startup wrote to the workspace; full tree must be empty.\n\
         Named probe paths checked: {PROBE_ARTIFACT_RELATIVE_PATHS:?}"
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

    // assert: full-tree workspace check — must be completely empty
    assert!(
        dir_is_empty(workspace_root),
        "production TUI live init wrote to the workspace; full tree must be empty.\n\
         Named probe paths checked: {PROBE_ARTIFACT_RELATIVE_PATHS:?}"
    );

    // assert: run_dir receives legitimate session writes, so only probe paths are checked
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
