use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};

pub(super) const SUMMARY: &str = "## Goal\nFinish the rewrite.\n## Constraints\nKeep the UI unchanged.\n## Progress\nThe first step is complete.\n## Key Decisions\nUse bounded queues.\n## Next Steps\nContinue the work.\n## Critical Context\nThe recent turn is retained below.";
pub(super) fn answer(text: impl Into<String>) -> Vec<Stream> {
    vec![Stream::TextDelta(text.into()), Stream::Done { usage: None }]
}

#[tokio::test]
async fn compaction_keeps_recent_turns_and_restores_the_same_context_after_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    for child_session in [false, true] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(MockProvider::script([
            answer("old answer ".repeat(1000)),
            answer("latest answer"),
            answer("Incomplete summary without sections."),
            answer(SUMMARY),
            answer("after compaction"),
            answer("after resume"),
        ]));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.compaction.keep_recent_tokens = 1;
        config.compaction.reserve_tokens = 0;
        config.compaction.suppress_auto_compaction = true;
        let coordinator = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run("compact", temp.path()).await?;
        let mut agent = coordinator
            .spawn_agent_idle(
                EventActor::new(ActorKind::Supervisor, None),
                "default",
                None,
            )
            .await?;
        if child_session {
            agent = coordinator
                .spawn_agent_idle(
                    EventActor::new(ActorKind::User, None),
                    "default",
                    Some(agent),
                )
                .await?;
        }
        assert!(matches!(
            coordinator
                .compact_agent_context_with_instructions(agent.clone(), None, "manual", None)
                .await?,
            ManualCompactionOutcome::NoOp
        ));
        for (index, prompt) in ["old request ".repeat(1000), "latest request".into()]
            .into_iter()
            .enumerate()
        {
            let turn = coordinator
                .request_agent_turn_with_model_and_selected_tags_and_attachments(
                    EventActor::new(ActorKind::User, None),
                    agent.clone(),
                    prompt,
                    crate::file_tag::SelectedPromptTags::default(),
                    if index == 0 {
                        vec![crate::attachment_transport::AttachmentMetadata::from_bytes(
                            "note",
                            "text/plain",
                            None,
                            b"Remember the attachment when summarizing.",
                            None,
                        )]
                    } else {
                        Vec::new()
                    },
                    None,
                    None,
                )
                .await?;
            super::history_tests::settled(&coordinator, &turn).await?;
        }
        assert!(coordinator
            .compact_agent_context(agent.clone(), None, "manual")
            .await
            .is_err());
        assert!(!crate::store::read_events(&run.events_path)?
            .iter()
            .any(|e| matches!(e.payload, EventV1::SessionCompaction(_))));
        let before = std::fs::read(&run.events_path)?;
        let outcome = coordinator
            .compact_agent_context_with_instructions(
                agent.clone(),
                None,
                "manual",
                Some("Keep the chosen queue design.".into()),
            )
            .await?;
        assert!(
            matches!(outcome, ManualCompactionOutcome::Compacted { tokens_before, tokens_after, .. } if tokens_after < tokens_before)
        );
        assert!(std::fs::read(&run.events_path)?.starts_with(&before));
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 4);
        assert!(requests[3].tools.is_none());
        assert_eq!(
            requests[3]
                .attachments
                .values()
                .flatten()
                .next()
                .ok_or("summary lost attachment")?
                .bytes()?,
            b"Remember the attachment when summarizing."
        );
        assert!(requests[3]
            .messages
            .iter()
            .any(|m| m.content.contains("Keep the chosen queue design.")));
        let turn = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                "continue",
            )
            .await?;
        super::history_tests::settled(&coordinator, &turn).await?;
        let requests = provider.captured_requests().await;
        assert!(requests[4]
            .messages
            .iter()
            .any(|m| m.content.contains(SUMMARY)));
        assert!(requests[4]
            .messages
            .iter()
            .any(|m| m.content == "latest answer"));
        assert!(!requests[4]
            .messages
            .iter()
            .any(|m| m.content.contains("old request")));
        assert!(requests[4].attachments.is_empty());
        coordinator.stop_run().await?;
        let history_dir = if child_session {
            config.session_dir.join(&agent)
        } else {
            run.run_dir.clone()
        };
        let events = crate::store::read_events(&history_dir.join("events.jsonl"))?;
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
                .count(),
            1
        );
        assert!(!events.iter().any(|e| matches!(&e.payload, EventV1::AssistantMessageFinished(a) if a.parts.iter().any(|p| matches!(p, crate::session::AssistantPart::Text {text} if text == SUMMARY)))));
        let mut invalid = events.clone();
        for event in &mut invalid {
            if let EventV1::SessionCompaction(e) = &mut event.payload {
                e.first_kept_request_id = Some("missing-retained-turn".into());
            }
        }
        assert!(crate::proj::project_resume_plan(&invalid, run.run_id.as_str()).is_err());
        let stable = crate::session_lineage::latest_clone_stable_prefix(&events)?;
        let child = crate::session_lineage::materialize_child_session(
            crate::session_lineage::ChildSessionMaterializationRequest {
                source_run_dir: &history_dir,
                events: &events,
                stable_prefix: &stable,
                source_kind:
                    crate::session_lineage::ChildSessionMaterializationSourceKind::DiskRunDirectory,
            },
        )?;
        let resumed = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        resumed
            .resume_run(child.child_run_id, "compact resumed")
            .await?;
        let turn = resumed
            .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "next")
            .await?;
        super::history_tests::settled(&resumed, &turn).await?;
        let requests = provider.captured_requests().await;
        assert!(requests[5]
            .messages
            .iter()
            .any(|m| m.content.contains(SUMMARY)));
        assert!(requests[5]
            .messages
            .iter()
            .any(|m| m.content == "latest answer"));
        assert!(!requests[5]
            .messages
            .iter()
            .any(|m| m.content.contains("old request")));
        resumed.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn automatic_compaction_obeys_gates_and_retries_overflow_only_once(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_providers::ProviderErrorCategory::ContextWindowExceeded;
    for mode in [
        "threshold",
        "retention_budget",
        "overflow",
        "disabled",
        "suppressed",
        "retry_disabled",
        "second_overflow",
        "native_context",
    ] {
        let temp = tempfile::tempdir()?;
        // A backend that compacts its own session (senpi's lane policy) stands harness
        // auto-compaction and the input-budget stop down.
        let native = mode == "native_context";
        let large = mode == "retention_budget" || native;
        let overflow = !matches!(mode, "threshold" | "retention_budget" | "native_context");
        let enabled = !matches!(mode, "disabled" | "suppressed" | "retry_disabled");
        let compacts = enabled && !native;
        let mut script = vec![answer("old answer ".repeat(1000)), answer("latest answer")];
        if overflow {
            script.push(vec![Stream::categorized_error(
                "input too long",
                ContextWindowExceeded,
            )]);
        }
        if compacts {
            script.push(answer(SUMMARY));
        }
        script.push(if mode == "second_overflow" {
            vec![Stream::categorized_error(
                "still too long",
                ContextWindowExceeded,
            )]
        } else {
            answer("continued")
        });
        let provider = Arc::new(MockProvider::script(script));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = if native {
            Arc::new(NativeContext(Arc::clone(&provider)))
        } else {
            Arc::clone(&provider) as Arc<dyn harness_providers::Provider>
        };
        config.compaction.keep_recent_tokens = if large { 20_000 } else { 1 };
        config.compaction.fallback_input_tokens = 8_000;
        config.compaction.reserve_tokens = 0;
        config.compaction.threshold_tokens =
            std::num::NonZeroU32::new(if overflow { u32::MAX } else { 5_000 });
        config.compaction.enabled = mode != "disabled";
        config.compaction.suppress_auto_compaction = mode == "suppressed";
        config.compaction.auto_retry_overflow = mode != "retry_disabled";
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run(mode, temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        for prompt in ["old request ".repeat(1000), "latest request".into()] {
            let turn = coordinator
                .request_agent_turn(
                    EventActor::new(ActorKind::User, None),
                    agent.clone(),
                    prompt,
                )
                .await?;
            super::history_tests::settled(&coordinator, &turn).await?;
        }
        let turn = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                if large {
                    "new prompt ".repeat(1200)
                } else {
                    "continue".into()
                },
            )
            .await?;
        let result = super::history_tests::settled(&coordinator, &turn).await;
        assert_eq!(
            result.is_ok(),
            enabled && mode != "second_overflow",
            "{mode}: {result:?}"
        );
        if mode == "disabled" {
            assert_eq!(
                coordinator
                    .compact_agent_context(agent, None, "manual")
                    .await?,
                ManualCompactionOutcome::NoOp
            );
        }
        coordinator.stop_run().await?;
        let events = crate::store::read_events(&run.events_path)?;
        if overflow {
            assert!(events.iter().any(|e| matches!(&e.payload,
                EventV1::ProviderRequestFinished(e) if e.metadata.as_ref()
                    .and_then(|m| m.provider_error_category) == Some(ContextWindowExceeded))));
        }
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
                .count(),
            usize::from(compacts),
            "{mode}"
        );
        assert_eq!(
            provider.call_count(),
            if compacts {
                4 + usize::from(overflow)
            } else {
                3
            },
            "{mode}"
        );
        if compacts {
            let requests = provider.captured_requests().await;
            let final_request = requests.last().ok_or("missing continuation")?;
            assert!(
                final_request
                    .messages
                    .iter()
                    .any(|m| m.content.contains(SUMMARY)),
                "{mode}"
            );
            assert!(
                final_request
                    .messages
                    .iter()
                    .any(|m| m.content == "latest answer"),
                "{mode}"
            );
            assert!(
                !final_request
                    .messages
                    .iter()
                    .any(|m| m.content.contains("old request")),
                "{mode}"
            );
        }
    }
    Ok(())
}

