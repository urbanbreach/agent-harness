use super::*;
use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

struct ProgressProvider {
    calls: AtomicUsize,
    new_tool_tokens: AtomicU64,
    waiting: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}

#[async_trait::async_trait]
impl harness_providers::Provider for ProgressProvider {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        if request
            .messages
            .last()
            .is_some_and(|message| message.content == "completed sibling")
        {
            return Box::pin(tokio_stream::iter([
                Stream::TextDelta("sibling finished".into()),
                Stream::Done { usage: None },
            ]));
        }
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Box::pin(tokio_stream::iter([
                Stream::TextDelta("Model output already included in reported usage. ".repeat(100)),
                Stream::ToolCallComplete {
                    tool_call_id: "write-progress".into(),
                    function_name: "write".into(),
                    arguments_json: json!({"path":"progress.txt","content":"child side effect"})
                        .to_string(),
                },
                Stream::ToolCallComplete {
                    tool_call_id: "rejected-progress".into(),
                    function_name: "unavailable-tool".into(),
                    arguments_json: "{}".into(),
                },
                Stream::DoneWithMetadata {
                    usage: Some(CompletionUsage {
                        prompt_tokens: 8000,
                        completion_tokens: 1000,
                        total_tokens: 9000,
                    }),
                    metadata: Some(ProviderStreamFinishedMetadata {
                        usage_complete: Some(true),
                        settled_reasoning: Some(Vec::new()),
                        ..Default::default()
                    }),
                },
            ]));
        }
        assert!(request
            .messages
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("write-progress")));
        self.new_tool_tokens.store(
            request
                .messages
                .iter()
                .filter(|message| message.role == harness_providers::MessageRole::Tool)
                .map(|message| message.content.len() as u64 / 4)
                .sum(),
            Ordering::SeqCst,
        );
        self.waiting.notify_one();
        if let Ok(permit) = self.release.acquire().await {
            permit.forget();
        }
        Box::pin(tokio_stream::iter([
            Stream::TextDelta("child finished".into()),
            Stream::DoneWithMetadata {
                usage: None,
                metadata: Some(ProviderStreamFinishedMetadata {
                    settled_reasoning: Some(Vec::new()),
                    ..Default::default()
                }),
            },
        ]))
    }
}

