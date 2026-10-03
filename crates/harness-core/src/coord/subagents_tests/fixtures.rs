use super::*;

pub(super) fn settled_metadata(reasoning: &str) -> Stream {
    Stream::DoneWithMetadata {
        usage: Some(CompletionUsage {
            prompt_tokens: 12,
            completion_tokens: 3,
            total_tokens: 15,
        }),
        metadata: Some(ProviderStreamFinishedMetadata {
            settled_reasoning: Some(vec![reasoning.into()]),
            usage_complete: Some(true),
            ..Default::default()
        }),
    }
}

pub(super) struct LargeTool(pub(super) Arc<AtomicUsize>);
#[async_trait::async_trait]
impl Tool for LargeTool {
    fn id(&self) -> &str {
        "large"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn call(&self, ctx: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let path = ctx.resolve_workspace_path(std::path::Path::new("sample.txt"))?;
        ctx.tool_state.record_read(
            &path,
            blake3::hash(&std::fs::read(&path)?).to_hex().to_string(),
        )?;
        Ok(ToolResult::structured(
            "raw-output-marker\n".repeat(4000),
            json!({"session_id":ctx.actor.agent_id}),
        ))
    }
}

pub(super) struct HeldProvider;
#[async_trait::async_trait]
impl harness_providers::Provider for HeldProvider {
    async fn stream_completion(
        &self,
        _: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        Box::pin(tokio_stream::pending())
    }
}

pub(super) fn system_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, None)
}

pub(super) async fn wait_terminal(
    events: &mut crate::store::EventStream,
    attempt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(e) if e.task_id.as_str() == attempt => return Ok(()),
                EventV1::TaskCancelled(e) if e.task_id.as_str() == attempt => {
                    return Err(EventStoreError::Invalid("turn failed"))
                }
                _ => {}
            }
        }
        Err(EventStoreError::Invalid("missing terminal"))
    })
    .await??;
    Ok(())
}

pub(super) async fn wait_cancelled(
    events: &mut crate::store::EventStream,
    attempt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCancelled(e) if e.task_id.as_str() == attempt)
            {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("cancelled terminal missing"))
    })
    .await??;
    Ok(())
}

pub(super) async fn assert_prompt_turns(
    coordinator: &CoordinatorHandle,
    agent: &str,
    turns: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        coordinator.subagent_history().await?.records[agent]
            .accounting
            .as_ref()
            .map(|accounting| accounting.turns),
        Some(turns)
    );
    Ok(())
}
