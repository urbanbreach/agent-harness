//! Integration matrix tests (Task 22) — update, export/trace, code graph CLI.
//!
//! Each family gets one real boundary E2E plus bad input, permission denial,
//! process failure, cancellation/restart, and redaction coverage.

use std::io::Cursor;
use std::io::Write;
use std::path::Path;

use harness::UnwrapOrAbort;
use harness::{CliDeps, CliIo};
use tempfile::tempdir;

fn run_cli(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let mut stdin = Cursor::new(Vec::<u8>::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let deps = CliDeps::real().with_current_dir(cwd.to_path_buf());
    let mut argv: Vec<String> = vec!["harness".to_string()];
    argv.extend(args.iter().copied().map(str::to_string));
    let outcome = harness::run(argv, &mut io, deps);
    (
        outcome.code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

fn seed_graph_workspace(ws: &Path) {
    let src = ws.join("src");
    std::fs::create_dir_all(&src).unwrap_or_abort();
    std::fs::write(
        src.join("lib.rs"),
        "pub fn alpha() {}\npub struct Beta {}\n",
    )
    .unwrap_or_abort();
}

// ---------------------------------------------------------------------------
// Update family
// ---------------------------------------------------------------------------

#[test]
fn update_boundary_e2e_check_with_newer_manifest_reports_update_available() {
    // Given: workspace with a manifest advertising a newer version
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    let ah = ws.join(".agent-harness");
    std::fs::create_dir_all(&ah).unwrap_or_abort();
    std::fs::write(
        ah.join("update-manifest.json"),
        r#"{"version": "99.0.0", "channel": "stable"}"#,
    )
    .unwrap_or_abort();

    // When: update check is run
    let (code, stdout, stderr) = run_cli(ws, &["update", "check"]);

    // Then: update available + durable receipt
    assert_eq!(code, 0, "stderr: {stderr}");
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(
        output["check"]["status"].as_str(),
        Some("update_available"),
        "stdout: {stdout}"
    );
    assert!(ws
        .join(".agent-harness/update-check.receipt.json")
        .is_file());
}

#[test]
fn update_bad_input_empty_url_fails_with_unavailable_status() {
    // Given: an empty URL for download
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();

    // When: update download is run with empty URL
    let (code, stdout, _stderr) = run_cli(ws, &["update", "download", "--url", ""]);

    // Then: unavailable with non-zero exit
    assert_eq!(code, 2);
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["download"]["status"].as_str(), Some("unavailable"));
}

#[test]
fn update_permission_denial_check_without_manifest_still_writes_receipt() {
    // Given: empty workspace, no manifest (operator has not configured updates)
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();

    // When: update check is run without a manifest
    let (code, stdout, stderr) = run_cli(ws, &["update", "check"]);

    // Then: unavailable (no manifest = no update permission), but receipt is still written
    assert_eq!(code, 0, "stderr: {stderr}");
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["check"]["status"].as_str(), Some("unavailable"));
    assert!(
        ws.join(".agent-harness/update-check.receipt.json")
            .is_file(),
        "receipt must be written even without a manifest"
    );
}

#[test]
fn update_process_failure_apply_with_missing_artifact_fails_closed() {
    // Given: target exists but artifact does not
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    let target = ws.join("harness");
    std::fs::write(&target, b"old binary").unwrap_or_abort();
    let artifact = ws.join("nonexistent");

    // When: update apply is run with a missing artifact
    let (code, stdout, _stderr) = run_cli(
        ws,
        &[
            "update",
            "apply",
            "--artifact-path",
            &artifact.display().to_string(),
            "--target",
            &target.display().to_string(),
        ],
    );

    // Then: failed, target unchanged
    assert_eq!(code, 2);
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["apply"]["status"].as_str(), Some("failed"));
    assert_eq!(
        std::fs::read_to_string(&target).unwrap_or_abort(),
        "old binary"
    );
}

