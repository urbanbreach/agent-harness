use harness_core::UnwrapOrAbort;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::clock::FakeClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{
    EventEnvelopeV1, EventV1, ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus,
};
use harness_core::proj::inspect_resume_plan;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

mod common;
use common::{
    allow_all_permission_policy, load_events, supervisor_actor, wait_for_tool_call_finish,
};

struct SpawnMetadataTool {
    id: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnMetadataArgs {
    child_session_id: String,
    child_request_id: String,
}

#[async_trait]
impl Tool for SpawnMetadataTool {
    fn id(&self) -> &str {
        self.id
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SpawnMetadataArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        Ok(ToolResult::structured(
            format!("delegated to {}", args.child_session_id),
            json!({
                "child_session_id": args.child_session_id,
                "child_request_id": args.child_request_id,
                "mode": "background",
                "status": "scheduled",
            }),
        ))
    }
}

struct BatchMetadataTool {
    id: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchMetadataArgs {
    tools: Vec<String>,
}

#[async_trait]
impl Tool for BatchMetadataTool {
    fn id(&self) -> &str {
        self.id
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BatchMetadataArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        let details = args
            .tools
            .into_iter()
            .enumerate()
            .map(|(index, tool_id)| {
                json!({
                    "index": index,
                    "tool_id": tool_id,
                    "success": true,
                    "status": "succeeded",
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolResult::structured(
            "batch complete",
            json!({
                "successful": details.len(),
                "failed": 0,
                "execution": {
                    "concurrency": "parallel",
                    "result_order": "input",
                    "nested_batch_disallowed": true,
                },
                "details": details,
            }),
        ))
    }
}

#[tokio::test]
async fn replay_preserves_batch_and_child_task_metadata_for_native_and_compat_paths() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_root).unwrap_or_abort();

    let coordinator = test_coordinator(temp_dir.path());
    let run = coordinator
        .start_run(
            "native_and_compat_replay_metadata",
            PathBuf::from(&workspace_root),
        )
        .await
        .unwrap_or_abort();

    let native_spawn_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "agent.spawn",
            json!({
                "child_session_id": "child-native-session",
                "child_request_id": "child-native-request",
            }),
        )
        .await
        .unwrap_or_abort();

    let compat_task_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "task",
            json!({
                "child_session_id": "child-compat-session",
                "child_request_id": "child-compat-request",
            }),
        )
        .await
        .unwrap_or_abort();

    let native_batch_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "tool.batch",
            json!({
                "tools": ["fs.read", "search.web", "todo.read"],
            }),
        )
        .await
        .unwrap_or_abort();

    let compat_batch_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "batch",
            json!({
                "tools": ["todo.write", "todo.read"],
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_tool_call_finish(&run.events_path, &native_spawn_call_id).await;
    wait_for_tool_call_finish(&run.events_path, &compat_task_call_id).await;
    wait_for_tool_call_finish(&run.events_path, &native_batch_call_id).await;
    wait_for_tool_call_finish(&run.events_path, &compat_batch_call_id).await;

    coordinator.stop_run().await.unwrap_or_abort();
    let events = load_events(&run.events_path);

    assert_tool_metadata(
        &events,
        &native_spawn_call_id,
        "agent.spawn",
        None,
        Some("child-native-session"),
        Some("child-native-request"),
    );
    assert_tool_metadata(
        &events,
        &compat_task_call_id,
        "task",
        None,
        Some("child-compat-session"),
        Some("child-compat-request"),
    );
    assert_tool_metadata(
        &events,
        &native_batch_call_id,
        "tool.batch",
        None,
        None,
        None,
    );
    assert_tool_metadata(&events, &compat_batch_call_id, "batch", None, None, None);

    let native_batch_finished = find_finished(&events, &native_batch_call_id);
    let native_batch_output = native_batch_finished.output_json.as_ref().unwrap_or_abort();
    assert_eq!(
        native_batch_output.pointer("/execution/concurrency"),
        Some(&json!("parallel"))
    );
    assert_eq!(
        native_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        native_batch_output.pointer("/details/0/tool_id"),
        Some(&json!("fs.read"))
    );
    assert_eq!(
        native_batch_output.pointer("/details/1/tool_id"),
        Some(&json!("search.web"))
    );
    assert_eq!(
        native_batch_output.pointer("/details/2/tool_id"),
        Some(&json!("todo.read"))
    );

    let compat_batch_finished = find_finished(&events, &compat_batch_call_id);
    let compat_batch_output = compat_batch_finished.output_json.as_ref().unwrap_or_abort();
    assert_eq!(
        compat_batch_output.pointer("/details/0/tool_id"),
        Some(&json!("todo.write"))
    );
    assert_eq!(
        compat_batch_output.pointer("/details/1/tool_id"),
        Some(&json!("todo.read"))
    );

    let plan = inspect_resume_plan(&run.run_dir);

    assert_replay_tool(
        &plan,
        &native_spawn_call_id,
        "agent.spawn",
        "agent.spawn",
        None,
        Some("child-native-session"),
        Some("child-native-request"),
    );
    assert_replay_tool(
        &plan,
        &compat_task_call_id,
        "task",
        "task",
        None,
        Some("child-compat-session"),
        Some("child-compat-request"),
    );
    assert_replay_tool(
        &plan,
        &native_batch_call_id,
        "tool.batch",
        "tool.batch",
        None,
        None,
        None,
    );
    assert_replay_tool(
        &plan,
        &compat_batch_call_id,
        "batch",
        "batch",
        None,
        None,
        None,
    );

    let replay_native_batch = plan.tool_calls.get(&native_batch_call_id).unwrap_or_abort();
    let replay_native_batch_output = replay_native_batch.output_json.as_ref().unwrap_or_abort();
    assert_eq!(
        replay_native_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        replay_native_batch_output.pointer("/details/0/tool_id"),
        Some(&json!("fs.read"))
    );

    let replay_compat_batch = plan.tool_calls.get(&compat_batch_call_id).unwrap_or_abort();
    let replay_compat_batch_output = replay_compat_batch.output_json.as_ref().unwrap_or_abort();
    assert_eq!(
        replay_compat_batch_output.pointer("/details/0/tool_id"),
        Some(&json!("todo.write"))
    );

    let native_completed = plan
        .completed_tasks
        .values()
        .find(|snapshot| {
            snapshot
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.lineage.as_ref())
                .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
                == Some(native_spawn_call_id.as_str())
        })
        .unwrap_or_abort();
    assert_eq!(
        native_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some("child-native-request")
    );

    let compat_completed = plan
        .completed_tasks
        .values()
        .find(|snapshot| {
            snapshot
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.lineage.as_ref())
                .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
                == Some(compat_task_call_id.as_str())
        })
        .unwrap_or_abort();
    assert_eq!(
        compat_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some("child-compat-session")
    );
}

