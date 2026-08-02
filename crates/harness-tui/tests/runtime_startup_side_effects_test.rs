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

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, SCHEMA_VERSION,
};
use harness_tui::{run_tui_with_options, LiveUpdate, TuiMode, TuiOptions};
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

fn completed_live_update() -> LiveUpdate {
    LiveUpdate::Event(Box::new(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-runtime-side-effect-finished".to_string(),
        seq: 1,
        run_id: "run-runtime-side-effect".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(
            ActorKind::System,
            Some("runtime-side-effect-test".to_string()),
        ),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run-runtime-side-effect".to_string()),
        payload: EventV1::RunFinished(RunFinishedEvent {
            summary: "test complete".to_string(),
        }),
    }))
}

/// Given an empty temp workspace, when production TUI startup runs, then no
/// synthetic operator-host probe artifacts are written to the workspace.
#[test]
fn production_tui_startup_does_not_write_synthetic_probe_artifacts() {
    // arrange
    let workspace = TempDir::new().expect("create temp workspace");
    let workspace_root = workspace.path();

    // act
    let (tx, rx) = mpsc::channel();
    drop(tx);
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
        skip_alternate_screen: false,
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
    let (tx, rx) = mpsc::channel();
    tx.send(completed_live_update())
        .expect("queue completed live update");
    drop(tx);
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
        skip_alternate_screen: false,
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

fn collect_file_snapshot(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    collect_file_snapshot_recursive(dir, dir, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn collect_file_snapshot_recursive(base: &Path, current: &Path, out: &mut Vec<(PathBuf, u64)>) {
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            collect_file_snapshot_recursive(base, &path, out);
        } else {
            let relative = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
            out.push((relative, meta.len()));
        }
    }
}

/// Given a populated temp workspace, when production TUI startup runs, then
/// existing files and symlinks are preserved byte-identical and no probe artifacts
/// are written.
#[test]
fn production_tui_startup_preserves_existing_files_and_symlinks() {
    // arrange
    let workspace = TempDir::new().expect("create temp workspace");
    let workspace_root = workspace.path();

    let target_file = workspace_root.join("target.txt");
    std::fs::write(&target_file, "target bytes").expect("write target file");

    let existing_file = workspace_root.join("existing.txt");
    std::fs::write(&existing_file, "existing bytes").expect("write existing file");

    let symlink = workspace_root.join("link_to_target.rs");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target_file, &symlink).expect("create symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target_file, &symlink).expect("create symlink");

    let before = collect_file_snapshot(workspace_root);

    // act
    let (tx, rx) = mpsc::channel();
    drop(tx);
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
        skip_alternate_screen: false,
    });

    // assert
    let after = collect_file_snapshot(workspace_root);
    assert_eq!(
        before, after,
        "production TUI startup changed the workspace file set or sizes: before={before:?}, after={after:?}"
    );

    assert_eq!(
        std::fs::read_to_string(&existing_file).expect("read existing file"),
        "existing bytes",
        "existing file was overwritten"
    );
    assert_eq!(
        std::fs::read_to_string(&symlink).expect("read through symlink"),
        "target bytes",
        "symlink target was overwritten"
    );

    for relative in PROBE_ARTIFACT_RELATIVE_PATHS {
        assert!(
            !workspace_root.join(relative).exists(),
            "probe artifact {relative} was written during startup"
        );
    }
}

/// Given a populated temp workspace, when production TUI live initialization
/// runs, then workspace files are preserved and the run dir does not receive
/// synthetic probe artifacts.
#[test]
fn production_tui_live_init_preserves_existing_workspace_files() {
    // arrange
    let workspace = TempDir::new().expect("create temp workspace");
    let workspace_root = workspace.path();
    let run_dir = TempDir::new().expect("create temp run dir");

    let existing_file = workspace_root.join("existing.txt");
    std::fs::write(&existing_file, "existing bytes").expect("write existing file");
    let before = collect_file_snapshot(workspace_root);

    // act
    let (tx, rx) = mpsc::channel();
    tx.send(completed_live_update())
        .expect("queue completed live update");
    drop(tx);
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
        skip_alternate_screen: false,
    });

    // assert
    let after = collect_file_snapshot(workspace_root);
    assert_eq!(
        before, after,
        "production TUI live init changed the workspace file set or sizes: before={before:?}, after={after:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&existing_file).expect("read existing file"),
        "existing bytes",
        "existing workspace file was overwritten during live init"
    );

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