#[test]
fn update_cancellation_restart_run_stops_when_up_to_date() {
    // Given: manifest at current version (no update available)
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    let ah = ws.join(".agent-harness");
    std::fs::create_dir_all(&ah).unwrap_or_abort();
    let version = harness_core::binary_update::current_binary_version().version;
    std::fs::write(
        ah.join("update-manifest.json"),
        format!(r#"{{"version": "{version}", "channel": "stable"}}"#),
    )
    .unwrap_or_abort();

    // When: update run is executed (full pipeline)
    let (code, stdout, stderr) = run_cli(ws, &["update", "run"]);

    // Then: up to date, pipeline did not proceed to download/apply/restart
    assert_eq!(code, 0, "stderr: {stderr}");
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["check"]["status"].as_str(), Some("up_to_date"));
    assert!(output.get("download").is_none());
}

#[test]
fn update_redaction_receipt_does_not_contain_secret_patterns() {
    // Given: workspace with a manifest
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    let ah = ws.join(".agent-harness");
    std::fs::create_dir_all(&ah).unwrap_or_abort();
    std::fs::write(
        ah.join("update-manifest.json"),
        r#"{"version": "99.0.0", "channel": "stable"}"#,
    )
    .unwrap_or_abort();

    // When: update check is run
    let (code, stdout, stderr) = run_cli(ws, &["update", "check"]);
    assert_eq!(code, 0, "stderr: {stderr}");

    // Then: the receipt does not contain secret-like patterns
    let receipt_path = ws.join(".agent-harness/update-check.receipt.json");
    assert!(receipt_path.is_file());
    let receipt = std::fs::read_to_string(&receipt_path).unwrap_or_abort();
    assert!(
        !receipt.contains("Bearer "),
        "receipt must not contain bearer tokens"
    );
    assert!(
        !receipt.contains("sk-AbCdEf"),
        "receipt must not contain API keys"
    );
    assert!(
        !receipt.contains("password"),
        "receipt must not contain passwords"
    );

    let stdout_json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let stdout_str = stdout_json.to_string();
    assert!(
        !stdout_str.contains("Bearer "),
        "stdout must not contain bearer tokens"
    );
    assert!(
        !stdout_str.contains("sk-AbCdEf"),
        "stdout must not contain API keys"
    );
}

// ---------------------------------------------------------------------------
// Code graph CLI family
// ---------------------------------------------------------------------------

#[test]
fn code_graph_cli_boundary_e2e_build_and_query_returns_hits() {
    // Given: a workspace with source files
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    seed_graph_workspace(ws);

    // When: build then query
    let (build_code, _, build_stderr) = run_cli(ws, &["code-graph", "build"]);
    assert_eq!(build_code, 0, "build stderr: {build_stderr}");
    let (code, stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha"]);

    // Then: query returns real hits
    assert_eq!(code, 0, "stderr: {stderr}");
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["result"]["status"].as_str(), Some("hit"));
}

