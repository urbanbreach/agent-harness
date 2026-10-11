use super::*;
use crate::{
    clock::FakeClock,
    config::{
        SubagentCatalogAuthority, SubagentCatalogModel, SubagentDefinition,
        SubagentDefinitionSnapshot, SubagentModelCatalog,
    },
    redact::DefaultRedactor,
    subagent::*,
    tool::{Tool, ToolCapability, ToolContext, ToolError},
};
use harness_providers::{
    mock::MockProvider, CompletionRequest, CompletionUsage, Provider, ProviderEventStream,
    ProviderStreamEvent as Stream, ProviderStreamFinishedMetadata,
};
use serde_json::{json, Value};
use tokio_stream::StreamExt;

mod messages;
mod native_resume;
mod output_contract;
mod queue;
mod workspace;

struct Gate {
    requests: mpsc::UnboundedSender<CompletionRequest>,
    permits: tokio::sync::Semaphore,
    responses: MockProvider,
}
#[async_trait::async_trait]
impl Provider for Gate {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        if self.requests.send(request.clone()).is_err() {
            return Box::pin(tokio_stream::iter([Stream::error(
                "fixture observer closed",
            )]));
        }
        match self.permits.acquire().await {
            Ok(permit) => permit.forget(),
            Err(_) => return Box::pin(tokio_stream::iter([Stream::error("fixture gate closed")])),
        }
        self.responses.stream_completion(request).await
    }
}

struct NativeTool(&'static str);
#[async_trait::async_trait]
impl Tool for NativeTool {
    fn id(&self) -> &str {
        self.0
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn permission_requests(&self, _: &Value) -> Vec<(String, String)> {
        vec![(self.0.into(), "*".into())]
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let id = ctx.tool_call_id.to_string();
        let value = match self.0 {
            "spawn_subagent" => {
                return ctx
                    .coordinator
                    .spawn_subagent(
                        ctx.actor,
                        id,
                        serde_json::from_value(args)
                            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?,
                    )
                    .await
                    .map(Into::into)
                    .map_err(Into::into)
            }
            "get_command_or_subagent_output" => serde_json::to_value(
                ctx.coordinator
                    .get_command_or_subagent_output(
                        ctx.actor,
                        id,
                        serde_json::from_value(args)
                            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?,
                    )
                    .await?,
            ),
            "wait_commands_or_subagents" => serde_json::to_value(
                ctx.coordinator
                    .wait_commands_or_subagents(
                        ctx.actor,
                        id,
                        serde_json::from_value(args)
                            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?,
                    )
                    .await?,
            ),
            "kill_command_or_subagent" => serde_json::to_value(
                ctx.coordinator
                    .kill_command_or_subagent(
                        ctx.actor,
                        id,
                        serde_json::from_value(args)
                            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?,
                    )
                    .await?,
            ),
            "send_subagent_message" => serde_json::to_value(
                ctx.coordinator
                    .send_subagent_message(
                        ctx.actor,
                        id,
                        serde_json::from_value(args)
                            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?,
                    )
                    .await?,
            ),
            _ => return Err(ToolError::Execution("invalid fixture operation".into())),
        }
        .map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolResult::structured("native result", value))
    }
}

fn done(text: &str) -> Vec<Stream> {
    vec![
        Stream::TextDelta(text.into()),
        Stream::DoneWithMetadata {
            usage: Some(CompletionUsage {
                prompt_tokens: 11,
                completion_tokens: 3,
                total_tokens: 14,
            }),
            metadata: Some(ProviderStreamFinishedMetadata {
                usage_complete: Some(true),
                settled_reasoning: Some(vec!["settled fixture".into()]),
                ..Default::default()
            }),
        },
    ]
}

pub(super) fn configuration(temp: &Path, provider: Arc<dyn Provider>) -> CoordinatorConfig {
    let mut registry = ToolRegistry::new();
    for name in [
        "spawn_subagent",
        "get_command_or_subagent_output",
        "wait_commands_or_subagents",
        "kill_command_or_subagent",
        "send_subagent_message",
    ] {
        registry.register(Arc::new(NativeTool(name)));
    }
    let mut config = CoordinatorConfig::new(temp.join("sessions"));
    config.provider = provider;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider_model_concurrency = 1;
    config.tool_concurrency = 1;
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = config.tool_registry.tool_ids();
    config.agent_profiles.insert("default".into(), parent);
    config.subagents.messaging_enabled = true;
    config.subagent_model_catalog = Some(SubagentModelCatalog {
        authority: SubagentCatalogAuthority::Complete,
        models: vec![SubagentCatalogModel {
            id: "mock:default".into(),
            family: Some("test".into()),
            picker_eligible: true,
        }],
    });
    config.agent_model_targets.insert(
        "default".into(),
        ResolvedModelTarget {
            model_ref: "mock:default".into(),
            provider: "mock".into(),
            model: "default".into(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            limits: crate::config::ResolvedModelLimits::from_values(
                Some(32_768),
                Some(30_000),
                Some(2_000),
                crate::config::ModelLimitProvenance::explicit("native fixture"),
            ),
            resolution: Default::default(),
            catalog_entry: None,
        },
    );
    let mut definitions = SubagentDefinitionSnapshot::default();
    definitions.user.insert(
        "native-fixture".into(),
        SubagentDefinition {
            name: "native-fixture".into(),
            prompt_body: Some("current native definition".into()),
            ..Default::default()
        },
    );
    config.subagent_definitions = Some(definitions);
    config
}

async fn start(
    config: CoordinatorConfig,
    root: &Path,
) -> Result<(CoordinatorHandle, String), Box<dyn std::error::Error>> {
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle.start_run("native subagents", root).await?;
    let parent = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    Ok((handle, parent))
}

async fn event<T>(
    events: &mut crate::store::EventStream,
    select: impl Fn(&EventEnvelopeV1) -> Option<T>,
) -> Result<T, Box<dyn std::error::Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if let Some(value) = select(&event?) {
                return Ok(value);
            }
        }
        Err(EventStoreError::Invalid(
            "native fixture event channel closed",
        ))
    })
    .await?
    .map_err(Into::into)
}

fn spawn_args(background: bool) -> Value {
    json!({"prompt":"child task", "description":"native fixture", "subagent_type":"native-fixture", "background":background})
}

fn launch(
    handle: &CoordinatorHandle,
    agent: &str,
    name: &'static str,
    args: Value,
) -> tokio::task::JoinHandle<Result<ToolResult, String>> {
    let handle = handle.clone();
    let actor = EventActor::new(ActorKind::Worker, Some(agent.into()));
    tokio::spawn(async move {
        handle
            .execute_agent_tool_call(actor, None, name, args)
            .await
    })
}

async fn join(
    task: tokio::task::JoinHandle<Result<ToolResult, String>>,
) -> Result<ToolResult, Box<dyn std::error::Error>> {
    Ok(tokio::time::timeout(std::time::Duration::from_secs(5), task).await???)
}
