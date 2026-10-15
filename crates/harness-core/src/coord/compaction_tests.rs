use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_compaction::{prompts::summary_message, CompactionMethod};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};

mod budget;
mod commit;
mod continuation;
mod maintenance;
mod maintenance_triggers;
mod methods;
mod retention;
mod speculation;
mod triggers;

pub(super) const SUMMARY: &str = "## Goal\nFinish the rewrite.\n## Constraints\nKeep the UI unchanged.\n## Progress\nThe first step is complete.\n## Key Decisions\nUse bounded queues.\n## Next Steps\nContinue the work.\n## Critical Context\nThe recent turn is retained below.";
pub(super) fn answer(text: impl Into<String>) -> Vec<Stream> {
    vec![Stream::TextDelta(text.into()), Stream::Done { usage: None }]
}
pub(super) async fn settled(
    coordinator: &CoordinatorHandle,
    id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(event) if event.task_id.as_str() == id => return Ok(()),
                EventV1::TaskCancelled(event) if event.task_id.as_str() == id => {
                    return Err::<(), Box<dyn std::error::Error>>(
                        std::io::Error::other(event.reason).into(),
                    )
                }
                _ => {}
            }
        }
        Err(std::io::Error::other("turn did not settle").into())
    })
    .await?
}

#[tokio::test]
async fn automatic_compaction_obeys_gates_and_stops_overflow_retries_when_no_cut_remains(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_providers::ProviderErrorCategory::ContextWindowExceeded;
    for mode in [
        "threshold",
        "overflow",
        "disabled",
        "retry_disabled",
        "second_overflow",
        "truncated_summary",
    ] {
        let temp = tempfile::tempdir()?;
        let overflow = mode != "threshold";
        let enabled = !matches!(mode, "disabled" | "retry_disabled");
        let truncated = mode == "truncated_summary";
        let mut script = vec![answer("old answer ".repeat(1000)), answer("latest answer")];
        if overflow {
            script.push(vec![Stream::categorized_error(
                "input too long",
                ContextWindowExceeded,
            )]);
        }
        if enabled {
            script.push(if truncated {
                vec![
                    Stream::TextDelta(SUMMARY.into()),
                    Stream::DoneWithMetadata {
                        usage: None,
                        metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
                            output_limit_reached: true,
                            ..Default::default()
                        }),
                    },
                ]
            } else {
                answer(SUMMARY)
            });
            script.push(answer("Rewrite checkpoint"));
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
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.compaction.method_order = vec![CompactionMethod::Soft];
        config.compaction.async_enabled = false;
        config.compaction.keep_recent_tokens = 100;
        config.compaction.reserve_tokens = Some(0);
        config.compaction.fallback_input_tokens = 32_000;
        config.compaction.threshold_tokens =
            std::num::NonZeroU32::new(if overflow { u32::MAX } else { 5_000 });
        config.compaction.enabled = false;
        config.compaction.auto_retry_overflow = mode != "retry_disabled";
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run(mode, temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        for prompt in ["old request ".repeat(1000), "latest request".into()] {
            let turn = coordinator
                .request_agent_turn(handle::system(), &agent, prompt)
                .await?;
            super::history_tests::settled(&coordinator, &turn).await?;
        }
        coordinator
            .call(move |s| {
                s.config.compaction.enabled = mode != "disabled";
                Ok(())
            })
            .await?;
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "continue")
            .await?;
        let result = super::history_tests::settled(&coordinator, &turn).await;
        assert_eq!(
            result.is_ok(),
            enabled && mode != "second_overflow",
            "{mode}: {result:?}"
        );
        coordinator.stop_run().await?;
        let events = crate::store::read_events(&run.events_path)?;
        let compactions: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.payload {
                EventV1::SessionCompaction(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(compactions.len(), usize::from(enabled), "{mode}");
        if enabled {
            let summary = compactions[0];
            assert_eq!(summary.method, Some(CompactionMethod::Soft));
            assert_eq!(summary.short_summary.as_deref(), Some("Rewrite checkpoint"));
            let requests = provider.captured_requests().await;
            let final_request = requests.last().ok_or("missing retry")?;
            assert!(
                final_request
                    .messages
                    .iter()
                    .any(|m| m.content == summary_message(summary.method, &summary.summary)),
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
                    .any(|m| m.content.contains(&"old request ".repeat(1000))),
                "{mode}"
            );
        }
        assert_eq!(
            provider.call_count(),
            if enabled {
                5 + usize::from(overflow)
            } else {
                3
            },
            "{mode}"
        );
    }
    Ok(())
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
            answer(SUMMARY),
            answer("Rewrite checkpoint"),
            answer("after successful compaction"),
        ]),
        entered: tokio::sync::Notify::new(),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.keep_recent_tokens = 100;
    config.compaction.enabled = false;
    config.compaction.method_order = vec![CompactionMethod::Soft];
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
    // A second manual pass while one owns the agent is rejected outright.
    assert!(matches!(
        coordinator
            .compact_agent_context(agent.clone(), None, "manual")
            .await,
        Err(CoordinatorError::CompactionRejected(
            super::compaction::ManualRejection::InProgress
        ))
    ));
    let cancelled = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "cancelled-before-start sentinel",
        )
        .await?;
    coordinator
        .cancel_task(&cancelled, "withdraw request")
        .await?;
    let next = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "after cancellation",
        )
        .await?;
    coordinator.cancel_compaction(agent.clone()).await?;
    for task in [active] {
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
    assert!(matches!(
        coordinator
            .compact_agent_context(agent.clone(), None, "manual")
            .await?,
        ManualCompactionOutcome::Compacted { .. }
    ));
    let events = crate::store::read_events(&run.events_path)?;
    let summary = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(summary) => Some(&summary.summary),
            _ => None,
        })
        .ok_or("missing successful compaction")?;
    assert!(!summary.contains("cancelled-before-start sentinel"));
    let next = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent,
            "continue after compaction",
        )
        .await?;
    super::history_tests::settled(&coordinator, &next).await?;
    coordinator.stop_run().await?;
    let requests = provider.provider.captured_requests().await;
    assert_eq!(requests.len(), 7);
    assert!(requests[3]
        .messages
        .iter()
        .any(|m| m.content == "old request"));
    assert!(!requests[3]
        .messages
        .iter()
        .any(|m| m.content.contains(SUMMARY)));
    assert!(requests
        .last()
        .ok_or("missing continuation")?
        .messages
        .iter()
        .any(|message| message.content == summary_message(Some(CompactionMethod::Soft), summary)));
    Ok(())
}
