use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_stream::StreamExt;

struct WaitingTool(Arc<Notify>);
#[async_trait::async_trait]
impl crate::tool::Tool for WaitingTool {
    fn id(&self) -> &str {
        "waiting"
    }
    fn capability(&self) -> crate::tool::ToolCapability {
        crate::tool::ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    async fn call(
        &self,
        context: crate::tool::ToolContext,
        _: serde_json::Value,
    ) -> Result<ToolResult, crate::tool::ToolError> {
        self.0.notify_one();
        context.cancellation.cancelled().await;
        Ok(ToolResult::text("cancelled"))
    }
}

#[tokio::test]
async fn tool_admission_bounds_queued_workers_and_releases_capacity_after_cancellation(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let entered = Arc::new(Notify::new());
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(WaitingTool(Arc::clone(&entered))));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.command_buffer = 2;
    config.tool_concurrency = 1;
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = Arc::new(registry);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("tool admission", root.path()).await?;
    let actor = EventActor::new(ActorKind::User, None);
    let first = handle
        .request_tool_call(actor.clone(), None, "waiting", serde_json::json!({}))
        .await?;
    tokio::time::timeout(Duration::from_secs(2), entered.notified()).await?;
    for _ in 0..2 {
        handle
            .request_tool_call(actor.clone(), None, "waiting", serde_json::json!({}))
            .await?;
    }
    assert!(
        handle
            .request_tool_call(actor.clone(), None, "waiting", serde_json::json!({}))
            .await
            .is_err(),
        "queued workers must be bounded, not just active tool execution"
    );
    let mut events = handle.subscribe_new_events().await?;
    handle.cancel_task(first.clone(), "free capacity").await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCancelled(e) if e.task_id.as_str() == first) {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("missing cancelled terminal"))
    })
    .await??;
    handle
        .request_tool_call(actor, None, "waiting", serde_json::json!({}))
        .await?;
    tokio::time::timeout(Duration::from_secs(2), handle.stop_run()).await??;
    let events = crate::store::read_events(&run.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::ToolCallRequested(_)))
            .count(),
        4
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::ToolCallFinished(_)))
            .count(),
        4
    );
    Ok(())
}

#[derive(Default)]
struct UnwindingProvider {
    entered: Notify,
    release: Notify,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl Provider for UnwindingProvider {
    async fn stream_completion(&self, _: CompletionRequest) -> ProviderEventStream {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.notify_one();
            self.release.notified().await;
            std::panic::resume_unwind(Box::new("sensitive panic payload"));
        }
        Box::pin(tokio_stream::iter([ProviderStreamEvent::Done {
            usage: None,
        }]))
    }
}

#[tokio::test]
async fn lost_worker_state_stops_the_run_before_queued_work_can_continue(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(UnwindingProvider::default());
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn Provider>;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("worker failure", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "first",
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(3), provider.entered.notified()).await?;
    let queued = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "queued")
        .await?;
    provider.release.notify_one();
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut cancelled = false;
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(e) => assert_ne!(e.task_id.as_str(), queued),
                EventV1::TaskCancelled(e) if e.task_id.as_str() == queued => cancelled = true,
                EventV1::RunFailed(_) => {
                    assert!(cancelled);
                    return Ok::<_, EventStoreError>(());
                }
                _ => {}
            }
        }
        Err(EventStoreError::Invalid("missing failure event"))
    })
    .await??;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(coordinator.run_info().await.is_err());
    assert!(!std::fs::read_to_string(&run.events_path)?.contains("sensitive panic payload"));
    let prefix = std::fs::read(&run.events_path)?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "recovered")
        .await?;
    resumed.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::ProviderRequestFinished(_)))
            .count(),
        1
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(std::fs::read(&run.events_path)?.starts_with(&prefix));
    crate::session_lineage::latest_clone_stable_prefix(&events)?;
    Ok(())
}

#[tokio::test]
async fn reasoning_fragments_share_the_provider_response_limit(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.provider = Arc::new(harness_providers::mock::MockProvider::script([vec![
        ProviderStreamEvent::ReasoningDelta("a".repeat(2 * 1024 * 1024)),
        ProviderStreamEvent::ReasoningDelta("b".repeat(2 * 1024 * 1024 + 1)),
        ProviderStreamEvent::Done { usage: None },
    ]]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("reasoning limit", root.path())
        .await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let task = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "think")
        .await?;
    assert!(super::history_tests::settled(&coordinator, &task)
        .await
        .is_err());
    coordinator.stop_run().await?;
    let journal = std::fs::read_to_string(run.events_path)?;
    assert!(journal.contains("provider response exceeds the runtime limit"));
    assert!(!journal.contains("aaaaaaaa"));
    Ok(())
}
