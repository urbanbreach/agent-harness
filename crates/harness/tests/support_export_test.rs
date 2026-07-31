//! T23 dedicated support export contract tests.
//!
//! Uses `harness::execute_session_export_with_io` (CliIo/CliDeps in-process
//! path) to prove the replay-derived support bundle contract, offline doctor,
//! agent catalog metadata, redaction manifests, secret scan, and fail-closed
//! behavior — all without binary spawn or network calls.

use harness::{CliDeps, UnwrapOrAbort};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    ToolCallFinishedEvent, ToolCallStatus, SCHEMA_VERSION,
};
use std::fs;
use tempfile::tempdir;

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

fn write_events(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn run_export(
    session_name: &str,
    output_path: &std::path::Path,
    session_dir: &std::path::Path,
    config_path: Option<&std::path::Path>,
    deps: &CliDeps,
) -> (i32, String, String) {
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let code = harness::execute_session_export_with_io(
        session_name.to_string(),
        output_path.to_path_buf(),
        config_path.map(std::path::PathBuf::from),
        Some(session_dir.to_path_buf()),
        &mut stdout,
        &mut stderr,
        deps,
    );
    (
        code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

fn minimal_workspace() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let workspace = tempdir().unwrap_or_abort();
    let config_path = workspace.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"{
            "provider": {
                "test": {
                    "type": "openai_compatible",
                    "baseURL": "http://127.0.0.1:1/v1",
                    "apiKey": "sk-t23-test-key-DO_NOT_LEAK",
                    "models": { "test-model": { "name": "Test" } }
                }
            },
            "model": "test/test-model",
            "default_agent": "build",
            "agent": { "build": { "enable": true, "model": "test/test-model" } },
            "permission": "allow"
        }"#,
    )
    .unwrap_or_abort();
    let session_dir = workspace.path().join(".agent-harness/sessions");
    (workspace, config_path, session_dir)
}

