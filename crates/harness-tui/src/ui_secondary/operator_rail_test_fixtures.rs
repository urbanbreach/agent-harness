use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::*;

pub(crate) fn operator_sidebar_config_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_mcp_server_connection_states(BTreeMap::new());
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());
    guard
}

pub(crate) fn operator_rail_test_app() -> AppState {
    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let mut app = AppState::new_live(Some("/tmp/sessions/run_fixture".into()), false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("w1".to_string()),
        ),
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "w1".to_string(),
            profile: "deep".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        2,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("w1".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_queue".to_string().into(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("tool:fs.read".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_1".to_string(),
            path: "src/ui_secondary.rs".to_string(),
            new_file_digest: "digest-edit-1".to_string(),
            diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
            diff_digest: Some("digest-edit-artifact-1".to_string()),
        }),
    ));
    app
}

pub(crate) fn operator_rail_modified_file_only_app() -> AppState {
    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let mut app = AppState::new_live(Some("/tmp/sessions/run_fixture".into()), false, None);
    app.ingest_event(operator_rail_test_event(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_1".to_string(),
            path: "demo.txt".to_string(),
            new_file_digest: "digest-edit-1".to_string(),
            diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
            diff_digest: Some("digest-edit-artifact-1".to_string()),
        }),
    ));
    app
}

pub(crate) fn operator_rail_activity_test_app() -> AppState {
    crate::app::set_pending_live_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
            .with_mode_label("Demo"),
    );

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_activity_request".to_string(),
        seq: 1,
        run_id: "run_fixture".into(),
        mono_ms: 10,
        ts: None,
        actor: harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        correlation_id: Some("req_activity".to_string()),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload: harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_activity".into(),
                text: "Inspect operator rail taxonomy".to_string(),
            },
        ),
    });
    app.ingest_event(operator_rail_test_event(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_mcp".into(),
                tool_id: "exa.search".to_string(),
                args_summary: r#"{"query":"ratatui sidebar"}"#.to_string(),
                args_digest: "digest-tool-mcp".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_mcp".into(),
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_mcp".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Returned 3 results".to_string()),
                output_digest: Some("digest-tool-mcp-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        5,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_web".into(),
                tool_id: "search.web".to_string(),
                args_summary: r#"{"query":"sidebar parity"}"#.to_string(),
                args_digest: "digest-tool-web".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        6,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_web".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Fetched sidebar examples".to_string()),
                output_digest: Some("digest-tool-web-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        7,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_batch".into(),
                tool_id: "tool.batch".to_string(),
                args_summary: r#"{"requested_call_count":2}"#.to_string(),
                args_digest: "digest-tool-batch".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("tool.batch".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        8,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_batch".into(),
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        9,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_lsp".into(),
                tool_id: "code.lsp".to_string(),
                args_summary: r#"{"operation":"goto_definition","path":"src/ui_secondary.rs"}"#
                    .to_string(),
                args_digest: "digest-tool-lsp".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event(
        10,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_lsp".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Found definition in src/ui_secondary.rs".to_string()),
                output_digest: Some("digest-tool-lsp-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            },
        ),
    ));
    app
}

pub(crate) fn operator_rail_test_event(
    seq: u64,
    actor: harness_core::event::EventActor,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt_{seq}"),
        seq,
        run_id: "run_fixture".into(),
        mono_ms: seq * 10,
        ts: None,
        actor,
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

pub(crate) fn operator_rail_test_event_with_correlation(
    seq: u64,
    actor: harness_core::event::EventActor,
    correlation_id: &str,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    let mut event = operator_rail_test_event(seq, actor, payload);
    event.correlation_id = Some(correlation_id.to_string());
    event
}
