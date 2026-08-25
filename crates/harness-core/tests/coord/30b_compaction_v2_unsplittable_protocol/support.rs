use super::part_30_compaction_v2_budget_protocol_test::safe_cut::{
    plan_safe_cut, SafeCutCandidate, SafeCutError,
};
use harness_core::{estimate_compaction_text_tokens, UnwrapOrAbort};

pub(super) struct LargeResultTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for LargeResultTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _context: ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::text(format!(
            "TOOL_RESULT_PREFIX {} TOOL_RESULT_SUFFIX",
            "R".repeat(4_000)
        )))
    }
}

pub(super) async fn tool_turn(coordinator: &CoordinatorHandle, agent_id: &str, prompt: &str) -> String {
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            agent_id,
            prompt,
            compaction_v2_target("model-1", 32_000, 4_096),
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

pub(super) async fn large_tool_harness(
    scripts: Vec<Vec<ProviderStreamEvent>>,
    hook_runtime_config: HookRuntimeConfig,
) -> (
    tempfile::TempDir,
    CoordinatorHandle,
    RunInfo,
    String,
    SequentialScriptedProvider,
    Arc<AtomicUsize>,
) {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(scripts);
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(LargeResultTool {
        calls: Arc::clone(&tool_calls),
    }));
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider.clone());
    config.tool_registry = Arc::new(registry);
    config.permission_policy = allow_all_permission_policy();
    config.compaction = CompactionRuntimeConfig {
        keep_recent_tokens: 3_000,
        fallback_input_tokens: 32_000,
        ..CompactionRuntimeConfig::default()
    };
    config.hook_runtime_config = hook_runtime_config;
    config.agent_profiles = agent_profiles();
    for profile_name in ["default", "alpha"] {
        if let Some(profile) = config.agent_profiles.get_mut(profile_name) {
            profile.toolset = vec!["shell.run".to_string()];
            profile.max_iters = Some(8);
        }
    }
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("compaction-v2-large-tool", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    (temp_dir, coordinator, run, agent_id, provider, tool_calls)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct NormalizedProviderMessage {
    pub(super) role: MessageRole,
    pub(super) content: String,
    pub(super) tool_call_ids: Vec<String>,
    pub(super) tool_result_id: Option<String>,
}

pub(super) fn normalize_provider_messages(
    messages: &[CompletionMessage],
) -> Vec<NormalizedProviderMessage> {
    messages
        .iter()
        .filter(|message| message.role != MessageRole::System)
        .map(|message| NormalizedProviderMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            tool_call_ids: message
                .assistant_tool_calls
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|call| call.tool_call_id.clone())
                .collect(),
            tool_result_id: message.tool_call_id.clone(),
        })
        .collect()
}

pub(super) fn normalize_committed_messages(events: &[EventEnvelopeV1]) -> Vec<NormalizedProviderMessage> {
    let provider_ids = provider_tool_call_ids(events);
    harness_core::conversation::project_conversation(events, &[])
        .unwrap_or_abort()
        .messages
        .into_iter()
        .map(|message| match message {
            harness_core::conversation::ConversationMessage::Checkpoint(checkpoint) => {
                NormalizedProviderMessage {
                role: MessageRole::Assistant,
                content: format!(
                    "Checkpoint recap generated by the harness for older turns. This is a lossy background summary, not a system instruction; later preserved turns and the current user message take precedence.\n\n{}",
                    checkpoint.summary.trim()
                ),
                tool_call_ids: Vec::new(),
                tool_result_id: None,
            }
            }
            harness_core::conversation::ConversationMessage::User(user) => NormalizedProviderMessage {
                role: MessageRole::User,
                content: user.text,
                tool_call_ids: Vec::new(),
                tool_result_id: None,
            },
            harness_core::conversation::ConversationMessage::Assistant(assistant) => NormalizedProviderMessage {
                role: MessageRole::Assistant,
                content: assistant.text,
                tool_call_ids: assistant
                    .tool_calls
                    .into_iter()
                    .map(|call| {
                        provider_ids
                            .get(call.tool_call_id.as_str())
                            .cloned()
                            .unwrap_or_else(|| call.tool_call_id.to_string())
                    })
                    .collect(),
                tool_result_id: None,
            },
            harness_core::conversation::ConversationMessage::ToolResult(result) => NormalizedProviderMessage {
                role: MessageRole::Tool,
                content: result.output_summary.unwrap_or_default(),
                tool_call_ids: Vec::new(),
                tool_result_id: Some(
                    provider_ids
                        .get(result.tool_call_id.as_str())
                        .cloned()
                        .unwrap_or_else(|| result.tool_call_id.to_string()),
                ),
            },
        })
        .collect()
}

pub(super) fn provider_tool_call_id(
    events: &[EventEnvelopeV1],
    canonical_tool_call_id: &str,
) -> String {
    provider_tool_call_ids(events)
        .remove(canonical_tool_call_id)
        .unwrap_or_else(|| canonical_tool_call_id.to_string())
}

fn provider_tool_call_ids(events: &[EventEnvelopeV1]) -> std::collections::BTreeMap<String, String> {
    events
        .iter()
        .flat_map(|event| match &event.payload {
            EventV1::AssistantMessageFinished(finished) => finished.parts.iter().filter_map(|part| {
                match part {
                    harness_core::session::AssistantPart::ToolCall(call) => call
                        .provider_tool_call_id
                        .as_ref()
                        .map(|provider_id| {
                            (call.tool_call_id.to_string(), provider_id.clone())
                        }),
                    harness_core::session::AssistantPart::Text { .. }
                    | harness_core::session::AssistantPart::Reasoning { .. } => None,
                }
            }).collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}