#[test]
fn support_export_produces_all_required_sections_via_in_process_cli_deps() {
    // arrange
    let (workspace, config_path, session_dir) = minimal_workspace();
    let run_dir = session_dir.join("run_t23_support");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events(
        &run_dir,
        &[
            envelope(
                "run_t23_support",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "t23-support".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_t23_support",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("support-export.json");
    let deps = CliDeps::real()
        .with_current_dir(workspace.path().to_path_buf())
        .with_env("HOME", workspace.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            workspace.path().join("config").to_string_lossy(),
        );

    // act
    let (code, _stdout, stderr) = run_export(
        "run_t23_support",
        &export_path,
        &session_dir,
        Some(&config_path),
        &deps,
    );

    // assert
    assert_eq!(code, 0, "export must succeed; stderr: {stderr}");
    let bundle: serde_json::Value =
        serde_json::from_slice(&fs::read(&export_path).unwrap_or_abort()).unwrap_or_abort();

    assert!(bundle["catalog"]["run_id"].is_string());
    assert!(bundle["replay"].is_object(), "replay section required");
    assert!(bundle["events"].is_array(), "events array required");
    let support = &bundle["support"];
    assert!(support.is_object(), "support section required");
    assert!(
        support["doctor_json"].is_object(),
        "offline doctor required"
    );
    assert!(
        support["config_summary"].is_object(),
        "config summary required"
    );
    assert!(
        support["provider_summary"].is_object(),
        "provider summary required"
    );
    assert!(
        support["agent_catalog_summary"].is_object(),
        "agent catalog summary required"
    );
    assert!(
        support["native_tool_catalog_summary"].is_object(),
        "native tool catalog summary required"
    );
    assert!(
        support["session_tool_readiness"].is_object(),
        "session tool readiness required"
    );
    assert!(
        support["redaction_manifest"].is_object(),
        "redaction manifest required"
    );
    assert!(
        support["secret_scan_status"].is_object(),
        "secret scan status required"
    );
    assert!(
        support["route_metadata"].is_array(),
        "route metadata array required"
    );
}

#[test]
fn support_export_offline_doctor_performs_no_network_probes() {
    // arrange
    let (workspace, config_path, session_dir) = minimal_workspace();
    let run_dir = session_dir.join("run_t23_doctor");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events(
        &run_dir,
        &[
            envelope(
                "run_t23_doctor",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "doctor-verify".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_t23_doctor",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("support-export-doctor.json");
    let deps = CliDeps::real()
        .with_current_dir(workspace.path().to_path_buf())
        .with_env("HOME", workspace.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            workspace.path().join("config").to_string_lossy(),
        );

    // act
    let (code, _stdout, stderr) = run_export(
        "run_t23_doctor",
        &export_path,
        &session_dir,
        Some(&config_path),
        &deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let bundle: serde_json::Value =
        serde_json::from_slice(&fs::read(&export_path).unwrap_or_abort()).unwrap_or_abort();
    assert_eq!(
        bundle["support"]["doctor_json"]["no_network_probes"], true,
        "offline doctor in export must not make network calls"
    );
}

#[test]
fn support_export_redacts_secret_api_key_shapes() {
    // arrange
    let (workspace, config_path, session_dir) = minimal_workspace();
    let run_dir = session_dir.join("run_t23_secrets");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events(
        &run_dir,
        &[
            envelope(
                "run_t23_secrets",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "secrets".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_t23_secrets",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("sk-t23-output-secret-DO_NOT_LEAK_0123456789".to_string()),
                    output_digest: Some("digest".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                "run_t23_secrets",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("support-export-secrets.json");
    let deps = CliDeps::real()
        .with_current_dir(workspace.path().to_path_buf())
        .with_env("HOME", workspace.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            workspace.path().join("config").to_string_lossy(),
        );

    // act
    let (code, _stdout, stderr) = run_export(
        "run_t23_secrets",
        &export_path,
        &session_dir,
        Some(&config_path),
        &deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let export_text = fs::read_to_string(&export_path).unwrap_or_abort();
    assert!(
        !export_text.contains("sk-t23-test-key-DO_NOT_LEAK"),
        "config apiKey must be redacted"
    );
    assert!(
        !export_text.contains("sk-t23-output-secret-DO_NOT_LEAK_0123456789"),
        "event output secret must be redacted"
    );
    let bundle: serde_json::Value = serde_json::from_str(&export_text).unwrap_or_abort();
    assert_eq!(bundle["support"]["redaction_manifest"]["status"], "clean");
    assert_eq!(bundle["support"]["secret_scan_status"]["status"], "clean");
    assert_eq!(
        bundle["support"]["secret_scan_status"]["secret_finding_count"],
        0
    );
    assert!(
        bundle["support"]["redaction_manifest"]["redacted_marker_count"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn support_export_session_tool_readiness_is_replay_derived() {
    // arrange
    let (workspace, config_path, session_dir) = minimal_workspace();
    let run_dir = session_dir.join("run_t23_readiness");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events(
        &run_dir,
        &[
            envelope(
                "run_t23_readiness",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "readiness".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_t23_readiness",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("support-export-readiness.json");
    let deps = CliDeps::real()
        .with_current_dir(workspace.path().to_path_buf())
        .with_env("HOME", workspace.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            workspace.path().join("config").to_string_lossy(),
        );

    // act
    let (code, _stdout, stderr) = run_export(
        "run_t23_readiness",
        &export_path,
        &session_dir,
        Some(&config_path),
        &deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let bundle: serde_json::Value =
        serde_json::from_slice(&fs::read(&export_path).unwrap_or_abort()).unwrap_or_abort();
    assert_eq!(
        bundle["support"]["session_tool_readiness"]["source"], "event_replay",
        "session tool readiness must be replay-derived"
    );
    assert_eq!(
        bundle["support"]["session_tool_readiness"]["available"],
        true
    );
    assert_eq!(
        bundle["support"]["session_tool_readiness"]["redacted_by_default"],
        true
    );
}

#[test]
fn support_export_fails_closed_for_missing_session_via_in_process_io() {
    // arrange
    let workspace = tempdir().unwrap_or_abort();
    let missing_session_dir = workspace.path().join("nonexistent-sessions");
    let export_path = workspace.path().join("should-not-exist.json");
    let deps = CliDeps::real()
        .with_current_dir(workspace.path().to_path_buf())
        .with_env("HOME", workspace.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            workspace.path().join("config").to_string_lossy(),
        );

    // act
    let (code, stdout, stderr) = run_export(
        "nonexistent-run",
        &export_path,
        &missing_session_dir,
        None,
        &deps,
    );

    // assert
    assert_ne!(code, 0, "export must fail for missing session");
    assert!(stdout.is_empty(), "no bundle output on failure");
    assert!(
        stderr.contains("failed") || stderr.contains("not found") || !stderr.is_empty(),
        "stderr must describe the failure: {stderr}"
    );
    assert!(!export_path.exists(), "no file must be written on failure");
}

#[test]
fn support_export_agent_catalog_source_is_harness_core() {
    // arrange
    let (workspace, config_path, session_dir) = minimal_workspace();
    let run_dir = session_dir.join("run_t23_catalog");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events(
        &run_dir,
        &[
            envelope(
                "run_t23_catalog",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "catalog".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_t23_catalog",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("support-export-catalog.json");
    let deps = CliDeps::real()
        .with_current_dir(workspace.path().to_path_buf())
        .with_env("HOME", workspace.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            workspace.path().join("config").to_string_lossy(),
        );

    // act
    let (code, _stdout, stderr) = run_export(
        "run_t23_catalog",
        &export_path,
        &session_dir,
        Some(&config_path),
        &deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let bundle: serde_json::Value =
        serde_json::from_slice(&fs::read(&export_path).unwrap_or_abort()).unwrap_or_abort();
    assert_eq!(
        bundle["support"]["agent_catalog_summary"]["source"],
        "harness_core::agent_catalog"
    );
    assert_eq!(
        bundle["support"]["native_tool_catalog_summary"]["source"],
        "harness_tools::tool_catalog"
    );
    let entries = bundle["support"]["agent_catalog_summary"]["entries"]
        .as_array()
        .unwrap_or_abort();
    assert!(
        entries
            .iter()
            .any(|e| e["id"] == "build" && e["role"] == "primary"),
        "build agent must appear in catalog with primary role"
    );
}
