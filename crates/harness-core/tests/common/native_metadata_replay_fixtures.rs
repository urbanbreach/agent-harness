use harness_core::UnwrapOrAbort;
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
use harness_core::ToolResultExt;
use serde::Deserialize;
use serde_json::{json, Value};

#[path = "mod.rs"]
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
            .tool_err("failed to open artifact store")?;
        let artifact = store
            .write_text(
                "delegated/task-output.json",
                &format!(
                    "{{\"child_session_id\":\"{}\",\"child_request_id\":\"{}\"}}",
                    args.child_session_id, args.child_request_id
                ),
            )
            .tool_err("failed to write artifact")?;

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
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: vec![],
            permission_ruleset: Vec::new(),
        },
    );
    profiles
}

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
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
    fs::create_dir_all(run_dir.join("artifacts")).unwrap_or_abort();
    let mut body = String::new();
    for event in events {
        body.push_str(&serde_json::to_string(event).unwrap_or_abort());
        body.push('\n');
    }
    fs::write(run_dir.join("events.jsonl"), body).unwrap_or_abort();
}
