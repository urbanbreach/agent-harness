use super::*;
use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget};

pub(crate) struct CompactionV2Harness {
    pub(crate) _temp_dir: tempfile::TempDir,
    pub(crate) coordinator: CoordinatorHandle,
    pub(crate) run: RunInfo,
    pub(crate) agent_id: String,
}

impl CompactionV2Harness {
    pub(crate) async fn with_provider(
        provider: Arc<dyn Provider>,
        compaction: CompactionRuntimeConfig,
    ) -> Self {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_agent_coordinator_with_provider_and_compaction(
            temp_dir.path(),
            provider,
            2,
            compaction,
        );
        let run = coordinator
            .start_run(
                "compaction-v2-red",
                PathBuf::from("/workspace/compaction-v2"),
            )
            .await
            .unwrap_or_abort();
        let agent_id = coordinator
            .spawn_agent_idle(supervisor_actor(), "alpha", None)
            .await
            .unwrap_or_abort();
        Self {
            _temp_dir: temp_dir,
            coordinator,
            run,
            agent_id,
        }
    }

    pub(crate) async fn scripted(
        events: Vec<Vec<ProviderStreamEvent>>,
        compaction: CompactionRuntimeConfig,
    ) -> (Self, SequentialScriptedProvider) {
        let provider = SequentialScriptedProvider::new(events);
        let harness = Self::with_provider(Arc::new(provider.clone()), compaction).await;
        (harness, provider)
    }

    pub(crate) async fn scripted_with_tool(
        events: Vec<Vec<ProviderStreamEvent>>,
        compaction: CompactionRuntimeConfig,
        tool_output: String,
    ) -> (Self, SequentialScriptedProvider) {
        Self::scripted_with_named_tool(events, compaction, "atomic_tool", tool_output).await
    }

    pub(crate) async fn scripted_with_named_tool(
        events: Vec<Vec<ProviderStreamEvent>>,
        compaction: CompactionRuntimeConfig,
        tool_id: &str,
        tool_output: String,
    ) -> (Self, SequentialScriptedProvider) {
        let provider = SequentialScriptedProvider::new(events);
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(StaticTextTool {
            id: tool_id.to_string(),
            output: tool_output,
        }));
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_agent_tool_coordinator_with_compaction(
            temp_dir.path(),
            Arc::new(provider.clone()),
            Arc::new(registry),
            allow_all_permission_policy(),
            vec![tool_id.to_string()],
            4,
            compaction,
        );
        let run = coordinator
            .start_run(
                "compaction-v2-red",
                PathBuf::from("/workspace/compaction-v2"),
            )
            .await
            .unwrap_or_abort();
        let agent_id = coordinator
            .spawn_agent_idle(supervisor_actor(), "alpha", None)
            .await
            .unwrap_or_abort();
        (
            Self {
                _temp_dir: temp_dir,
                coordinator,
                run,
                agent_id,
            },
            provider,
        )
    }

    pub(crate) async fn turn(&self, prompt: &str) -> String {
        let store = self.coordinator.event_store().await.unwrap_or_abort();
        let mut events = store.subscribe(1).unwrap_or_abort();
        let request_id = self
            .coordinator
            .request_agent_turn(supervisor_actor(), self.agent_id.clone(), prompt)
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

    pub(crate) async fn turn_with_target(
        &self,
        prompt: &str,
        target: ResolvedModelTarget,
    ) -> String {
        let store = self.coordinator.event_store().await.unwrap_or_abort();
        let mut events = store.subscribe(1).unwrap_or_abort();
        let request_id = self
            .coordinator
            .request_agent_turn_with_model_target(
                supervisor_actor(),
                self.agent_id.clone(),
                prompt,
                target,
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

    pub(crate) fn events(&self) -> Vec<EventEnvelopeV1> {
        load_events(&self.run.events_path)
    }

    pub(crate) async fn stop(&self) {
        self.coordinator.stop_run().await.unwrap_or_abort();
    }
}
