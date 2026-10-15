use super::maintenance::RoutedSummaries;
use super::*;
use crate::event::{LifecyclePhase, RuntimeEvent};
use harness_providers::ProviderSessionEvent;

/// Routes like `RoutedSummaries` and records the session events the provider hears.
struct Spy {
    inner: RoutedSummaries,
    events: std::sync::Mutex<Vec<ProviderSessionEvent>>,
}
#[async_trait::async_trait]
impl harness_providers::Provider for Spy {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        self.inner.stream_completion(request).await
    }
    fn session_event(&self, event: &ProviderSessionEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event.clone());
        }
    }
}

/// A threshold pass reports a start and an end lifecycle event that share the operation id
/// persisted on the compaction record, and the end carries the committed result.
#[tokio::test]
async fn compaction_lifecycle_events_pair_with_the_committed_record(
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(Spy {
        inner: RoutedSummaries {
            turns: MockProvider::script([
                answer("old answer ".repeat(1000)),
                answer("latest answer"),
                answer("final text answer"),
            ]),
            summaries: MockProvider::script([answer(SUMMARY), answer("Rewrite checkpoint")]),
        },
        events: Default::default(),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.method_order = vec![CompactionMethod::Soft];
    config.compaction.keep_recent_tokens = 100;
    config.compaction.reserve_tokens = Some(0);
    config.compaction.fallback_input_tokens = 32_000;
    config.compaction.threshold_tokens = std::num::NonZeroU32::new(5_000);
    config.compaction.enabled = false;
    config.compaction.async_enabled = false;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("lifecycle", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for text in ["old request ".repeat(1000), "latest request".into()] {
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, text)
            .await?;
        settled(&coordinator, &turn).await?;
    }
    let store = coordinator.event_store().await?;
    let mut live = store.subscribe_runtime(1)?;
    coordinator
        .call(|s| {
            s.config.compaction.enabled = true;
            Ok(())
        })
        .await?;
    let turn = coordinator
        .request_agent_turn(handle::system(), &agent, "continue")
        .await?;
    settled(&coordinator, &turn).await?;
    let lifecycle = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut seen = Vec::new();
        while let Some(event) = live.next().await {
            let Ok(RuntimeEvent::Live(e)) = event else {
                continue;
            };
            let LiveEventV1::CompactionLifecycle(lifecycle) = e.payload else {
                continue;
            };
            let end = lifecycle.phase == LifecyclePhase::End;
            seen.push(lifecycle);
            if end {
                break;
            }
        }
        seen
    })
    .await?;
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    let record = events
        .iter()
        .find_map(|e| match &e.payload {
            EventV1::SessionCompaction(c) => Some(c),
            _ => None,
        })
        .ok_or("missing compaction record")?;
    let [start, end] = lifecycle.as_slice() else {
        return Err(format!("expected start and end, got {}", lifecycle.len()).into());
    };
    assert_eq!(start.phase, LifecyclePhase::Start);
    assert_eq!(start.reason, "threshold");
    assert_eq!(start.action, "context-full");
    assert_eq!(Some(&start.operation_id), record.operation_id.as_ref());
    assert_eq!(end.operation_id, start.operation_id);
    assert!(!end.will_retry && !end.aborted && !end.skipped && end.error.is_none());
    let result = end.result.as_ref().ok_or("end without result")?;
    assert_eq!(result.method, Some(CompactionMethod::Soft));
    assert_eq!(result.tokens_before, Some(record.tokens_before));
    // The provider hears the compaction with the same operation id, trigger and phase.
    let heard = provider.events.lock().map_err(|_| "poisoned")?;
    let descriptor = heard
        .iter()
        .find_map(|event| match event {
            ProviderSessionEvent::Compacted { descriptor, .. } => descriptor.clone(),
            _ => None,
        })
        .ok_or("compaction reached the provider without a descriptor")?;
    assert_eq!(Some(&descriptor.operation_id), record.operation_id.as_ref());
    assert_eq!(descriptor.trigger, "auto");
    assert_eq!(descriptor.phase, "pre_turn");
    Ok(())
}

