use harness_tools::UnwrapOrAbort;
use std::fs;
use std::path::Path;

use harness_core::config::ShellAllowlist;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFailedEvent, RunStartedEvent,
    TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata, SCHEMA_VERSION,
};
use harness_tools::coordinator_registry;
use serde_json::{json, Value};

mod common;

use common::{setup_workspace_fixture, test_context};

const SESSION_TOOL_IDS: [&str; 4] = [
    "session_list",
    "session_read",
    "session_search",
    "session_info",
];

#[test]
fn sessions_replay_docs_name_session_tools_and_no_side_effect_contract() {
    // arrange
    let docs = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/architecture/sessions-and-replay.md"),
    )
    .unwrap_or_abort();

    // act
    let undoc_tools: Vec<_> = SESSION_TOOL_IDS
        .iter()
        .filter(|tool_id| !docs.contains(*tool_id))
        .copied()
        .collect();
    let side_effects_doced =
        docs.contains("do not execute providers, tools, hooks, MCP, network, or CLI");

    // assert
    assert!(
        undoc_tools.is_empty(),
        "sessions docs missing tools {undoc_tools:?}"
    );
    assert!(
        side_effects_doced,
        "sessions docs should state replay inspection has no provider/tool/hook/MCP/network/CLI side effects"
    );
}

#[test]
fn session_tools_describe_replay_source_in_model_visible_surface() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    let offenders: Vec<_> = SESSION_TOOL_IDS
        .iter()
        .filter_map(|tool_id| {
            let tool = registry.get(tool_id).unwrap_or_abort();
            let description = tool.description();
            (!description.contains("replay-derived") && !description.contains("replay-safe"))
                .then_some((*tool_id, description.to_string()))
        })
        .collect();

    // assert
    assert!(
        offenders.is_empty(),
        "session tools should identify replay-only source: {offenders:?}"
    );
}

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_session_events(workspace: &Path, run_id: &str, events: &[EventEnvelopeV1]) {
    let run_dir = workspace.join(".agent-harness/sessions").join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn write_session_metadata(workspace: &Path, run_id: &str, metadata: Value) {
    let run_dir = workspace.join(".agent-harness/sessions").join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(
        run_dir.join("meta.json"),
        serde_json::to_string_pretty(&metadata).unwrap_or_abort(),
    )
    .unwrap_or_abort();
}

async fn session_info(workspace: &Path, run_id: &str, selector: &str) -> Value {
    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(workspace, run_id, "session-info-test");
    registry
        .get("session_info")
        .unwrap_or_abort()
        .call(ctx, json!({"session": selector}))
        .await
        .unwrap_or_abort()
        .structured_json
        .unwrap_or_abort()
}

fn run_started(run_id: &str, seq: u64, run_name: &str, workspace: &Path) -> EventEnvelopeV1 {
    envelope(
        run_id,
        seq,
        EventV1::RunStarted(RunStartedEvent {
            run_name: run_name.to_string().into(),
            workspace_root: workspace.display().to_string(),
        }),
    )
}

#[tokio::test]
async fn session_info_reports_failed_replay_only_child_lineage_and_missing_sessions() {
    // arrange
    let workspace = setup_workspace_fixture();
    write_session_events(
        workspace.workspace(),
        "run_failed_info",
        &[
            run_started("run_failed_info", 1, "failed info", workspace.workspace()),
            envelope(
                "run_failed_info",
                2,
                EventV1::RunFailed(RunFailedEvent {
                    error: "provider failed closed".to_string(),
                }),
            ),
        ],
    );
    write_session_events(
        workspace.workspace(),
        "run_replay_only",
        &[run_started(
            "run_replay_only",
            1,
            "replay only",
            workspace.workspace(),
        )],
    );
    write_session_events(
        workspace.workspace(),
        "run_child_lineage",
        &[
            run_started(
                "run_child_lineage",
                1,
                "child lineage",
                workspace.workspace(),
            ),
            envelope(
                "run_child_lineage",
                2,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_1".to_string().into(),
                    result_summary: "child done".to_string(),
                    result_digest: "digest-child".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_session_id: Some("run_parent".to_string()),
                            child_session_id: Some("run_child_lineage".to_string()),
                            ..TaskLineageMetadata::default()
                        }),
                        ..TaskCompletionMetadata::default()
                    }),
                }),
            ),
        ],
    );
    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(
        workspace.workspace(),
        "run-session-info-missing",
        "session-info-test",
    );

    // act
    let failed = session_info(
        workspace.workspace(),
        "run-session-info-failed",
        "run_failed_info",
    )
    .await;
    let replay_only = session_info(
        workspace.workspace(),
        "run-session-info-replay",
        "run_replay_only",
    )
    .await;
    let child = session_info(
        workspace.workspace(),
        "run-session-info-child",
        "run_child_lineage",
    )
    .await;
    let missing = registry
        .get("session_info")
        .unwrap_or_abort()
        .call(ctx, json!({"session": "missing_session"}))
        .await
        .expect_err("missing session should fail");

    // assert
    assert_eq!(failed.pointer("/catalog/status"), Some(&json!("failed")));
    assert_eq!(failed.pointer("/event_counts/run_failed"), Some(&json!(1)));
    assert_eq!(replay_only.pointer("/source"), Some(&json!("event_replay")));
    assert!(replay_only.get("metadata").is_some_and(Value::is_null));
    assert_eq!(
        child.pointer("/lineage/parent_session_id"),
        Some(&json!("run_parent"))
    );
    assert_eq!(
        child.pointer("/lineage/child_session_count"),
        Some(&json!(1))
    );
    assert!(missing
        .to_string()
        .contains("unknown session `missing_session`"));
}

#[tokio::test]
async fn session_info_spills_large_payloads() {
    // arrange
    let workspace = setup_workspace_fixture();
    write_session_events(
        workspace.workspace(),
        "run_large_info",
        &[run_started(
            "run_large_info",
            1,
            "large info",
            workspace.workspace(),
        )],
    );
    write_session_metadata(
        workspace.workspace(),
        "run_large_info",
        json!({
            "run_id": "run_large_info",
            "run_name": "large info",
            "workspace_root": workspace.workspace(),
            "profile_preset": "build",
            "provider": "default",
            "model": format!("model-{}", "x".repeat(30_000)),
            "mode_source": "prompt"
        }),
    );
    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(
        workspace.workspace(),
        "run-session-info-spill",
        "session-info-spill",
    );

    // act
    let spilled = registry
        .get("session_info")
        .unwrap_or_abort()
        .call(ctx, json!({"session": "run_large_info"}))
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(spilled.artifacts.len(), 1);
    let spill_json = spilled.structured_json.unwrap_or_abort();
    assert_eq!(spill_json.pointer("/spilled"), Some(&json!(true)));
    let artifact_path = spilled.artifacts[0].path.clone();
    let artifact_body =
        fs::read_to_string(workspace.workspace().join(&artifact_path)).unwrap_or_abort();
    assert!(artifact_body.contains("run_large_info"));
    assert!(artifact_body.contains("model-"));
}
