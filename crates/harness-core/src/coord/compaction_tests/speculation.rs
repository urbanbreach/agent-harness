use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const SPECULATED: &str = "## Goal\nSpeculated summary of the long request.";

/// Answers summarization requests itself and scripts everything else.
struct Summaries {
    turns: MockProvider,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl harness_providers::Provider for Summaries {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        if request.messages.first().is_some_and(|message| {
            message.content == harness_compaction::prompts::summarization_system_prompt()
        }) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(tokio_stream::iter(answer(SPECULATED)))
        } else {
            self.turns.stream_completion(request).await
        }
    }
}

/// A context inside the lead band below the threshold prepares a summary in the
/// background; crossing the threshold commits it without another summarizer call.
#[tokio::test]
async fn armed_speculation_commits_at_the_threshold_without_a_second_summary(
) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(Summaries {
        turns: MockProvider::script([answer("first answer"), answer("second answer")]),
        calls: AtomicUsize::new(0),
    });
    let settings = crate::config::load_config_from_str(
        r#"{
            model: 'local/big', provider: { local: {
                type: 'openai_compatible', baseUrl: 'https://example.invalid',
                models: { big: { limit: { context: 100000, output: 8192 } } }
            } }
        }"#,
    )?;
    let target = crate::config::resolve_model_selection(&settings, "local/big", None)?.primary;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.agent_model_targets.insert("default".into(), target);
    config.compaction.method_order = vec![CompactionMethod::Soft];
    config.compaction.threshold_tokens = std::num::NonZeroU32::new(50_000);
    config.compaction.keep_recent_tokens = 1_000;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("speculation", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    // About 44,000 estimated tokens: inside [threshold - 8,192, threshold).
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "word ".repeat(35_200),
        )
        .await?;
    super::settled(&coordinator, &turn).await?;
    let id = agent.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let id = id.clone();
            let armed = coordinator
                .call(move |s| Ok(s.speculation.get(&id).is_some_and(|slot| slot.is_armed())))
                .await?;
            if armed {
                return Ok::<_, CoordinatorError>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await??;
    let speculative_calls = provider.calls.load(Ordering::SeqCst);
    assert!(speculative_calls > 0);

    // The next prompt crosses the threshold inside the grace band.
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "next ".repeat(6_400),
        )
        .await?;
    super::settled(&coordinator, &turn).await?;
    coordinator.stop_run().await?;

    assert_eq!(provider.calls.load(Ordering::SeqCst), speculative_calls);
    let events = crate::store::read_events(&run.events_path)?;
    let summaries: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::SessionCompaction(summary) => Some(summary),
            _ => None,
        })
        .collect();
    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].summary.contains(SPECULATED));
    let requests = provider.turns.captured_requests().await;
    let last = requests.last().ok_or("missing second turn request")?;
    assert!(last.messages.iter().any(|m| m.content.contains(SPECULATED)));
    assert!(!last
        .messages
        .iter()
        .any(|m| m.content.contains(&"word ".repeat(1_000))));
    Ok(())
}

/// Scenario 57: a local method first, or async compaction off, never prepares a summary in
/// the lead band.
#[tokio::test]
async fn local_first_or_async_off_never_speculates() -> Result<(), Box<dyn std::error::Error>> {
    for (order, async_enabled) in [
        (vec![CompactionMethod::Shake, CompactionMethod::Soft], true),
        (vec![CompactionMethod::Soft], false),
    ] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(Summaries {
            turns: MockProvider::script([answer("first answer")]),
            calls: AtomicUsize::new(0),
        });
        let settings = crate::config::load_config_from_str(
            r#"{
                model: 'local/big', provider: { local: {
                    type: 'openai_compatible', baseUrl: 'https://example.invalid',
                    models: { big: { limit: { context: 100000, output: 8192 } } }
                } }
            }"#,
        )?;
        let target = crate::config::resolve_model_selection(&settings, "local/big", None)?.primary;
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.agent_model_targets.insert("default".into(), target);
        config.compaction.method_order = order;
        config.compaction.async_enabled = async_enabled;
        config.compaction.threshold_tokens = std::num::NonZeroU32::new(50_000);
        config.compaction.keep_recent_tokens = 1_000;
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run("speculation", temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        // About 44,000 estimated tokens: inside [threshold - 8,192, threshold).
        let turn = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                "word ".repeat(35_200),
            )
            .await?;
        super::settled(&coordinator, &turn).await?;
        let id = agent.clone();
        let slot = coordinator
            .call(move |s| Ok(s.speculation.contains_key(&id)))
            .await?;
        coordinator.stop_run().await?;
        assert!(!slot);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

