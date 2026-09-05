//! End-to-end CLI proof for `harness attribution diff|blame` (vcs.edit_attribution_blame_diff_ux).
//!
//! Drives the full `harness attribution diff <path>` and `blame <path>` surface
//! in-process via [`harness::run`] so argument parsing → command dispatch → the
//! `harness_core::edit_attribution` backend → real filesystem effects are
//! exercised together.

use std::fs;
use std::io::Cursor;

use harness::{run, CliDeps, CliIo, UnwrapOrAbort};
use harness_core::edit_attribution::EditAttributionJournal;
use tempfile::tempdir;

fn write_file(root: &std::path::Path, rel: &str, content: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_abort();
    }
    fs::write(&path, content).unwrap_or_abort();
}

fn attribution_cli(workspace: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let mut stdin = Cursor::new(Vec::<u8>::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let mut argv: Vec<&str> = vec!["harness", "attribution"];
    argv.extend_from_slice(args);
    let outcome = run(
        argv,
        &mut io,
        CliDeps::real().with_current_dir(workspace.to_path_buf()),
    );
    (
        outcome.code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

fn parse_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim()).unwrap_or_abort()
}

#[test]
fn attribution_diff_produces_json_for_agent_edited_path() {
    // Given: agent wrote a file
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/main.rs", b"fn main() {}\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/main.rs", b"fn main() {}\n", None)
        .unwrap_or_abort();

    // When
    let (code, stdout, stderr) = attribution_cli(root, &["diff", "src/main.rs"]);

    // Then
    assert_eq!(code, 0, "stderr: {stderr}");
    let json = parse_json(&stdout);
    assert_eq!(json["path"].as_str().unwrap_or_abort(), "src/main.rs");
    assert!(!json["drifted"].as_bool().unwrap_or_abort());
    assert_eq!(json["agent_lines"].as_i64().unwrap_or_abort(), 1);
    assert_eq!(json["external_lines"].as_i64().unwrap_or_abort(), 0);
    assert!(json["agent_snapshot_sha256"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
}

#[test]
fn attribution_diff_shows_external_drift_separately() {
    // Given: agent wrote content, external modified it
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/drift.rs", b"line one\nline two\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/drift.rs", b"line one\nline two\n", None)
        .unwrap_or_abort();
    write_file(root, "src/drift.rs", b"line one\nEXTERNAL EDIT\n");

    // When
    let (code, stdout, stderr) = attribution_cli(root, &["diff", "src/drift.rs"]);

    // Then
    assert_eq!(code, 0, "stderr: {stderr}");
    let json = parse_json(&stdout);
    assert!(json["drifted"].as_bool().unwrap_or_abort());
    assert_eq!(json["agent_lines"].as_i64().unwrap_or_abort(), 1);
    assert_eq!(json["external_lines"].as_i64().unwrap_or_abort(), 1);
    assert!(json["unified_diff"]
        .as_str()
        .is_some_and(|s| s.contains("EXTERNAL EDIT")));
}

#[test]
fn attribution_blame_produces_json_for_agent_edited_path() {
    // Given
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/blame.rs", b"keep\nkeep\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/blame.rs", b"keep\nkeep\n", None)
        .unwrap_or_abort();

    // When
    let (code, stdout, stderr) = attribution_cli(root, &["blame", "src/blame.rs"]);

    // Then
    assert_eq!(code, 0, "stderr: {stderr}");
    let json = parse_json(&stdout);
    assert_eq!(json["path"].as_str().unwrap_or_abort(), "src/blame.rs");
    assert_eq!(json["agent_lines"].as_i64().unwrap_or_abort(), 2);
    assert_eq!(json["external_lines"].as_i64().unwrap_or_abort(), 0);
    let lines = json["lines"].as_array().unwrap_or_abort();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["source"].as_str().unwrap_or_abort(), "agent_tool");
    assert_eq!(lines[1]["source"].as_str().unwrap_or_abort(), "agent_tool");
}

#[test]
fn attribution_blame_shows_external_drift_lines_separately() {
    // Given: agent wrote 3 lines, external modified line 2
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/mixed.rs", b"line1\nline2\nline3\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/mixed.rs", b"line1\nline2\nline3\n", None)
        .unwrap_or_abort();
    write_file(root, "src/mixed.rs", b"line1\nEXTERNAL\nline3\n");

    // When
    let (code, stdout, stderr) = attribution_cli(root, &["blame", "src/mixed.rs"]);

    // Then
    assert_eq!(code, 0, "stderr: {stderr}");
    let json = parse_json(&stdout);
    assert_eq!(json["agent_lines"].as_i64().unwrap_or_abort(), 2);
    assert_eq!(json["external_lines"].as_i64().unwrap_or_abort(), 1);
    let lines = json["lines"].as_array().unwrap_or_abort();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["source"].as_str().unwrap_or_abort(), "agent_tool");
    assert_eq!(lines[1]["source"].as_str().unwrap_or_abort(), "external");
    assert_eq!(lines[2]["source"].as_str().unwrap_or_abort(), "agent_tool");
    assert_eq!(lines[1]["content"].as_str().unwrap_or_abort(), "EXTERNAL");
}

#[test]
fn attribution_diff_fails_closed_for_unknown_path() {
    // Given
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();

    // When
    let (code, stdout, stderr) = attribution_cli(root, &["diff", "never-seen.rs"]);

    // Then
    assert_ne!(code, 0);
    assert!(stdout.is_empty() || !stdout.contains("\"path\""));
    assert!(stderr.contains("attribution:"));
    assert!(stderr.contains("no attribution record"));
}

#[test]
fn attribution_blame_fails_closed_for_unknown_path() {
    // Given
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();

    // When
    let (code, _stdout, stderr) = attribution_cli(root, &["blame", "never-seen.rs"]);

    // Then
    assert_ne!(code, 0);
    assert!(stderr.contains("attribution:"));
    assert!(stderr.contains("no attribution record"));
}

#[test]
fn attribution_diff_fails_closed_for_external_only_path() {
    // Given: path observed as external only (no agent snapshot)
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/ext.rs", b"external\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .observe_external("src/ext.rs", b"external\n", None)
        .unwrap_or_abort();

    // When
    let (code, _stdout, stderr) = attribution_cli(root, &["diff", "src/ext.rs"]);

    // Then
    assert_ne!(code, 0);
    assert!(stderr.contains("no agent snapshot"));
}

#[test]
fn attribution_blame_fails_closed_for_external_only_path() {
    // Given
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/ext2.rs", b"external\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .observe_external("src/ext2.rs", b"external\n", None)
        .unwrap_or_abort();

    // When
    let (code, _stdout, stderr) = attribution_cli(root, &["blame", "src/ext2.rs"]);

    // Then
    assert_ne!(code, 0);
    assert!(stderr.contains("no agent snapshot"));
}

#[test]
fn attribution_diff_with_explicit_workspace_flag() {
    // Given
    let dir = tempdir().unwrap_or_abort();
    let root = dir.path();
    write_file(root, "src/wf.rs", b"content\n");
    let mut journal = EditAttributionJournal::open(root).unwrap_or_abort();
    journal
        .record_agent_tool_edit("src/wf.rs", b"content\n", None)
        .unwrap_or_abort();

    // When: run from a different cwd with --workspace flag
    let other_dir = tempdir().unwrap_or_abort();
    let mut stdin = Cursor::new(Vec::<u8>::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let outcome = run(
        [
            "harness",
            "attribution",
            "diff",
            "src/wf.rs",
            "--workspace",
            root.to_str().unwrap_or_abort(),
        ],
        &mut io,
        CliDeps::real().with_current_dir(other_dir.path().to_path_buf()),
    );

    // Then
    assert_eq!(
        outcome.code,
        0,
        "stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let json = parse_json(&String::from_utf8_lossy(&stdout));
    assert_eq!(json["path"].as_str().unwrap_or_abort(), "src/wf.rs");
    assert!(!json["drifted"].as_bool().unwrap_or_abort());
}
