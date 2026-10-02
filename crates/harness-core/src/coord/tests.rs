use super::*;
use crate::{
    clock::FakeClock,
    event::*,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_stream::StreamExt;

pub(super) struct CountTool(pub(super) Arc<AtomicUsize>);
#[async_trait::async_trait]
impl Tool for CountTool {
    fn id(&self) -> &'static str {
        "count"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn filesystem_paths(&self, args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        Ok(args
            .get("path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .into_iter()
            .collect())
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        if let Some(path) = args.get("path").and_then(Value::as_str) {
            ctx.resolve_workspace_path(std::path::Path::new(path))?;
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::text("tool result"))
    }
}

struct PendingProvider {
    entered: Arc<tokio::sync::Notify>,
    dropped: Arc<AtomicUsize>,
}
struct PendingStream(Arc<AtomicUsize>);
impl tokio_stream::Stream for PendingStream {
    type Item = Stream;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Stream>> {
        std::task::Poll::Pending
    }
}
impl Drop for PendingStream {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait::async_trait]
impl harness_providers::Provider for PendingProvider {
    async fn stream_completion(
        &self,
        _: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        self.entered.notify_one();
        Box::pin(PendingStream(Arc::clone(&self.dropped)))
    }
}
#[tokio::test]
async fn cancellation_removes_queued_turns_and_drops_the_active_provider_stream(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(PendingProvider {
        entered: Arc::clone(&entered),
        dropped: Arc::clone(&dropped),
    });
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("cancel", temp.path()).await?;
    let store = coordinator.event_store().await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let first = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "first",
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified()).await?;
    let second = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "second",
        )
        .await?;
    let third = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "third")
        .await?;
    coordinator
        .cancel_task(&second, "queued cancellation")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run()).await??;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    let events = store
        .replay(1)?
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let cancelled: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::TaskCancelled(e) => Some(e.task_id.as_str()),
            _ => None,
        })
        .collect();
    for id in [&first, &second, &third] {
        assert!(
            cancelled.contains(&id.as_str()),
            "missing cancellation for {id}"
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::ProviderRequestStarted(_)))
            .count(),
        1
    );
    assert!(matches!(
        events.last().map(|e| &e.payload),
        Some(EventV1::RunFinished(_))
    ));
    Ok(())
}

type AppendHook =
    dyn Fn(&crate::store::EventEnvelopeWithoutSeqV1) -> Result<(), EventStoreError> + Send + Sync;
pub(super) struct InterceptStore {
    pub inner: Arc<dyn EventStore>,
    pub before_append: Box<AppendHook>,
}
impl EventStore for InterceptStore {
    fn append(
        &self,
        event: crate::store::EventEnvelopeWithoutSeqV1,
    ) -> Result<EventEnvelopeV1, EventStoreError> {
        (self.before_append)(&event)?;
        self.inner.append(event)
    }
    fn append_applied(
        &self,
        event: crate::store::EventEnvelopeWithoutSeqV1,
        apply: &mut dyn FnMut(&EventEnvelopeV1),
    ) -> Result<EventEnvelopeV1, EventStoreError> {
        (self.before_append)(&event)?;
        self.inner.append_applied(event, apply)
    }
    fn replay(&self, seq: u64) -> Result<crate::store::EventStream, EventStoreError> {
        self.inner.replay(seq)
    }
    fn subscribe(&self, seq: u64) -> Result<crate::store::EventStream, EventStoreError> {
        self.inner.subscribe(seq)
    }
    fn subscribe_runtime(
        &self,
        seq: u64,
    ) -> Result<crate::store::RuntimeEventStream, EventStoreError> {
        self.inner.subscribe_runtime(seq)
    }
    fn publish_live(&self, event: LiveEventEnvelope) {
        self.inner.publish_live(event);
    }
    fn close_writer(&self) -> Result<(), EventStoreError> {
        self.inner.close_writer()
    }
}
#[tokio::test]
async fn journal_failure_stops_continuation_and_rejects_further_work(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CountTool(Arc::new(AtomicUsize::new(0)))));
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "call".into(),
                function_name: "count".into(),
                arguments_json: "{}".into(),
            },
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("must not continue".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["count".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("failure", temp.path()).await?;
    let original = coordinator.event_store().await?;
    // Inject a storage failure at the existing store seam; assertions use public behavior.
    coordinator
        .call(move |s| {
            let storage = s.store.clone().ok_or(CoordinatorError::RunNotStarted)?;
            s.store = Some(Arc::new(InterceptStore {
                inner: storage,
                before_append: Box::new(|event| {
                    if matches!(event.payload, EventV1::ToolCallFinished(_)) {
                        Err(EventStoreError::Io(std::io::Error::other(
                            "simulated disk failure",
                        )))
                    } else {
                        Ok(())
                    }
                }),
            }));
            Ok(())
        })
        .await?;
    let mut events = original.subscribe(1)?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let task = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "check",
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            let event = event?;
            if matches!(event.payload, EventV1::TaskCompleted(ref e) if e.task_id.as_str() == task)
                || matches!(event.payload, EventV1::TaskCancelled(ref e) if e.task_id.as_str() == task) { return Ok::<_, EventStoreError>(()); }
        }
        Err(EventStoreError::Invalid("missing terminal event"))
    }).await??;
    assert_eq!(provider.call_count(), 1);
    assert!(coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "again")
        .await
        .is_err());
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run())
            .await?
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn context_preflight_blocks_an_oversized_request_before_calling_the_provider(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let settings = crate::config::load_config_from_str(
        r#"{
        model: 'local/small', provider: { local: { type: 'openai_compatible', baseUrl: 'https://example.invalid',
          models: { small: { limit: { context: 256, output: 64 } } } } }
    }"#,
    )?;
    let target = crate::config::resolve_model_selection(&settings, "local/small", None)?.primary;
    let provider = Arc::new(MockProvider::default());
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.enabled = false;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("budget", temp.path()).await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let task = coordinator
        .request_agent_turn_with_model_target(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "too long ".repeat(300),
            target,
        )
        .await?;
    let failed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(event) if event.task_id.as_str() == task => {
                    return Ok::<_, EventStoreError>(false)
                }
                EventV1::TaskCancelled(event) if event.task_id.as_str() == task => {
                    return Ok(event.failure)
                }
                _ => {}
            }
        }
        Err(EventStoreError::Invalid("missing task result"))
    })
    .await??;
    assert!(failed);
    assert_eq!(provider.call_count(), 0);
    let next = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent.clone(), "ok")
        .await?;
    super::history_tests::settled(&coordinator, &next).await?;
    assert_eq!(provider.call_count(), 1);
    assert!(!provider.captured_requests().await[0]
        .messages
        .iter()
        .any(|m| m.content.contains("too long")));
    coordinator.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "budget resumed")
        .await?;
    let next = resumed
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "continue")
        .await?;
    super::history_tests::settled(&resumed, &next).await?;
    assert!(!provider
        .captured_requests()
        .await
        .last()
        .ok_or("missing resumed request")?
        .messages
        .iter()
        .any(|m| m.content.contains("too long")));
    resumed.stop_run().await?;
    Ok(())
}
