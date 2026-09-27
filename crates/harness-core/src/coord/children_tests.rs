use super::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError},
};
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_stream::StreamExt;

struct Delegate;
#[async_trait::async_trait]
impl Tool for Delegate {
    fn id(&self) -> &str {
        "delegate"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        if let Some(id) = args["inspect"].as_str() {
            return ctx
                .coordinator
                .child_task_output(ctx.tool_call_id.to_string(), id)
                .await
                .map_err(|e| ToolError::Execution(e.to_string()));
        }
        ctx.coordinator
            .delegate_task(
                ctx.tool_call_id.to_string(),
                ChildTaskRequest {
                    session_id: None,
                    profile: "child".into(),
                    prompt: "inspect".into(),
                    description: "Inspection".into(),
                    run_in_background: args["background"].as_bool().unwrap_or(false),
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))
    }
}
struct Gate {
    permits: tokio::sync::Semaphore,
    entered: tokio::sync::Notify,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl Provider for Gate {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        assert_eq!(
            request.context.initiator,
            harness_providers::ProviderRequestInitiator::Agent
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        match self.permits.acquire().await {
            Ok(permit) => permit.forget(),
            Err(_) => {
                return Box::pin(tokio_stream::iter([ProviderStreamEvent::error(
                    "gate closed",
                )]))
            }
        }
        Box::pin(tokio_stream::iter([
            ProviderStreamEvent::TextDelta("child report".into()),
            ProviderStreamEvent::Done { usage: None },
        ]))
    }
}
async fn child_started(
    events: &mut crate::store::EventStream,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while let Some(event) = events.next().await {
                if let EventV1::TaskScheduled(e) = event?.payload {
                    if e.state == TaskScheduleState::Started
                        && e.metadata
                            .as_ref()
                            .and_then(|m| m.lineage.as_ref())
                            .and_then(|l| l.child_request_id.as_ref())
                            .is_some()
                    {
                        return Ok(e.task_id.to_string());
                    }
                }
            }
            Err(EventStoreError::Invalid("child did not start"))
        })
        .await??,
    )
}

#[tokio::test]
async fn child_demotion_releases_waiters_but_retains_capacity_and_cancellation(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(Gate {
        permits: tokio::sync::Semaphore::new(0),
        entered: tokio::sync::Notify::new(),
        calls: AtomicUsize::new(0),
    });
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Delegate));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.provider = Arc::clone(&provider) as Arc<dyn Provider>;
    config.provider_model_concurrency = 1;
    config.tool_concurrency = 1;
    config.command_buffer = 2;
    config.permission_policy = PermissionPolicy::allow_all();
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = vec!["delegate".into()];
    config.agent_profiles.insert("default".into(), parent);
    config
        .agent_profiles
        .insert("child".into(), AgentProfile::fallback("child"));
    let handle = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("children", temp.path()).await?;
    let agent = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut events = handle.event_store().await?.subscribe(1)?;
    let launch = || {
        let (handle, actor) = (
            handle.clone(),
            EventActor::new(ActorKind::Worker, Some(agent.clone())),
        );
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(actor, None, "delegate", json!({}))
                .await
        })
    };
    let first = launch();
    let first_id = child_started(&mut events).await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        provider.entered.notified(),
    )
    .await?;
    let second = launch();
    let second_id = child_started(&mut events).await?;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), launch())
            .await??
            .is_err()
    );
    let demoted = handle.demote_all_foreground_child_tasks().await?;
    assert_eq!(demoted.len(), 2);
    assert!(demoted
        .iter()
        .all(crate::foreground_demote::DemoteToBackgroundResult::is_demoted));
    for task in [first, second] {
        let output = tokio::time::timeout(std::time::Duration::from_secs(3), task).await???;
        assert_eq!(
            output
                .structured_json
                .as_ref()
                .and_then(|j| j["status"].as_str()),
            Some("running")
        );
    }
    assert!(handle
        .demote_foreground_child_task(first_id.clone())
        .await?
        .is_rejected());
    provider.permits.add_permits(2);
    super::history_tests::settled(&handle, &first_id).await?;
    super::history_tests::settled(&handle, &second_id).await?;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    let third = launch();
    let third_id = child_started(&mut events).await?;
    handle.stop_run().await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(3), third)
            .await??
            .is_err()
    );
    let journal = crate::store::read_events(&run.events_path)?;
    assert!(journal.iter().any(
        |e| matches!(&e.payload, EventV1::TaskCancelled(c) if c.task_id.as_str() == third_id)
    ));
    assert_eq!(
        journal
            .iter()
            .filter(|e| matches!(e.payload, EventV1::BackgroundTaskNotification(_)))
            .count(),
        2
    );
    // Reopen a crash prefix: demotion was committed, but its tool result was not.
    let cutoff = journal
        .iter()
        .rposition(|e| {
            matches!(&e.payload,
                EventV1::UiIntentReceived(e) if e.intent == "background_foreground_child"
            )
        })
        .ok_or("missing demotion")?;
    let EventV1::UiIntentReceived(intent) = &journal[cutoff].payload else {
        return Err("missing intent".into());
    };
    let child = intent.params.get("session_id").ok_or("missing child")?;
    let mut prefix = Vec::new();
    for event in &journal[..=cutoff] {
        serde_json::to_writer(&mut prefix, event)?;
        prefix.push(b'\n');
    }
    std::fs::write(&run.events_path, prefix)?;
    for event in &journal {
        if let EventV1::AgentSpawned(e) = &event.payload {
            if e.parent_agent_id.is_some() {
                std::fs::remove_dir_all(config.session_dir.join(&e.agent_id))?;
            }
        }
    }
    let calls = provider.calls.load(Ordering::SeqCst);
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "after demotion crash")
        .await?;
    let output = resumed
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(agent)),
            None,
            "delegate",
            json!({"inspect":child}),
        )
        .await?;
    assert_eq!(
        output.structured_json.ok_or("missing report")?["run_in_background"],
        true
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), calls);
    resumed.stop_run().await?;
    Ok(())
}