/// The footer status follows the background summary through running, armed and idle, and it
/// carries the per-model threshold and the lead below it.
#[tokio::test]
async fn compaction_status_tracks_speculation_and_the_per_model_threshold(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::event::{RuntimeEvent, SpeculationState};
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(Summaries {
        turns: MockProvider::script([answer("first answer"), answer("second answer")]),
        calls: AtomicUsize::new(0),
    });
    let settings = crate::config::load_config_from_str(
        r#"{
            model: 'local/big', provider: { local: {
                type: 'openai_compatible', baseUrl: 'https://example.invalid',
                models: { big: { limit: { context: 100000, output: 8192 } } }
            } }
        }"#,
    )?;
    let target = crate::config::resolve_model_selection(&settings, "local/big", None)?.primary;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.agent_model_targets.insert("default".into(), target);
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.model_ref = "local:big".into();
    config.agent_profiles.insert("default".into(), profile);
    config.compaction.method_order = vec![CompactionMethod::Soft];
    config.compaction.model_thresholds.insert(
        "local/big".into(),
        crate::config::CompactionThreshold::Tokens {
            tokens: std::num::NonZeroU32::new(50_000).ok_or("zero")?,
        },
    );
    config.compaction.keep_recent_tokens = 1_000;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("status", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let store = coordinator.event_store().await?;
    let mut live = store.subscribe_runtime(1)?;
    let (sender, mut statuses) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = live.next().await {
            if let Ok(RuntimeEvent::Live(event)) = event
                && let LiveEventV1::CompactionStatus(status) = event.payload
            {
                let _ = sender.send(status);
            }
        }
    });
    let mut seen = Vec::new();
    // About 44,000 tokens: inside the lead band, so a background summary starts and arms.
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "word ".repeat(35_200),
        )
        .await?;
    super::settled(&coordinator, &turn).await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(status) = statuses.recv().await {
            let armed = status.speculation == SpeculationState::Armed;
            seen.push(status);
            if armed {
                break;
            }
        }
    })
    .await
    .map_err(|_| format!("never armed: {seen:?}"))?;
    // The next prompt crosses the threshold and commits the armed result.
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "next ".repeat(6_400),
        )
        .await?;
    super::settled(&coordinator, &turn).await?;
    coordinator.stop_run().await?;
    drop(run);
    while let Ok(status) = statuses.try_recv() {
        seen.push(status);
    }
    let states: Vec<_> = seen.iter().map(|status| status.speculation).collect();
    let position = |state, from: usize| {
        states[from..]
            .iter()
            .position(|s| *s == state)
            .map(|offset| from + offset)
    };
    let running = position(SpeculationState::Running, 0)
        .ok_or_else(|| format!("never running: {states:?}"))?;
    let armed = position(SpeculationState::Armed, running).ok_or("never armed")?;
    position(SpeculationState::Idle, armed).ok_or("never idle after armed")?;
    assert_eq!(states.first(), Some(&SpeculationState::Idle));
    assert!(seen.iter().all(|status| status.auto
        && status.threshold_tokens == Some(50_000)
        && status.speculation_tokens == Some(50_000 - 8_192)));
    assert!(
        seen.windows(2).all(|pair| pair[0] != pair[1]),
        "unchanged status is not repeated"
    );
    Ok(())
}
