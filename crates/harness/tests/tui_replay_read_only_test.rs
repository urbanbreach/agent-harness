//! Wave 0 Packet 0.2 — Replay side-effect free and root-explicit.
//!
//! Regression tests verifying that:
//! - W0-P02-A: Replay resolves the workspace root from the recorded `RunStarted`
//!   event, not from `current_dir()`.
//! - W0-P02-B: A rootless replay stream (no `RunStarted`) does not fall back to
//!   `current_dir()` for write-capable paths.
//! - W0-P02-C: Replay startup does not mutate the recorded root directory or the
//!   launcher CWD (no writes, no tool/provider/network/hook execution).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use harness::UnwrapOrAbort;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    SCHEMA_VERSION,
};
use tempfile::tempdir;

#[path = "common/mod.rs"]
mod common;

use common::CliHarness;

// ---------------------------------------------------------------------------
// Event helpers
// ---------------------------------------------------------------------------

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

// ---------------------------------------------------------------------------
// Filesystem snapshot helpers
// ---------------------------------------------------------------------------

/// Snapshot all files under `root` as a map of relative path -> (size, modified).
/// Used to detect any writes during replay startup.
fn snapshot_files(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut map = BTreeMap::new();
    collect_files(root, root, &mut map);
    map
}

fn collect_files(base: &Path, dir: &Path, map: &mut BTreeMap<PathBuf, (u64, SystemTime)>) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap_or_abort() {
        let entry = entry.unwrap_or_abort();
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, map);
        } else if path.is_file() {
            let metadata = entry.metadata().unwrap_or_abort();
            let rel = path.strip_prefix(base).unwrap_or_abort().to_path_buf();
            map.insert(rel, (metadata.len(), metadata.modified().unwrap_or_abort()));
        }
    }
}

// ---------------------------------------------------------------------------
// W0-P02-A: Replay workspace root resolved from RunStarted, not current_dir()
// ---------------------------------------------------------------------------

#[test]
fn w0_p02_a_replay_workspace_root_resolved_from_run_started_not_cwd() {
    // arrange: a temp directory A as the recorded workspace root
    let workspace_a = tempdir().unwrap_or_abort();
    let workspace_a_path = workspace_a.path().canonicalize().unwrap_or_abort();

    // And: events containing RunStarted pointing to A
    let events = vec![
        envelope(
            "run_w0_p02_a",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "w0-p02-a".into(),
                workspace_root: workspace_a_path.to_string_lossy().to_string(),
            }),
        ),
        envelope(
            "run_w0_p02_a",
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    // act: deriving the workspace root from events
    let root = harness::replay_workspace_root_from_events(&events);

    // assert: the workspace root is A, not the process CWD
    assert_eq!(
        root.as_deref(),
        Some(workspace_a_path.as_path()),
        "replay workspace root must come from RunStarted event, not current_dir()"
    );
}

// ---------------------------------------------------------------------------
// W0-P02-B: Rootless stream does not fall back to current_dir()
// ---------------------------------------------------------------------------

#[test]
fn w0_p02_b_rootless_replay_stream_does_not_fall_back_to_cwd() {
    // arrange: events without a RunStarted event (rootless stream)
    let events = vec![envelope(
        "run_w0_p02_b",
        1,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "no run started event".to_string(),
        }),
    )];

    // act: deriving the workspace root from events
    let root = harness::replay_workspace_root_from_events(&events);

    // assert: no workspace root is derived (no CWD fallback)
    assert!(
        root.is_none(),
        "rootless replay stream must not fall back to current_dir() for write-capable paths"
    );
}

// ---------------------------------------------------------------------------
// W0-P02-C: Replay startup does not mutate filesystem
// Two sub-scenarios:
//   C1: early-return path (--exit-on-finish + terminal event) — no TUI launch
//   C2: full replay init path (no --exit-on-finish) — actual run_tui_with_options
// ---------------------------------------------------------------------------