struct NativeContext(Arc<MockProvider>);
#[async_trait::async_trait]
impl harness_providers::Provider for NativeContext {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        self.0.stream_completion(request).await
    }
    fn manages_context(&self, _: &harness_providers::CompletionRequest) -> bool {
        true
    }
}

struct SummaryGate {
    provider: MockProvider,
    entered: tokio::sync::Notify,
}
#[async_trait::async_trait]
impl harness_providers::Provider for SummaryGate {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        let stream = self.provider.stream_completion(request).await;
        if self.provider.call_count() == 3 {
            self.entered.notify_one();
            std::future::pending::<()>().await;
        }
        stream
    }
}
#[tokio::test]
async fn cancelling_active_and_queued_compaction_keeps_history_and_unblocks_prompts(
) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(SummaryGate {
        provider: MockProvider::script([
            answer("old answer ".repeat(1000)),
            answer("latest answer"),
            answer(SUMMARY),
            answer("continued"),
        ]),
        entered: tokio::sync::Notify::new(),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.keep_recent_tokens = 1;
    config.compaction.suppress_auto_compaction = true;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("cancel summary", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    for prompt in ["old request", "latest request"] {
        let turn = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                prompt,
            )
            .await?;
        super::history_tests::settled(&coordinator, &turn).await?;
    }
    let store = coordinator.event_store().await?;
    let mut live = store.subscribe_runtime(1)?;
    let (handle, id) = (coordinator.clone(), agent.clone());
    let active =
        tokio::spawn(async move { handle.compact_agent_context(id, None, "manual").await });
    tokio::time::timeout(Duration::from_secs(3), provider.entered.notified()).await?;
    let (handle, id) = (coordinator.clone(), agent.clone());
    let queued =
        tokio::spawn(async move { handle.compact_agent_context(id, None, "manual").await });
    let mut events = store.subscribe(1)?;
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut compactions = 0;
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskScheduled(e) if e.state == TaskScheduleState::Queued && e.task_id.as_str().starts_with("compact-")) { compactions += 1; }
            if compactions == 2 { return Ok::<_, EventStoreError>(()); }
        }
        Err(EventStoreError::Invalid("queued compaction disappeared"))
    }).await??;
    let next = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "after cancellation",
        )
        .await?;
    coordinator.cancel_compaction(agent).await?;
    for task in [active, queued] {
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(3), task).await??,
            Err(CoordinatorError::CompactionCancelled { .. })
        ));
    }
    super::history_tests::settled(&coordinator, &next).await?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(event) = live.next().await {
            if matches!(
                event?,
                RuntimeEvent::Live(e) if matches!(e.payload, LiveEventV1::CompactionProgress {preview:None, ..})
            ) {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid(
            "compaction progress did not clear",
        ))
    })
    .await??;
    coordinator.stop_run().await?;
    assert!(!crate::store::read_events(&run.events_path)?
        .iter()
        .any(|e| matches!(e.payload, EventV1::SessionCompaction(_))));
    let requests = provider.provider.captured_requests().await;
    assert_eq!(requests.len(), 4);
    assert!(requests[3]
        .messages
        .iter()
        .any(|m| m.content == "old request"));
    assert!(!requests[3]
        .messages
        .iter()
        .any(|m| m.content.contains(SUMMARY)));
    Ok(())
}