fn assert_tool_metadata(
    events: &[EventEnvelopeV1],
    tool_call_id: &str,
    canonical_tool_id: &str,
    alias_source_tool_id: Option<&str>,
    child_session_id: Option<&str>,
    child_request_id: Option<&str>,
) {
    let requested = find_requested(events, tool_call_id);
    let requested_metadata = requested.metadata.as_ref().unwrap_or_abort();
    assert_eq!(
        requested_metadata.canonical_tool_id.as_deref(),
        Some(canonical_tool_id)
    );
    assert_eq!(
        requested_metadata.alias_source_tool_id.as_deref(),
        alias_source_tool_id
    );

    let finished = find_finished(events, tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let finished_metadata = finished.metadata.as_ref().unwrap_or_abort();
    assert_eq!(
        finished_metadata.canonical_tool_id.as_deref(),
        Some(canonical_tool_id)
    );
    assert_eq!(
        finished_metadata.alias_source_tool_id.as_deref(),
        alias_source_tool_id
    );
    assert_eq!(
        finished_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        child_session_id
    );
    assert_eq!(
        finished_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        child_request_id
    );
}

fn assert_replay_tool(
    plan: &harness_core::proj::ResumePlan,
    tool_call_id: &str,
    invoked_tool_id: &str,
    canonical_tool_id: &str,
    alias_source_tool_id: Option<&str>,
    child_session_id: Option<&str>,
    child_request_id: Option<&str>,
) {
    let replay_tool = plan.tool_calls.get(tool_call_id).unwrap_or_abort();
    assert_eq!(replay_tool.tool_id.as_deref(), Some(invoked_tool_id));
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some(invoked_tool_id)
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some(canonical_tool_id)
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        Some(canonical_tool_id)
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.alias_source_tool_id.as_deref()),
        alias_source_tool_id
    );
    assert_eq!(
        replay_tool.lifecycle_state,
        Some(harness_core::event::ToolCallLifecycleState::Completed)
    );
    assert_eq!(replay_tool.status, Some(ToolCallStatus::Succeeded));
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some(canonical_tool_id)
    );
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        alias_source_tool_id
    );
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        child_session_id
    );
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        child_request_id
    );
}

fn test_coordinator(session_dir: &Path) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.deterministic_store = true;
    config.permission_policy = allow_all_permission_policy();
    config.tool_registry = test_registry();

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn test_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SpawnMetadataTool { id: "agent.spawn" }));
    registry.register(Arc::new(SpawnMetadataTool { id: "task" }));
    registry.register(Arc::new(BatchMetadataTool { id: "tool.batch" }));
    registry.register(Arc::new(BatchMetadataTool { id: "batch" }));
    Arc::new(registry)
}

fn find_requested(events: &[EventEnvelopeV1], tool_call_id: &str) -> ToolCallRequestedEvent {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(payload) if payload.tool_call_id == tool_call_id => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort()
}

fn find_finished(events: &[EventEnvelopeV1], tool_call_id: &str) -> ToolCallFinishedEvent {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort()
}
