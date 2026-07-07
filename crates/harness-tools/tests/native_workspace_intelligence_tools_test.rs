use harness_tools::UnwrapOrAbort;
use std::fs;
use std::path::Path;

mod common;

use common::{setup_workspace_fixture, test_context};
use harness_core::config::ShellAllowlist;
use harness_core::event::{
    ActorKind, AssistantMessageFinishedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderAssistantMessageMetadata, RunFinishedEvent, RunStartedEvent, ToolCallFinishedEvent,
    ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tools::coordinator_registry;
use serde_json::json;

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
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

fn write_session_metadata(workspace: &Path, run_id: &str, metadata: serde_json::Value) {
    let run_dir = workspace.join(".agent-harness/sessions").join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(
        run_dir.join("meta.json"),
        serde_json::to_string_pretty(&metadata).unwrap_or_abort(),
    )
    .unwrap_or_abort();
}

fn run_started(run_id: &str, seq: u64, run_name: &str, workspace: &Path) -> EventEnvelopeV1 {
    envelope(
        run_id,
        seq,
        EventV1::RunStarted(RunStartedEvent {
            run_name: run_name.to_string(),
            workspace_root: workspace.display().to_string(),
        }),
    )
}

#[cfg(unix)]
#[tokio::test]
async fn session_read_rejects_events_file_symlink_escape() {
    // arrange
    let workspace = setup_workspace_fixture();
    let outside_events = workspace.workspace().join("outside-events.jsonl");
    let event = run_started(
        "run_symlink_escape",
        1,
        "outside session data",
        workspace.workspace(),
    );
    fs::write(
        &outside_events,
        format!("{}\n", serde_json::to_string(&event).unwrap_or_abort()),
    )
    .unwrap_or_abort();
    let run_dir = workspace
        .workspace()
        .join(".agent-harness/sessions/run_symlink_escape");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    std::os::unix::fs::symlink(&outside_events, run_dir.join("events.jsonl")).unwrap_or_abort();

    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(
        workspace.workspace(),
        "run-session-symlink-test",
        "session-symlink-test",
    );

    // act
    let err = registry
        .get("session_read")
        .unwrap_or_abort()
        .call(ctx, json!({"session": "run_symlink_escape"}))
        .await
        .expect_err("session_read should reject escaped events symlink");

    // assert
    assert!(
        err.to_string().contains("outside session directory"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn session_list_filters_status_profile_resumable_text_and_sort_order() {
    // arrange
    let workspace = setup_workspace_fixture();
    write_session_events(
        workspace.workspace(),
        "run_alpha",
        &[run_started(
            "run_alpha",
            1,
            "Alpha workspace session",
            workspace.workspace(),
        )],
    );
    write_session_metadata(
        workspace.workspace(),
        "run_alpha",
        json!({
            "run_id": "run_alpha",
            "profile_preset": "build",
            "provider": "default",
            "model": "gpt-5.5",
            "mode_source": "interactive_live"
        }),
    );
    write_session_events(
        workspace.workspace(),
        "run_beta",
        &[
            run_started("run_beta", 1, "Beta prompt session", workspace.workspace()),
            envelope(
                "run_beta",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );
    write_session_metadata(
        workspace.workspace(),
        "run_beta",
        json!({
            "run_id": "run_beta",
            "profile_preset": "plan",
            "provider": "default",
            "model": "gpt-5.5",
            "mode_source": "prompt"
        }),
    );
    write_session_events(
        workspace.workspace(),
        "run_gamma",
        &[run_started(
            "run_gamma",
            1,
            "Gamma workspace session",
            workspace.workspace(),
        )],
    );
    write_session_metadata(
        workspace.workspace(),
        "run_gamma",
        json!({
            "run_id": "run_gamma",
            "profile_preset": "build",
            "provider": "default",
            "model": "gpt-5.5",
            "mode_source": "interactive_live"
        }),
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(
        workspace.workspace(),
        "run-session-list-filter-test",
        "session-list-filter-test",
    );

    // act
    let build_sessions = registry
        .get("session_list")
        .unwrap_or_abort()
        .call(
            ctx.clone(),
            json!({"profile": "build", "sort": "run_id_desc"}),
        )
        .await
        .unwrap_or_abort();
    let beta_session = registry
        .get("session_list")
        .unwrap_or_abort()
        .call(
            ctx,
            json!({
                "status": "finished",
                "profile": "plan",
                "resumable": false,
                "filter": "beta",
                "sort": "run_id_asc"
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    let build_json = build_sessions.structured_json.unwrap_or_abort();
    assert_eq!(
        build_json
            .pointer("/sessions/0/catalog/run_id")
            .and_then(serde_json::Value::as_str),
        Some("run_gamma")
    );
    assert_eq!(
        build_json
            .pointer("/sessions/1/catalog/run_id")
            .and_then(serde_json::Value::as_str),
        Some("run_alpha")
    );
    assert_eq!(
        build_json
            .get("returned_count")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );

    let beta_json = beta_session.structured_json.unwrap_or_abort();
    assert_eq!(
        beta_json
            .get("returned_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        beta_json
            .pointer("/sessions/0/catalog/run_id")
            .and_then(serde_json::Value::as_str),
        Some("run_beta")
    );
    assert_eq!(
        beta_json
            .pointer("/sessions/0/catalog/status")
            .and_then(serde_json::Value::as_str),
        Some("finished")
    );
}

#[tokio::test]
async fn model_visible_session_tools_are_replay_safe_redacted_and_capped() {
    // arrange
    let workspace = setup_workspace_fixture();
    write_session_events(
        workspace.workspace(),
        "run_session_tools",
        &[
            envelope(
                "run_session_tools",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "session-tools".to_string(),
                    workspace_root: workspace.workspace().display().to_string(),
                }),
            ),
            envelope(
                "run_session_tools",
                2,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req-1".to_string(),
                    text: "hello sk-test_secret_0123456789".to_string(),
                }),
            ),
            envelope(
                "run_session_tools",
                3,
                EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                    request_id: "req-1".to_string(),
                    tool_call_count: 1,
                    assistant_message: Some(ProviderAssistantMessageMetadata {
                        message_id: Some("msg-1".to_string()),
                        text_digest: Some("assistant-text-digest".to_string()),
                        reasoning_digest: Some("assistant-reasoning-digest".to_string()),
                    }),
                }),
            ),
            envelope(
                "run_session_tools",
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "tool-1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("hello tool summary".to_string()),
                    output_digest: Some("digest".to_string()),
                    output_json: Some(json!({
                        "raw": "sk-test_raw_tool_payload_0123456789",
                        "large": "x".repeat(512),
                    })),
                    metadata: None,
                }),
            ),
        ],
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(
        workspace.workspace(),
        "run-session-tool-test",
        "session-tool-test",
    );
    // act
    let listed = registry
        .get("session_list")
        .unwrap_or_abort()
        .call(ctx.clone(), json!({"limit": 5}))
        .await
        .unwrap_or_abort();
    // assert
    assert_eq!(
        listed
            .structured_json
            .as_ref()
            .and_then(|json| json.get("source"))
            .and_then(serde_json::Value::as_str),
        Some("event_replay")
    );
    assert_eq!(
        listed
            .structured_json
            .as_ref()
            .and_then(|json| json.get("returned_count"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    let list_capped = registry
        .get("session_list")
        .unwrap_or_abort()
        .call(ctx.clone(), json!({"limit": 999}))
        .await
        .unwrap_or_abort();
    assert_eq!(
        list_capped
            .structured_json
            .as_ref()
            .and_then(|json| json.get("effective_limit"))
            .and_then(serde_json::Value::as_u64),
        Some(200)
    );
    assert_eq!(
        list_capped
            .structured_json
            .as_ref()
            .and_then(|json| json.get("limit_clamped"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let read = registry
        .get("session_read")
        .unwrap_or_abort()
        .call(
            ctx.clone(),
            json!({"session": "run_session_tools", "eventLimit": 10}),
        )
        .await
        .unwrap_or_abort();
    let read_json = read.structured_json.unwrap_or_abort();
    assert_eq!(
        read_json
            .get("redacted")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(
        !serde_json::to_string(&read_json)
            .unwrap_or_abort()
            .contains("sk-test_secret_0123456789"),
        "session_read should redact secrets"
    );
    assert!(
        !serde_json::to_string(&read_json)
            .unwrap_or_abort()
            .contains("sk-test_raw_tool_payload_0123456789"),
        "session_read should not expose raw tool output secrets"
    );
    let read_events = read_json
        .get("events")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert!(
        read_events.iter().all(|event| event.get("assistant_message").is_none()),
        "session_read should expose assistant message metadata instead of the raw assistant_message field"
    );
    assert!(
        read_events
            .iter()
            .all(|event| event.get("output_json").is_none()),
        "session_read should expose tool output summaries/digests instead of raw output_json"
    );
    let read_message_window = registry
        .get("session_read")
        .unwrap_or_abort()
        .call(
            ctx.clone(),
            json!({
                "session": "run_session_tools",
                "eventOffset": 1,
                "eventLimit": 2,
                "messageOffset": 1,
                "messageLimit": 1
            }),
        )
        .await
        .unwrap_or_abort();
    let read_message_json = read_message_window.structured_json.unwrap_or_abort();
    assert_eq!(
        read_message_json
            .get("returned_event_count")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    assert_eq!(
        read_message_json
            .get("returned_message_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        read_message_json
            .pointer("/messages/0/event_type")
            .and_then(serde_json::Value::as_str),
        Some("assistant_message_finished")
    );
    assert_eq!(
        read_message_json
            .get("total_message_count")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    let read_capped = registry
        .get("session_read")
        .unwrap_or_abort()
        .call(
            ctx.clone(),
            json!({"session": "run_session_tools", "eventLimit": 999}),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(
        read_capped
            .structured_json
            .as_ref()
            .and_then(|json| json.get("effective_event_limit"))
            .and_then(serde_json::Value::as_u64),
        Some(200)
    );
    assert_eq!(
        read_capped
            .structured_json
            .as_ref()
            .and_then(|json| json.get("event_limit_clamped"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let search = registry
        .get("session_search")
        .unwrap_or_abort()
        .call(
            ctx.clone(),
            json!({"query": "hello", "session": "run_session_tools", "limit": 10}),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(
        search
            .structured_json
            .as_ref()
            .and_then(|json| json.get("returned_count"))
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    let search_capped = registry
        .get("session_search")
        .unwrap_or_abort()
        .call(
            ctx.clone(),
            json!({
                "query": "hello",
                "session": "run_session_tools",
                "limit": 999,
                "contextLimit": 999
            }),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(
        search_capped
            .structured_json
            .as_ref()
            .and_then(|json| json.get("effective_limit"))
            .and_then(serde_json::Value::as_u64),
        Some(200)
    );
    assert_eq!(
        search_capped
            .structured_json
            .as_ref()
            .and_then(|json| json.get("effective_context_limit"))
            .and_then(serde_json::Value::as_u64),
        Some(500)
    );
    assert_eq!(
        search_capped
            .structured_json
            .as_ref()
            .and_then(|json| json.get("context_limit_clamped"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    let no_match = registry
        .get("session_search")
        .unwrap_or_abort()
        .call(
            ctx.clone(),
            json!({"query": "does-not-appear", "session": "run_session_tools", "limit": 10}),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(
        no_match
            .structured_json
            .as_ref()
            .and_then(|json| json.get("returned_count"))
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );

    let info = registry
        .get("session_info")
        .unwrap_or_abort()
        .call(ctx.clone(), json!({"session": "run_session_tools"}))
        .await
        .unwrap_or_abort();
    assert_eq!(
        info.structured_json
            .as_ref()
            .and_then(|json| json.get("source"))
            .and_then(serde_json::Value::as_str),
        Some("event_replay")
    );

    let corrupt_dir = workspace
        .workspace()
        .join(".agent-harness/sessions/corrupt_session");
    fs::create_dir_all(&corrupt_dir).unwrap_or_abort();
    fs::write(
        corrupt_dir.join("events.jsonl"),
        "{\"schema_version\":1,\"event_id\":\"evt-bad\"\n",
    )
    .unwrap_or_abort();
    let corrupt_info = registry
        .get("session_info")
        .unwrap_or_abort()
        .call(ctx.clone(), json!({"session": "corrupt_session"}))
        .await
        .unwrap_or_abort();
    assert_eq!(
        corrupt_info.structured_json.as_ref().and_then(|json| {
            json.pointer("/recovery/parse_errors")
                .and_then(serde_json::Value::as_array)
                .map(|errors| errors.is_empty())
        }),
        Some(false)
    );

    let traversal_err = registry
        .get("session_read")
        .unwrap_or_abort()
        .call(ctx, json!({"session": "../outside"}))
        .await
        .expect_err("session traversal should be rejected");
    assert!(traversal_err.to_string().contains("parent traversal"));
}
