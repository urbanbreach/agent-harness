use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::{
    AgentProfile, ProviderCompactionFacts, ProviderContextCheckpoint,
    ProviderContextCheckpointMetadata, ProviderConversationTurn,
};
use harness_core::clock::FakeClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent, TaskCompletedEvent,
    TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, SCHEMA_VERSION,
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

struct DelegatingAliasTaskTool;
struct ReplayGuardTool;

static REPLAY_GUARD_TOOL_CALLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegatingAliasTaskArgs {
    child_session_id: String,
    child_request_id: String,
}

#[async_trait]
impl Tool for DelegatingAliasTaskTool {
    fn id(&self) -> &str {
        "task"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: DelegatingAliasTaskArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        let store = ctx
            .artifact_store()
            .map_err(|err| ToolError::Execution(format!("failed to open artifact store: {err}")))?;
        let artifact = store
            .write_text(
                "delegated/task-output.json",
                &format!(
                    "{{\"child_session_id\":\"{}\",\"child_request_id\":\"{}\"}}",
                    args.child_session_id, args.child_request_id
                ),
            )
            .map_err(|err| ToolError::Execution(format!("failed to write artifact: {err}")))?;

        Ok(ToolResult::structured_with_artifacts(
            format!("delegated work to child session {}", args.child_session_id),
            json!({
                "child_session_id": args.child_session_id,
                "child_request_id": args.child_request_id,
            }),
            vec![artifact],
        ))
    }
}

#[async_trait]
impl Tool for ReplayGuardTool {
    fn id(&self) -> &str {
        "replay.guard"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, _ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        REPLAY_GUARD_TOOL_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::text("guard invoked"))
    }
}

#[tokio::test]
async fn replay_preserves_native_tool_artifacts_and_task_lineage() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("create workspace root");

    let coordinator = test_coordinator(temp_dir.path(), task_alias_registry());
    let run = coordinator
        .start_run("native_metadata_replay", workspace_root)
        .await
        .expect("start run");

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "task",
            json!({
                "child_session_id": "child-run-001",
                "child_request_id": "child-req-001",
            }),
        )
        .await
        .expect("request delegated task");

    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);

    let requested = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(payload) if payload.tool_call_id == tool_call_id => {
                Some(payload)
            }
            _ => None,
        })
        .expect("tool call requested event");
    let requested_metadata = requested
        .metadata
        .as_ref()
        .expect("requested metadata should be present");
    assert_eq!(
        requested_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(requested_metadata.alias_source_tool_id.as_deref(), None);

    let finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id => {
                Some(payload)
            }
            _ => None,
        })
        .expect("tool call finished event");
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let finished_metadata = finished
        .metadata
        .as_ref()
        .expect("finished metadata should be present");
    assert_eq!(finished_metadata.canonical_tool_id.as_deref(), Some("task"));
    assert_eq!(finished_metadata.alias_source_tool_id.as_deref(), None);
    assert_eq!(
        finished_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some("child-run-001")
    );
    assert_eq!(
        finished_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some("child-req-001")
    );
    assert_eq!(finished_metadata.artifact_refs.len(), 1);
    let output_json = finished
        .output_json
        .as_ref()
        .expect("stable output json should be present");
    let harness = output_json
        .get("_harness")
        .expect("stable output json should include _harness metadata");
    assert_eq!(
        harness
            .get("artifact_refs")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    let completed = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(payload) => Some(payload),
            _ => None,
        })
        .expect("task completed event");
    let completed_metadata = completed
        .metadata
        .as_ref()
        .expect("task completion metadata should be present");
    assert_eq!(
        completed_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.parent_tool_call_id.as_deref()),
        Some(tool_call_id.as_str())
    );
    assert_eq!(
        completed_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some("child-run-001")
    );
    assert!(completed_metadata.timing.is_some());

    let artifact_written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ArtifactWritten(payload)
                if payload.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(payload)
            }
            _ => None,
        })
        .expect("artifact written event with tool call ref");
    assert_eq!(
        artifact_written
            .tool_metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("task")
    );

    let plan = inspect_resume_plan(&run.run_dir);
    let replay_tool = plan
        .tool_calls
        .get(&tool_call_id)
        .expect("resume plan should retain tool metadata");
    assert_eq!(replay_tool.tool_id.as_deref(), Some("task"));
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.alias_source_tool_id.as_deref()),
        None
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
        Some("task")
    );
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .map(|metadata| metadata.artifact_refs.len()),
        Some(1)
    );

    let replay_task = plan
        .completed_tasks
        .get(&completed.task_id)
        .expect("resume plan should retain completed task metadata");
    assert_eq!(
        replay_task
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some("child-req-001")
    );
}

