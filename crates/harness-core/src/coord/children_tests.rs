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
mod workspace;

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
    definitions.cli.insert(
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

#[tokio::test]
async fn native_registration_queue_demotion_and_foreground_cancel_keep_distinct_scopes(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (requests, mut received) = mpsc::unbounded_channel();
    let provider = Arc::new(Gate {
        requests,
        permits: tokio::sync::Semaphore::new(0),
        responses: MockProvider::script([done("first"), done("second"), done("cancelled")]),
    });
    let mut config = configuration(temp.path(), Arc::<Gate>::clone(&provider));
    config.subagents.max_concurrent = 1;
    let (handle, parent) = start(config, temp.path()).await?;
    let mut events = handle.subscribe_new_events().await?;
    let first = launch(&handle, &parent, "spawn_subagent", spawn_args(false));
    let first_id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let first_request = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("first provider request absent")?;
    assert_eq!(
        first_request.context.session_id.as_deref(),
        Some(first_id.as_str())
    );
    let second = launch(&handle, &parent, "spawn_subagent", spawn_args(false));
    let second_id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let queued = join(launch(
        &handle,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[second_id]}),
    ))
    .await?;
    assert_eq!(
        queued
            .structured_json
            .as_ref()
            .and_then(|v| v["Result"]["status"].as_str()),
        Some("initializing")
    );
    assert_eq!(provider.responses.call_count(), 0);
    let demoted = handle.demote_all_foreground_child_tasks().await?;
    assert_eq!(demoted.len(), 2);
    assert!(demoted
        .iter()
        .all(crate::foreground_demote::DemoteToBackgroundResult::is_demoted));
    let first_handle = join(first).await?;
    let second_handle = join(second).await?;
    assert!(first_handle.display_text.contains(&first_id));
    assert!(second_handle.display_text.contains(&second_id));
    let mut terminal = handle.subscribe_new_events().await?;
    provider.permits.add_permits(1);
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r)
            if r.child_id == first_id && r.kind == "terminal_published" =>
        {
            Some(())
        }
        _ => None,
    })
    .await?;
    let second_request = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("second provider request absent")?;
    assert_eq!(
        second_request.context.session_id.as_deref(),
        Some(second_id.as_str())
    );
    let mut terminal = handle.subscribe_new_events().await?;
    provider.permits.add_permits(1);
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r)
            if r.child_id == second_id && r.kind == "terminal_published" =>
        {
            Some(())
        }
        _ => None,
    })
    .await?;
    let mut events = handle.subscribe_new_events().await?;
    let third = launch(&handle, &parent, "spawn_subagent", spawn_args(false));
    let tool = event(&mut events, |event| match &event.payload {
        EventV1::ToolCallRequested(r) if r.tool_id == "spawn_subagent" => {
            Some(r.tool_call_id.to_string())
        }
        _ => None,
    })
    .await?;
    let third_id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv()).await?;
    let mut terminal = handle.subscribe_new_events().await?;
    handle.cancel_task(&tool, "foreground cancelled").await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), third)
            .await??
            .is_err()
    );
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r)
            if r.child_id == third_id && r.kind == "terminal_published" =>
        {
            Some(())
        }
        _ => None,
    })
    .await?;
    let cancelled = join(launch(
        &handle,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[third_id]}),
    ))
    .await?;
    assert_eq!(
        cancelled
            .structured_json
            .as_ref()
            .and_then(|v| v["Result"]["status"].as_str()),
        Some("cancelled")
    );
    handle.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn native_background_waiter_cancel_does_not_cancel_work_or_replay_it(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (requests, mut received) = mpsc::unbounded_channel();
    let provider = Arc::new(Gate {
        requests,
        permits: tokio::sync::Semaphore::new(0),
        responses: MockProvider::script([done("survived")]),
    });
    let config = configuration(temp.path(), Arc::<Gate>::clone(&provider));
    let (handle, parent) = start(config.clone(), temp.path()).await?;
    let run = handle.run_info().await?;
    let mut events = handle.subscribe_new_events().await?;
    let background = launch(&handle, &parent, "spawn_subagent", spawn_args(true));
    let id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let _ = join(background).await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv()).await?;
    let mut events = handle.subscribe_new_events().await?;
    let waiter = launch(
        &handle,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[id],"timeout_ms":60_000}),
    );
    let tool = event(&mut events, |event| match &event.payload {
        EventV1::ToolCallRequested(r) if r.tool_id == "get_command_or_subagent_output" => {
            Some(r.tool_call_id.to_string())
        }
        _ => None,
    })
    .await?;
    handle
        .request_subagent_cancel(
            EventActor::new(ActorKind::System, None),
            SubagentCommandRequest::WaiterCancel { waiter_id: tool },
        )
        .await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await??
            .is_err()
    );
    assert!(matches!(
        handle.raw_finalized_state(id.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let mut terminal = handle.subscribe_new_events().await?;
    provider.permits.add_permits(1);
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r) if r.child_id == id && r.kind == "terminal_published" => {
            Some(())
        }
        _ => None,
    })
    .await?;
    handle.stop_run().await?;
    let bytes = std::fs::read(&run.events_path)?;
    let calls = provider.responses.call_count();
    let restored = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    restored
        .resume_run(run.run_id.to_string(), "native reload")
        .await?;
    let output = join(launch(
        &restored,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[id]}),
    ))
    .await?;
    assert_eq!(
        output
            .structured_json
            .as_ref()
            .and_then(|v| v["Result"]["status"].as_str()),
        Some("completed")
    );
    assert_eq!(provider.responses.call_count(), calls);
    assert!(matches!(
        restored.raw_finalized_state(id).await?,
        FinalizedStateResult::Available { .. }
    ));
    restored.stop_run().await?;
    assert!(std::fs::read(&run.events_path)?.starts_with(&bytes));
    Ok(())
}
