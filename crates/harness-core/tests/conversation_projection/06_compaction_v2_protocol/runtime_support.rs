use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{PermissionMode, ToolFailureMode};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{ActorKind, EventActor};
use harness_core::file_tag::SelectedPromptTags;
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolRegistry, ToolResult};
use harness_providers::{
    CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent,
};
use tokio_stream::StreamExt;

#[derive(Clone)]
pub(super) struct CapturingScriptedProvider {
    requests: Arc<Mutex<Vec<CompletionRequest>>>,
    scripts: Arc<[Vec<ProviderStreamEvent>]>,
    next_script: Arc<Mutex<usize>>,
}

impl CapturingScriptedProvider {
    pub(super) fn new(scripts: Vec<Vec<ProviderStreamEvent>>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            scripts: Arc::from(scripts),
            next_script: Arc::new(Mutex::new(0)),
        }
    }

    pub(super) fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }
}

#[async_trait]
impl Provider for CapturingScriptedProvider {
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

    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.requests.lock().unwrap_or_abort().push(request);
        let mut next_script = self.next_script.lock().unwrap_or_abort();
        let script = self
            .scripts
            .get(*next_script)
            .cloned()
            .unwrap_or_else(|| vec![ProviderStreamEvent::error("missing scripted response")]);
        *next_script = next_script.saturating_add(1);
        Box::pin(tokio_stream::iter(script))
    }
}

pub(super) struct RuntimeHarness {
    pub(super) temp_dir: tempfile::TempDir,
    pub(super) coordinator: CoordinatorHandle,
    pub(super) run: RunInfo,
    pub(super) agent_id: String,
}

impl RuntimeHarness {
    pub(super) async fn start(provider: Arc<dyn Provider>) -> Self {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = coordinator(temp_dir.path(), provider);
        let run = coordinator
            .start_run("compaction-v2-projection", temp_dir.path().to_path_buf())
            .await
            .unwrap_or_abort();
        let agent_id = coordinator
            .spawn_agent_idle(supervisor_actor(), "default", None)
            .await
            .unwrap_or_abort();
        Self {
            temp_dir,
            coordinator,
            run,
            agent_id,
        }
    }

    pub(super) async fn resume(&self, provider: Arc<dyn Provider>) -> CoordinatorHandle {
        let resumed = coordinator(self.temp_dir.path(), provider);
        resumed
            .resume_run(self.run.run_id.as_str(), "compaction-v2-projection")
            .await
            .unwrap_or_abort();
        resumed
    }

    pub(super) async fn turn(
        &self,
        prompt: &str,
        attachments: Vec<AttachmentMetadata>,
    ) -> String {
        run_turn(&self.coordinator, &self.agent_id, prompt, attachments).await
    }
}

pub(super) async fn run_turn(
    coordinator: &CoordinatorHandle,
    agent_id: &str,
    prompt: &str,
    attachments: Vec<AttachmentMetadata>,
) -> String {
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            supervisor_actor(),
            agent_id,
            prompt,
            SelectedPromptTags::default(),
            attachments,
            None,
            None,
        )
        .await
        .unwrap_or_abort();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.next().await.unwrap_or_abort().unwrap_or_abort();
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    event.payload,
                    EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap_or_abort();
    request_id
}

pub(super) fn text_response(text: &str) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta(text.to_string()),
        ProviderStreamEvent::Done { usage: None },
    ]
}

pub(super) fn tool_response() -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "kept-tool-call".to_string(),
            function_name: "projection_probe".to_string(),
            arguments_json: r#"{"value":"工具 😀"}"#.to_string(),
        },
        ProviderStreamEvent::Done { usage: None },
    ]
}

pub(super) fn load_events(path: &Path) -> Vec<EventEnvelopeV1> {
    std::fs::read_to_string(path)
        .unwrap_or_abort()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_abort())
        .collect()
}

fn coordinator(session_dir: &Path, provider: Arc<dyn Provider>) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.provider = provider;
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Deny,
    );
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(ProjectionProbe));
    config.tool_registry = Arc::new(tools);
    config.agent_profiles = BTreeMap::from([(
        "default".to_string(),
        AgentProfile {
            name: "default".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_ref_explicit: true,
            system_prompt: "projection-system".to_string(),
            temperature: None,
            cache_retention: Default::default(),
            max_iters: Some(4),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: vec!["projection.probe".to_string()],
            permission_ruleset: Vec::new(),
        },
    )]);
    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("projection-supervisor".to_string()))
}

struct ProjectionProbe;

#[async_trait]
impl Tool for ProjectionProbe {
    harness_core::tool_metadata!(
        "projection.probe",
        "Return a deterministic projection sentinel",
        ToolCapability::ReadFs,
        serde_json::json!({"type": "object"})
    );

    async fn call(
        &self,
        _context: ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolResult, harness_core::tool::ToolError> {
        Ok(ToolResult::text("工具結果 😀"))
    }
}