/// The compaction is committed before the progress guard judges it: a kept tail too large
/// for the recovery band leaves the record in place and stamps the dead-end warning on it
/// as an append-only patch, with no second summary.
#[tokio::test]
async fn oversized_kept_tail_commits_first_then_stamps_the_warning_on_the_record(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::event::CompactionPatch;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(RoutedSummaries {
        turns: MockProvider::script([
            answer("old answer ".repeat(500)),
            answer("latest answer ".repeat(2_000)),
            answer("final text answer"),
        ]),
        // The turn is split, so the prefix gets its own summary before the short one.
        summaries: MockProvider::script([
            answer(SUMMARY),
            answer("The request was long."),
            answer("Rewrite checkpoint"),
        ]),
    });
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.method_order = vec![CompactionMethod::Soft];
    config.compaction.keep_recent_tokens = 100;
    config.compaction.reserve_tokens = Some(0);
    config.compaction.fallback_input_tokens = 32_000;
    config.compaction.threshold_tokens = std::num::NonZeroU32::new(5_000);
    config.compaction.enabled = false;
    config.compaction.async_enabled = false;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("dead end", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for text in ["old request ".repeat(500), "latest request".into()] {
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, text)
            .await?;
        settled(&coordinator, &turn).await?;
    }
    coordinator
        .call(|s| {
            s.config.compaction.enabled = true;
            Ok(())
        })
        .await?;
    let turn = coordinator
        .request_agent_turn(handle::system(), &agent, "continue")
        .await?;
    settled(&coordinator, &turn).await?;
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    let compactions: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
        .collect();
    assert_eq!(compactions.len(), 1, "no second summary");
    let patches: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::CompactionPatched(p) => Some((e.seq, p)),
            _ => None,
        })
        .collect();
    let [(patched_at, patch)] = patches.as_slice() else {
        return Err(format!("expected one warning patch, got {}", patches.len()).into());
    };
    assert!(compactions[0].seq < *patched_at);
    assert_eq!(patch.compaction_seq, compactions[0].seq);
    assert!(matches!(&patch.patch,
        CompactionPatch::Warning { text } if text.starts_with("Compaction freed too little context")));
    Ok(())
}

/// Every manual pass reports a start and an end, whether it shook, handed off, compacted or
/// was rejected, because the UI releases its queue on the end event.
#[tokio::test]
async fn manual_passes_always_end_their_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("manual lifecycle", temp.path())
        .await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    let store = coordinator.event_store().await?;
    let mut live = store.subscribe_runtime(1)?;
    let (sender, mut lifecycle) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = live.next().await {
            if let Ok(RuntimeEvent::Live(event)) = event
                && let LiveEventV1::CompactionLifecycle(item) = event.payload
            {
                let _ = sender.send(item);
            }
        }
    });
    let _ = coordinator
        .shake_agent_context(agent.clone(), harness_compaction::ShakeMode::Elide)
        .await;
    let _ = coordinator.handoff_agent_context(agent.clone(), None).await;
    let _ = coordinator
        .compact_agent_context(agent.clone(), None, "manual")
        .await;
    let mut phases = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while phases.len() < 6 {
            let Some(item) = lifecycle.recv().await else {
                break;
            };
            phases.push((item.action, item.phase));
        }
    })
    .await?;
    coordinator.stop_run().await?;
    for action in ["shake", "handoff", "context-full"] {
        let own: Vec<_> = phases.iter().filter(|(a, _)| a == action).collect();
        assert_eq!(
            own.iter().map(|(_, phase)| *phase).collect::<Vec<_>>(),
            [LifecyclePhase::Start, LifecyclePhase::End],
            "{action}: {phases:?}"
        );
    }
    Ok(())
}