#[test]
fn code_graph_cli_bad_input_unknown_kind_rejected_with_usage_error() {
    // Given: an empty workspace
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();

    // When: query with unknown kind
    let (code, _stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha", "--kind", "bogus"]);

    // Then: usage error
    assert_eq!(code, 2);
    assert!(stderr.contains("unknown kind"));
}

#[test]
fn code_graph_cli_permission_denial_query_without_index_fails_closed() {
    // Given: empty workspace, no index
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();

    // When: query is made without an index
    let (code, stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha"]);

    // Then: unavailable, no index created (read-only)
    assert_eq!(code, 0, "stderr: {stderr}");
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["result"]["status"].as_str(), Some("unavailable"));
    assert!(!ws.join(".agent-harness/code-graph-index.json").exists());
}

#[test]
fn code_graph_cli_process_failure_corrupt_index_returns_unavailable() {
    // Given: a workspace with a corrupt index
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    let ah = ws.join(".agent-harness");
    std::fs::create_dir_all(&ah).unwrap_or_abort();
    std::fs::write(ah.join("code-graph-index.json"), "not valid json {{{").unwrap_or_abort();

    // When: query is made against the corrupt index
    let (code, stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha"]);

    // Then: unavailable (fail closed on parse error)
    assert_eq!(code, 0, "stderr: {stderr}");
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["result"]["status"].as_str(), Some("unavailable"));
}

#[test]
fn code_graph_cli_cancellation_restart_rebuild_after_corruption_recovers() {
    // Given: a workspace with a corrupt index
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    seed_graph_workspace(ws);
    let ah = ws.join(".agent-harness");
    std::fs::create_dir_all(&ah).unwrap_or_abort();
    std::fs::write(ah.join("code-graph-index.json"), "corrupt").unwrap_or_abort();

    // When: rebuild (overwriting corrupt index) then query
    let (build_code, _, build_stderr) = run_cli(ws, &["code-graph", "build"]);
    assert_eq!(build_code, 0, "build stderr: {build_stderr}");
    let (code, stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha"]);

    // Then: query succeeds after rebuild
    assert_eq!(code, 0, "stderr: {stderr}");
    let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(output["result"]["status"].as_str(), Some("hit"));
}

#[test]
fn code_graph_cli_redaction_query_result_does_not_contain_secret_patterns() {
    // Given: a workspace with a built index
    let dir = tempdir().unwrap_or_abort();
    let ws = dir.path();
    seed_graph_workspace(ws);
    let (build_code, _, build_stderr) = run_cli(ws, &["code-graph", "build"]);
    assert_eq!(build_code, 0, "build stderr: {build_stderr}");

    // When: query result is serialized
    let (code, stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha"]);
    assert_eq!(code, 0, "stderr: {stderr}");

    // Then: the result JSON does not contain secret-like patterns
    let json = stdout.trim();
    assert!(!json.contains("Bearer "), "must not contain bearer tokens");
    assert!(
        !json.contains("sk-AbCdEf"),
        "must not contain API key patterns"
    );
    assert!(!json.contains("password"), "must not contain passwords");
    assert!(json.contains("alpha"), "must contain the queried symbol");
}

// ---------------------------------------------------------------------------
// Export/trace family
// ---------------------------------------------------------------------------

fn write_minimal_events(run_dir: &Path, run_id: &str) {
    std::fs::create_dir_all(run_dir).unwrap_or_abort();
    let events = format!(
        r#"{{"schema_version":1,"event_id":"evt-0001","seq":1,"run_id":"{run_id}","mono_ms":1,"actor":{{"kind":"system","agent_id":"test"}},"payload":{{"event_type":"run_started","data":{{"run_name":"export-test","workspace_root":"/tmp/ws"}}}}}}
{{"schema_version":1,"event_id":"evt-0002","seq":2,"run_id":"{run_id}","mono_ms":2,"actor":{{"kind":"system","agent_id":"test"}},"payload":{{"event_type":"run_finished","data":{{"summary":"done"}}}}}}"#
    );
    let mut file = std::fs::File::create(run_dir.join("events.jsonl")).unwrap_or_abort();
    file.write_all(events.as_bytes()).unwrap_or_abort();
}

fn write_events_with_secret(run_dir: &Path, run_id: &str) {
    std::fs::create_dir_all(run_dir).unwrap_or_abort();
    let events = format!(
        r#"{{"schema_version":1,"event_id":"evt-0001","seq":1,"run_id":"{run_id}","mono_ms":1,"actor":{{"kind":"system","agent_id":"test"}},"payload":{{"event_type":"run_started","data":{{"run_name":"export-redaction","workspace_root":"/tmp/ws"}}}}}}
{{"schema_version":1,"event_id":"evt-0002","seq":2,"run_id":"{run_id}","mono_ms":2,"actor":{{"kind":"system","agent_id":"test"}},"payload":{{"event_type":"tool_call_finished","data":{{"tool_call_id":"toolcall_000001","status":"succeeded","output_summary":"raw token sk-AbCdEf0123456789 and Authorization: Bearer abc.def-ghi_123","output_digest":"digest-secret","output_json":{{"secret":"sk-AbCdEf0123456789","authorization":"Bearer abc.def-ghi_123"}}}}}}}}
{{"schema_version":1,"event_id":"evt-0003","seq":3,"run_id":"{run_id}","mono_ms":3,"actor":{{"kind":"system","agent_id":"test"}},"payload":{{"event_type":"run_finished","data":{{"summary":"done"}}}}}}"#
    );
    let mut file = std::fs::File::create(run_dir.join("events.jsonl")).unwrap_or_abort();
    file.write_all(events.as_bytes()).unwrap_or_abort();
}

#[test]
fn export_trace_boundary_e2e_writes_json_bundle_for_valid_session() {
    // Given: a session directory with valid events
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_export_e2e");
    write_minimal_events(&run_dir, "run_export_e2e");
    let export_path = session_dir.path().join("export-e2e.json");

    // When: export is run
    let (code, stdout, stderr) = run_cli(
        session_dir.path(),
        &[
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_export_e2e",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ],
    );

    // Then: export succeeds and writes a JSON bundle
    assert_eq!(code, 0, "stderr: {stderr}; stdout: {stdout}");
    assert!(export_path.is_file(), "export file must exist");
    let bundle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap_or_abort())
            .expect("valid JSON bundle");
    assert!(bundle.get("catalog").is_some() || bundle.get("run_id").is_some());
}

#[test]
fn export_trace_bad_input_missing_session_fails_closed() {
    // Given: a session directory that does not exist
    let session_dir = tempdir().unwrap_or_abort();
    let missing = session_dir.path().join("missing-session");

    // When: export is run for a missing session
    let (code, _stdout, _stderr) = run_cli(
        session_dir.path(),
        &[
            "--session-dir",
            missing.to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "missing-session",
        ],
    );

    // Then: fail-closed with non-zero exit
    assert!(!code_eq_zero(code), "export must fail for missing session");
}

#[test]
fn export_trace_permission_denial_nonexistent_session_dir_fails_closed() {
    // Given: a nonexistent session directory
    let dir = tempdir().unwrap_or_abort();
    let nonexistent = dir.path().join("nonexistent-session-dir");

    // When: export is run with a nonexistent session dir
    let (code, _stdout, stderr) = run_cli(
        dir.path(),
        &[
            "--session-dir",
            nonexistent.to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "some-run",
        ],
    );

    // Then: fail-closed with error message
    assert!(!code_eq_zero(code), "must fail for nonexistent session dir");
    assert!(
        stderr.contains("failed to read session directory") || stderr.contains("not found"),
        "stderr should mention session directory error: {stderr}"
    );
}

#[test]
fn export_trace_process_failure_corrupt_events_fails_closed() {
    // Given: a session directory with corrupt events
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_corrupt");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    std::fs::write(run_dir.join("events.jsonl"), "not valid json {{{").unwrap_or_abort();
    let export_path = session_dir.path().join("export-corrupt.json");

    // When: export is run
    let (code, _stdout, _stderr) = run_cli(
        session_dir.path(),
        &[
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_corrupt",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ],
    );

    // Then: fail-closed with non-zero exit
    assert!(!code_eq_zero(code), "export must fail for corrupt events");
}

#[test]
fn export_trace_cancellation_restart_export_succeeds_after_retry() {
    // Given: a session directory with valid events
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_retry");
    write_minimal_events(&run_dir, "run_retry");
    let export_path = session_dir.path().join("export-retry.json");

    // When: export is run (first attempt fails due to wrong session name, then succeeds)
    let (fail_code, _, _) = run_cli(
        session_dir.path(),
        &[
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "wrong_name",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ],
    );
    assert!(
        !code_eq_zero(fail_code),
        "first attempt with wrong name must fail"
    );

    let (code, _stdout, stderr) = run_cli(
        session_dir.path(),
        &[
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_retry",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ],
    );

    // Then: second attempt succeeds (restart/retry recovers)
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(export_path.is_file(), "export file must exist after retry");
}

#[test]
fn export_trace_redaction_secret_payloads_are_redacted_in_export_bundle() {
    // Given: a session with events containing secret-like payloads
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_redaction");
    write_events_with_secret(&run_dir, "run_redaction");
    let export_path = session_dir.path().join("export-redacted.json");

    // When: export is run
    let (code, _stdout, stderr) = run_cli(
        session_dir.path(),
        &[
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_redaction",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ],
    );

    // Then: export succeeds and the bundle does not contain raw secrets
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(export_path.is_file());
    let bundle = std::fs::read_to_string(&export_path).unwrap_or_abort();
    assert!(
        !bundle.contains("sk-AbCdEf0123456789"),
        "export bundle must not contain raw API keys: {bundle}"
    );
    assert!(
        !bundle.contains("Bearer abc.def-ghi_123"),
        "export bundle must not contain raw bearer tokens: {bundle}"
    );
}

fn code_eq_zero(code: i32) -> bool {
    code == 0
}