#[tokio::test]
async fn legacy_sessions_remain_loadable_after_native_metadata_extension() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_legacy_native_metadata";
    let run_dir = temp_dir.path().join(run_id);
    write_events(
        &run_dir,
        &[
            envelope(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "legacy prompt".to_string(),
                    request_digest: "digest-legacy".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                4,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool-args".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                5,
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                }),
            ),
            envelope(
                run_id,
                6,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                }),
            ),
            envelope(
                run_id,
                7,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "legacy done".to_string(),
                    result_digest: "digest-task".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                8,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("legacy ok".to_string()),
                    output_digest: Some("digest-tool-out".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                9,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "legacy segment done".to_string(),
                }),
            ),
        ],
    );

    let legacy_plan = inspect_resume_plan(&run_dir);
    assert!(
        legacy_plan.is_resumable,
        "legacy run should remain resumable"
    );
    assert_eq!(
        legacy_plan
            .tool_calls
            .get("toolcall_000001")
            .and_then(|snapshot| snapshot.metadata.as_ref()),
        None,
        "legacy records should deserialize without requiring new metadata fields"
    );
    assert_eq!(
        legacy_plan
            .completed_tasks
            .get("task_000001")
            .and_then(|snapshot| snapshot.metadata.as_ref()),
        None,
        "legacy task completion records should remain valid without metadata"
    );

    let coordinator = test_coordinator(temp_dir.path(), Arc::new(ToolRegistry::new()));
    let resumed = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("legacy run should still resume");
    coordinator.stop_run().await.expect("stop resumed run");

    let events = load_events(&resumed.events_path);
    assert!(
        events.len() > 9,
        "resume should append a new segment without rewriting old events"
    );
}

#[tokio::test]
async fn resume_projection_handles_checkpoint_between_turn_and_provider_restart() {
    REPLAY_GUARD_TOOL_CALLS.store(0, Ordering::SeqCst);

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_checkpoint_between_turn_and_provider_restart";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(run_dir.join("artifacts/compactions/agent_000001"))
        .expect("create checkpoint dirs");
    fs::write(
        run_dir.join("artifacts/compactions/agent_000001/checkpoint_000002.json"),
        serde_json::to_string(&ProviderContextCheckpoint {
            metadata: ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_000002".to_string(),
                agent_id: "agent_000001".to_string(),
                run_id: run_id.to_string(),
                through_seq: 3,
                through_request_id: Some("req_000001".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("model-1".to_string()),
                tokens_before: Some(4_000),
                tokens_before_estimate: None,
                tokens_after_estimate: None,
                summary_tokens_estimate: None,
                compacted_turns: None,
                preserved_turns: None,
                reduction_tokens_estimate: None,
                reduction_percent_estimate: None,
                trigger_reason: Some("manual".to_string()),
            },
            summary: "checkpointed turn summary".to_string(),
            recent_turns: vec![ProviderConversationTurn {
                user_prompt: "turn before checkpoint".to_string(),
                assistant_response: "assistant before checkpoint".to_string(),
                ..Default::default()
            }],
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts::default(),
            tail_boundary: None,
            summary_source: None,
            timeline_entry: None,
        })
        .expect("serialize checkpoint"),
    )
    .expect("write checkpoint");

    write_events(
        &run_dir,
        &[
            envelope(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                run_id,
                2,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "turn before checkpoint".to_string(),
                    request_digest: "digest-before".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "checkpointed segment ended".to_string(),
                }),
            ),
            envelope(
                run_id,
                4,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                run_id,
                5,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                run_id,
                6,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "turn after restart".to_string(),
                    request_digest: "digest-after".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                7,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "resumed segment finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    assert_eq!(plan.provider_model.as_deref(), Some("default/model-1"));
    assert!(plan.is_resumable);

    let coordinator = test_coordinator(temp_dir.path(), tool_registry_with_guard());
    let resumed = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run with checkpointed history");
    coordinator.stop_run().await.expect("stop resumed run");

    assert_eq!(REPLAY_GUARD_TOOL_CALLS.load(Ordering::SeqCst), 0);
    assert!(load_events(&resumed.events_path).len() > 6);
}

fn task_alias_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(DelegatingAliasTaskTool));
    Arc::new(registry)
}

fn tool_registry_with_guard() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReplayGuardTool));
    Arc::new(registry)
}

fn test_coordinator(session_dir: &Path, tool_registry: Arc<ToolRegistry>) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.deterministic_store = true;
    config.permission_policy = allow_all_permission_policy();
    config.tool_registry = tool_registry;
    config.agent_profiles = agent_profiles();

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn agent_profiles() -> BTreeMap<String, AgentProfile> {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "alpha".to_string(),
        AgentProfile {
            name: "alpha".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_ref_explicit: true,
            system_prompt: "alpha-prompt".to_string(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: vec![],
        },
    );
    profiles
}

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_events(run_dir: &Path, events: &[EventEnvelopeV1]) {
    fs::create_dir_all(run_dir.join("artifacts")).expect("create run directory");
    let mut body = String::new();
    for event in events {
        body.push_str(&serde_json::to_string(event).expect("serialize event"));
        body.push('\n');
    }
    fs::write(run_dir.join("events.jsonl"), body).expect("write events");
}
