use harness_tools::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{ShellAllowlist, ToolFailureMode};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::redact::DefaultRedactor;
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderStreamEvent,
};
use harness_tools::coordinator_registry;
use tokio_stream as stream;

use crate::common::{allow_all_permission_policy, anonymous_supervisor_actor, worker_actor};

#[derive(Debug)]
struct LargeSummaryProvider;

#[async_trait]
impl Provider for LargeSummaryProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(stream::iter([
            ProviderStreamEvent::TextDelta(format!(
                "child-large-summary:{}:child-summary-tail",
                "x".repeat(1_600)
            )),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                }),
            },
        ]))
    }
}

pub(crate) fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
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

pub(crate) fn run_started(run_id: &str, workspace: &Path) -> EventEnvelopeV1 {
    envelope(
        run_id,
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: run_id.to_string().into(),
            workspace_root: workspace.display().to_string(),
        }),
    )
}

pub(crate) fn write_session_events(workspace: &Path, run_id: &str, events: &[EventEnvelopeV1]) {
    let run_dir = workspace.join(".agent-harness/sessions").join(run_id);
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

pub(crate) fn read_artifact(workspace: &Path, artifact_path: &str) -> String {
    std::fs::read_to_string(workspace.join(artifact_path)).unwrap_or_abort()
}

pub(crate) fn task_profiles() -> BTreeMap<String, AgentProfile> {
    BTreeMap::from([
        ("parent".to_string(), profile("parent", &["task"])),
        ("general".to_string(), profile("general", &[])),
    ])
}

pub(crate) fn profile(name: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        model_ref: "mock:large-summary".to_string(),
        model_ref_explicit: true,
        system_prompt: format!("{name} prompt"),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
        permission_ruleset: Vec::new(),
    }
}

pub(crate) async fn spawn_task_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("sessions");
    std::fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = allow_all_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = task_profiles();
    config.provider = Arc::new(LargeSummaryProvider);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_large_output_child_task", workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent(anonymous_supervisor_actor(), "parent", None)
        .await
        .unwrap_or_abort();

    (handle, run, worker_id)
}

pub(crate) fn task_tool_actor(worker_id: &str) -> EventActor {
    worker_actor(worker_id)
}
