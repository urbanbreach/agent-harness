use crate::UnwrapOrAbort;
use std::sync::{Mutex, OnceLock};

use crate::config::{
    clear_registered_mcp_server_first_class_tool_ids,
    set_registered_mcp_server_first_class_tool_ids,
};
use crate::coord::tool_identity_metadata;
use crate::proj::inspect_resume_plan;

use super::*;

fn mcp_identity_registry_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(super) async fn mcp_effective_identity_persists_for_direct_and_wrapper_calls() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = allow_shell_permission_policy();

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("mcp_identity", temp_dir.path())
        .await
        .unwrap_or_abort();

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let direct_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "mcp.fixture.echo",
            json!({"text": "hello direct"}),
        )
        .await
        .unwrap_or_abort();
    let wrapper_call_id = handle
        .request_tool_call(
            actor,
            Some("deep".to_string()),
            "mcp.fixture.tool.call",
            json!({
                "tool": "echo",
                "arguments": { "text": "hello wrapper" },
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_events(
        &handle,
        &run.events_path,
        "wrapper MCP tool call finished",
        |event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == wrapper_call_id
            )
        },
    )
    .await;
    handle.stop_run().await.unwrap_or_abort();

    let events = read_events(&run.events_path);
    let direct_requested = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data) if data.tool_call_id.as_str() == direct_call_id => {
                Some(data)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(direct_requested.tool_id, "mcp.fixture.echo");
    assert_eq!(
        direct_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        direct_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );

    let wrapper_requested = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data) if data.tool_call_id.as_str() == wrapper_call_id => {
                Some(data)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(wrapper_requested.tool_id, "mcp.fixture.tool.call");
    assert_eq!(
        wrapper_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );

    let wrapper_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == wrapper_call_id => {
                Some(data)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(wrapper_finished.status, ToolCallStatus::Succeeded);
    assert_eq!(
        wrapper_finished
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_finished
            .output_json
            .as_ref()
            .and_then(|value| value.get("payload"))
            .and_then(|value| value.get("tool"))
            .and_then(|value| value.as_str()),
        Some("echo")
    );

    let resume_plan = inspect_resume_plan(&run.run_dir);
    let direct_snapshot = resume_plan
        .tool_calls
        .get(&direct_call_id)
        .unwrap_or_abort();
    assert_eq!(
        direct_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        direct_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        direct_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        None
    );
    assert_eq!(
        direct_snapshot.lifecycle_state,
        Some(crate::event::ToolCallLifecycleState::Completed)
    );
    assert_eq!(
        direct_snapshot
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    let wrapper_snapshot = resume_plan
        .tool_calls
        .get(&wrapper_call_id)
        .unwrap_or_abort();
    assert_eq!(
        wrapper_snapshot.tool_id.as_deref(),
        Some("mcp.fixture.tool.call")
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("mcp.fixture.tool.call")
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        None
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.alias_source_tool_id.as_deref()),
        None
    );
    assert_eq!(
        wrapper_snapshot.lifecycle_state,
        Some(crate::event::ToolCallLifecycleState::Completed)
    );
    assert_eq!(
        wrapper_snapshot
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_snapshot
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );
}

pub(super) fn mcp_effective_identity_uses_registered_first_class_ids_for_reserved_wrapper_names() {
    let _guard = mcp_identity_registry_test_lock().lock().unwrap_or_abort();
    clear_registered_mcp_server_first_class_tool_ids();
    set_registered_mcp_server_first_class_tool_ids(std::collections::BTreeMap::from([(
        "fixture".to_string(),
        std::collections::BTreeMap::from([(
            "tool.call".to_string(),
            "mcp.fixture.tool_call_2".to_string(),
        )]),
    )]));

    let wrapper_metadata = tool_identity_metadata(
        "mcp.fixture.tool.call",
        &json!({
            "tool": "tool.call",
            "arguments": { "text": "hello wrapper" },
        }),
    )
    .unwrap_or_abort();
    assert_eq!(
        wrapper_metadata.canonical_tool_id.as_deref(),
        Some("mcp.fixture.tool_call_2")
    );
    assert_eq!(wrapper_metadata.alias_source_tool_id.as_deref(), None);

    let direct_metadata = tool_identity_metadata(
        "mcp.fixture.tool_call_2",
        &json!({ "text": "hello direct" }),
    )
    .unwrap_or_abort();
    assert_eq!(
        direct_metadata.canonical_tool_id.as_deref(),
        Some("mcp.fixture.tool_call_2")
    );
    assert_eq!(direct_metadata.alias_source_tool_id.as_deref(), None);

    clear_registered_mcp_server_first_class_tool_ids();
}