#[tokio::test]
async fn running_child_reports_current_progress_without_double_counting_model_output(
) -> Result<(), Box<dyn std::error::Error>> {
    for known_window in [true, false] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(ProgressProvider {
            calls: AtomicUsize::new(0),
            new_tool_tokens: AtomicU64::new(0),
            waiting: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        });
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.tool_registry = Arc::new(harness_tools::coordinator_registry(
            ShellAllowlist::default(),
        ));
        config.permission_policy = PermissionPolicy::allow_all();
        let mut profile = AgentProfile::fallback("default");
        profile.toolset = vec![
            "spawn_subagent".into(),
            "get_command_or_subagent_output".into(),
            "wait_commands_or_subagents".into(),
            "write".into(),
        ];
        config.agent_profiles.insert("default".into(), profile);
        if known_window {
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
                    limits: ResolvedModelLimits::from_values(
                        Some(10_000),
                        Some(9_000),
                        Some(1_000),
                        ModelLimitProvenance::explicit("progress fixture"),
                    ),
                    resolution: Default::default(),
                    catalog_entry: None,
                },
            );
        }
        let clock = Arc::new(FakeClock::new());
        let coordinator = spawn_coordinator(
            config,
            Arc::<FakeClock>::clone(&clock),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator
            .start_run("live child progress", temp.path())
            .await?;
        let parent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let actor = EventActor::new(ActorKind::Worker, Some(parent));
        let started = coordinator
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "spawn_subagent",
                json!({"prompt":"write and report","description":"Progress fixture"}),
            )
            .await?;
        let child = started
            .structured_json
            .as_ref()
            .and_then(|value| value["subagent_id"].as_str())
            .ok_or("child id missing")?;
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            provider.waiting.notified(),
        )
        .await?;
        clock.advance(2500);
        let progress = coordinator
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "get_command_or_subagent_output",
                json!({"task_ids":[child]}),
            )
            .await?;
        let value = &progress
            .structured_json
            .as_ref()
            .ok_or("progress missing")?["Result"];
        assert_eq!(value["status"], "running");
        assert_eq!(value["duration_secs"], 2.5);
        let text = value["output"].as_str().ok_or("progress text missing")?;
        assert!(
            text.contains("Elapsed: 2.5s\nProgress: turn 1, 1 tool calls, 9K/"),
            "{text}"
        );
        let context = 9000 + provider.new_tool_tokens.load(Ordering::SeqCst);
        assert!(
            context < 10_000,
            "the tool results fit the remaining context"
        );
        let expected = if known_window {
            format!("9K/10K tokens ({}% context)", context / 100)
        } else {
            "9K/unknown tokens (unknown context)".into()
        };
        assert!(text.contains(&expected), "{text}");
        let (body, hint) = text.split_once("\n\n").ok_or("poll hint missing")?;
        assert!(body.ends_with("Tools used: write\nErrors: 0"), "{text}");
        assert!(hint.starts_with("Use timeout_ms to wait for completion. Unless the user specified, do not kill this subagent and do not tell it to stop"));
        assert_eq!(
            value["raw_output_bytes"],
            body.len(),
            "advisory text is not child output"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("progress.txt"))?,
            "child side effect"
        );
        for (tool, timeout, prefix) in [
            (
                "get_command_or_subagent_output",
                1,
                "Waited the requested 1ms.",
            ),
            ("wait_commands_or_subagents", 0, "Waited the requested 0ms."),
        ] {
            let mut input = json!({"task_ids":[child], "timeout_ms":timeout});
            if tool == "wait_commands_or_subagents" {
                input["mode"] = json!("wait_any");
            }
            let output = coordinator
                .execute_agent_tool_call(actor.clone(), None, tool, input)
                .await?;
            let value = output.structured_json.ok_or("timed wait result missing")?;
            let value = value
                .get("Result")
                .unwrap_or(&value["MultiResult"]["results"][0]);
            assert!(value["output"]
                .as_str()
                .ok_or("timed wait body missing")?
                .contains(prefix));
            assert_eq!(value["status"], "running");
        }
        let sibling = coordinator.execute_agent_tool_call(actor.clone(), None, "spawn_subagent",
            json!({"prompt":"completed sibling", "description":"completed sibling", "background":false})).await?;
        let sibling = sibling
            .structured_json
            .as_ref()
            .and_then(|value| value["subagent_id"].as_str())
            .ok_or("sibling id missing")?;
        let pending = coordinator
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "wait_commands_or_subagents",
                json!({"task_ids":[child,sibling], "mode":"wait_any", "timeout_ms":1}),
            )
            .await?;
        let pending = pending
            .structured_json
            .ok_or("pending wait result missing")?;
        assert!(
            pending["MultiResult"]["results"][0]["output"]
                .as_str()
                .ok_or("pending wait body missing")?
                .contains("Waited the requested 1ms."),
            "an already-completed sibling must not end a wait for pending work"
        );
        assert_eq!(pending["MultiResult"]["results"][1]["status"], "completed");
        provider.release.add_permits(1);
        let completed = coordinator
            .execute_agent_tool_call(
                actor,
                None,
                "get_command_or_subagent_output",
                json!({"task_ids":[child],"timeout_ms":3000}),
            )
            .await?;
        assert_eq!(
            completed
                .structured_json
                .as_ref()
                .ok_or("completion missing")?["Result"]["status"],
            "completed"
        );
        assert_eq!(
            coordinator.subagent_history().await?.records[child]
                .accounting
                .as_ref()
                .map(|a| (a.tool_calls, a.tokens_used)),
            Some((1, Some(9000)))
        );
        coordinator.stop_run().await?;
    }
    Ok(())
}
