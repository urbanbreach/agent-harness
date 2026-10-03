use super::*;

pub(super) struct NotificationGate {
    pub(super) parent: tokio::sync::Semaphore,
    pub(super) child: tokio::sync::Semaphore,
    pub(super) parent_started: tokio::sync::Notify,
    pub(super) child_started: tokio::sync::Notify,
    pub(super) collect_reports: usize,
    pub(super) continue_parent: bool,
    pub(super) spawn_reports: usize,
    pub(super) report: String,
    pub(super) reminders: tokio::sync::Mutex<Vec<String>>,
    pub(super) wakeups: AtomicUsize,
    pub(super) collected: tokio::sync::Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl harness_providers::Provider for NotificationGate {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        let Some(last) = request.messages.last() else {
            return Box::pin(tokio_stream::iter([Stream::error("missing prompt")]));
        };
        for message in request.messages.iter().filter(|message| {
            message.role == harness_providers::MessageRole::Tool
                && message.name.as_deref() == Some("spawn_subagent")
        }) {
            let id = message
                .content
                .lines()
                .find_map(|line| line.strip_prefix("subagent_id: "));
            let Some(id) = id else {
                return Box::pin(tokio_stream::iter([Stream::error(
                    "spawn result omitted its subagent identity",
                )]));
            };
            let mut collected = self.collected.lock().await;
            if !collected.iter().any(|known| known == id) {
                collected.push(id.into());
            }
        }
        if last.content == "launch" {
            let calls = (0..self.collect_reports.max(self.spawn_reports).max(1)).map(|index| Stream::ToolCallComplete {
                tool_call_id: format!("launch-child-{index}"), function_name: "spawn_subagent".into(),
                arguments_json: json!({"prompt":if index == 0 || self.collect_reports == 0 {"child work"} else {"child failure"},"description":"Child report"}).to_string(),
            }).chain(std::iter::once(Stream::Done { usage: None })).collect::<Vec<_>>();
            return Box::pin(tokio_stream::iter(calls));
        }
        let answer = if last.content.starts_with("<agent_message sender=") {
            "reactivated child answer"
        } else if matches!(last.content.as_str(), "child work" | "child failure") {
            self.child_started.notify_one();
            if let Ok(permit) = self.child.acquire().await {
                permit.forget();
            }
            if last.content == "child failure" {
                return Box::pin(tokio_stream::iter([Stream::error("child failed")]));
            }
            return Box::pin(tokio_stream::iter([
                Stream::TextDelta(self.report.clone()),
                Stream::DoneWithMetadata {
                    usage: None,
                    metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
                        settled_reasoning: Some(Vec::new()),
                        ..Default::default()
                    }),
                },
            ]));
        } else if last.role == harness_providers::MessageRole::Tool
            && !last.content.contains("<system-reminder>")
        {
            if self.collect_reports > 0 {
                let block = last.name.as_deref() == Some("get_command_or_subagent_output");
                if block {
                    let statuses: Vec<_> = last
                        .content
                        .lines()
                        .filter_map(|line| {
                            line.strip_prefix("Status: ").or_else(|| {
                                line.strip_prefix("--- Task ")?
                                    .rsplit_once(" [")?
                                    .1
                                    .strip_suffix("] ---")
                            })
                        })
                        .collect();
                    let complete = statuses.len() == self.collect_reports
                        && statuses
                            .iter()
                            .all(|status| matches!(*status, "completed" | "failed"));
                    if complete {
                        return Box::pin(tokio_stream::iter([
                            Stream::TextDelta("reports summarized".into()),
                            Stream::Done { usage: None },
                        ]));
                    }
                }
                return Box::pin(tokio_stream::iter([
                    Stream::ToolCallComplete {
                        tool_call_id: format!("collect-{block}"),
                        function_name: "get_command_or_subagent_output".into(),
                        arguments_json: json!({"task_ids":self.collected.lock().await.clone(),"timeout_ms":if block {30_000} else {0}})
                            .to_string(),
                    },
                    Stream::Done { usage: None },
                ]));
            }
            self.parent_started.notify_one();
            if let Ok(permit) = self.parent.acquire().await {
                permit.forget();
            }
            if self.continue_parent {
                return Box::pin(tokio_stream::iter([
                    Stream::ToolCallComplete {
                        tool_call_id: "unrelated-poll".into(),
                        function_name: "get_command_or_subagent_output".into(),
                        arguments_json: json!({"task_ids":["missing-child"],"timeout_ms":0})
                            .to_string(),
                    },
                    Stream::Done { usage: None },
                ]));
            }
            "parent finished"
        } else if last.content == "check history" {
            assert!(
                request.messages.iter().all(|message| message.role
                    != harness_providers::MessageRole::User
                    || !message.content.contains("child report")),
                "consumed notifications must not reappear in provider history"
            );
            "history checked"
        } else {
            self.wakeups.fetch_add(1, Ordering::SeqCst);
            self.reminders.lock().await.push(last.content.clone());
            assert_eq!(
                request
                    .messages
                    .iter()
                    .filter(|message| {
                        matches!(
                            message.role,
                            harness_providers::MessageRole::User
                                | harness_providers::MessageRole::Tool
                        ) && message.content.contains("child report")
                    })
                    .count(),
                1,
                "wake prompts and buffered reminders must deliver a completion once"
            );
            assert!(
                last.content.contains("child report"),
                "notification must include the child result"
            );
            assert_eq!(
                last.role,
                if self.continue_parent {
                    harness_providers::MessageRole::Tool
                } else {
                    harness_providers::MessageRole::User
                }
            );
            assert!(last.content.ends_with("</system-reminder>"));
            assert!(last.content.contains(if self.continue_parent {
                "Background subagent \""
            } else {
                "While you were idle, "
            }));
            assert!(last.content.contains("=== Task "));
            assert!(last.content.contains("<subagent_meta>"));
            "notification delivered"
        };
        Box::pin(tokio_stream::iter([
            Stream::TextDelta(answer.into()),
            Stream::Done { usage: None },
        ]))
    }
}

pub(super) fn configure_reactivation(config: &mut CoordinatorConfig) {
    use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget};
    config.subagents.messaging_enabled = true;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_subagent_tools(
        &mut registry,
        &config.subagents,
        &Default::default(),
        config.subagent_model_catalog.as_ref(),
    );
    config.tool_registry = Arc::new(registry);
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
                Some(32_768),
                Some(30_000),
                Some(2_000),
                ModelLimitProvenance::explicit("notification fixture"),
            ),
            resolution: Default::default(),
            catalog_entry: None,
        },
    );
}