fn run_replay_and_assert_no_mutation(
    workspace_a_path: &Path,
    cwd_b_path: &Path,
    run_dir: &Path,
    use_exit_on_finish: bool,
    label: &str,
) {
    // Snapshot filesystem before replay startup
    let workspace_a_before = snapshot_files(workspace_a_path);
    let cwd_b_before = snapshot_files(cwd_b_path);
    let run_dir_before = snapshot_files(run_dir);

    // arrange: build CLI args
    let mut args = vec![
        "tui".to_string(),
        "--replay".to_string(),
        run_dir.to_str().unwrap_or_abort().to_string(),
    ];
    if use_exit_on_finish {
        args.push("--exit-on-finish".to_string());
    }

    // act: launch replay from CWD B
    let output = CliHarness::new()
        .args(&args)
        .current_dir(cwd_b_path.to_path_buf())
        .output();

    // assert: replay exits without error (with --exit-on-finish) or errors gracefully
    // without mutating the filesystem
    if use_exit_on_finish {
        assert!(
            output.status.success(),
            "{label}: replay with --exit-on-finish should succeed\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // And: no files were created, modified, or deleted in workspace A
    let workspace_a_after = snapshot_files(workspace_a_path);
    assert_eq!(
        workspace_a_before, workspace_a_after,
        "{label}: replay must not mutate the recorded workspace root directory"
    );

    // And: no files were created, modified, or deleted in CWD B
    let cwd_b_after = snapshot_files(cwd_b_path);
    assert_eq!(
        cwd_b_before, cwd_b_after,
        "{label}: replay must not mutate the launcher CWD directory"
    );

    // And: no files were created, modified, or deleted in the run dir
    let run_dir_after = snapshot_files(run_dir);
    assert_eq!(
        run_dir_before, run_dir_after,
        "{label}: replay must not mutate the recorded run directory"
    );
}

#[test]
fn w0_p02_c1_replay_early_return_does_not_mutate_filesystem() {
    // arrange
    let workspace_a = tempdir().unwrap_or_abort();
    let workspace_a_path = workspace_a.path().canonicalize().unwrap_or_abort();
    std::fs::write(
        workspace_a_path.join("README.marker"),
        "workspace-a-marker\n",
    )
    .unwrap_or_abort();

    let cwd_b = tempdir().unwrap_or_abort();
    let cwd_b_path = cwd_b.path().canonicalize().unwrap_or_abort();
    std::fs::write(cwd_b_path.join("cwd.marker"), "cwd-b-marker\n").unwrap_or_abort();

    let run_dir = tempdir().unwrap_or_abort();
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_w0_p02_c1",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "w0-p02-c1".into(),
                    workspace_root: workspace_a_path.to_string_lossy().to_string(),
                }),
            ),
            envelope(
                "run_w0_p02_c1",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    // act
    // assert
    run_replay_and_assert_no_mutation(&workspace_a_path, &cwd_b_path, run_dir.path(), true, "C1");
}

#[test]
fn w0_p02_c2_replay_full_init_does_not_mutate_filesystem() {
    // arrange
    let workspace_a = tempdir().unwrap_or_abort();
    let workspace_a_path = workspace_a.path().canonicalize().unwrap_or_abort();
    std::fs::write(
        workspace_a_path.join("README.marker"),
        "workspace-a-marker\n",
    )
    .unwrap_or_abort();

    let cwd_b = tempdir().unwrap_or_abort();
    let cwd_b_path = cwd_b.path().canonicalize().unwrap_or_abort();
    std::fs::write(cwd_b_path.join("cwd.marker"), "cwd-b-marker\n").unwrap_or_abort();

    let run_dir = tempdir().unwrap_or_abort();
    write_events_jsonl(
        run_dir.path(),
        &[envelope(
            "run_w0_p02_c2",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "w0-p02-c2".into(),
                workspace_root: workspace_a_path.to_string_lossy().to_string(),
            }),
        )],
    );

    // act
    // assert
    run_replay_and_assert_no_mutation(&workspace_a_path, &cwd_b_path, run_dir.path(), false, "C2");
}

fn snapshot_contents(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut map = BTreeMap::new();
    collect_contents(root, root, &mut map);
    map
}

fn collect_contents(base: &Path, dir: &Path, map: &mut BTreeMap<PathBuf, Vec<u8>>) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap_or_abort() {
        let entry = entry.unwrap_or_abort();
        let path = entry.path();
        if path.is_dir() {
            collect_contents(base, &path, map);
        } else if path.is_file() {
            let rel = path.strip_prefix(base).unwrap_or_abort().to_path_buf();
            map.insert(rel, std::fs::read(&path).unwrap_or_abort());
        }
    }
}

#[test]
fn w0_p02_c3_rootless_replay_full_init_does_not_mutate_filesystem() {
    // arrange
    let cwd_b = tempdir().unwrap_or_abort();
    let cwd_b_path = cwd_b.path().canonicalize().unwrap_or_abort();
    std::fs::write(cwd_b_path.join("cwd.marker"), "cwd-b-marker\n").unwrap_or_abort();

    let run_dir = tempdir().unwrap_or_abort();
    write_events_jsonl(
        run_dir.path(),
        &[envelope(
            "run_w0_p02_c3",
            1,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "no run started event".to_string(),
            }),
        )],
    );

    let cwd_before = snapshot_contents(&cwd_b_path);
    let run_dir_before = snapshot_contents(run_dir.path());

    // act
    let output = CliHarness::new()
        .args(&[
            "tui".to_string(),
            "--replay".to_string(),
            run_dir.path().to_str().unwrap_or_abort().to_string(),
        ])
        .current_dir(cwd_b_path.to_path_buf())
        .output();

    // assert
    assert!(
        !output.status.success(),
        "rootless replay without --exit-on-finish should fail gracefully rather than synthesize a workspace"
    );

    let cwd_after = snapshot_contents(&cwd_b_path);
    let run_dir_after = snapshot_contents(run_dir.path());
    assert_eq!(
        cwd_before, cwd_after,
        "rootless replay must not mutate the launcher CWD directory"
    );
    assert_eq!(
        run_dir_before, run_dir_after,
        "rootless replay must not mutate the recorded run directory"
    );
}

#[test]
fn w0_p02_c4_replay_preserves_file_contents_byte_identical() {
    // arrange
    let workspace_a = tempdir().unwrap_or_abort();
    let workspace_a_path = workspace_a.path().canonicalize().unwrap_or_abort();
    std::fs::write(
        workspace_a_path.join("README.marker"),
        "workspace-a-marker\n",
    )
    .unwrap_or_abort();

    let cwd_b = tempdir().unwrap_or_abort();
    let cwd_b_path = cwd_b.path().canonicalize().unwrap_or_abort();
    std::fs::write(cwd_b_path.join("cwd.marker"), "cwd-b-marker\n").unwrap_or_abort();

    let run_dir = tempdir().unwrap_or_abort();
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_w0_p02_c4",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "w0-p02-c4".into(),
                    workspace_root: workspace_a_path.to_string_lossy().to_string(),
                }),
            ),
            envelope(
                "run_w0_p02_c4",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let workspace_before = snapshot_contents(&workspace_a_path);
    let cwd_before = snapshot_contents(&cwd_b_path);
    let run_dir_before = snapshot_contents(run_dir.path());

    // act
    let output = CliHarness::new()
        .args(&[
            "tui".to_string(),
            "--replay".to_string(),
            run_dir.path().to_str().unwrap_or_abort().to_string(),
            "--exit-on-finish".to_string(),
        ])
        .current_dir(cwd_b_path.to_path_buf())
        .output();

    // assert
    assert!(
        output.status.success(),
        "byte-identical replay should succeed\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let workspace_after = snapshot_contents(&workspace_a_path);
    let cwd_after = snapshot_contents(&cwd_b_path);
    let run_dir_after = snapshot_contents(run_dir.path());
    assert_eq!(
        workspace_before, workspace_after,
        "replay must preserve workspace file contents byte-identical"
    );
    assert_eq!(
        cwd_before, cwd_after,
        "replay must preserve launcher CWD file contents byte-identical"
    );
    assert_eq!(
        run_dir_before, run_dir_after,
        "replay must preserve run directory file contents byte-identical"
    );
}
