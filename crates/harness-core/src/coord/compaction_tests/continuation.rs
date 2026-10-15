use super::maintenance::RoutedSummaries;
use super::*;

/// A post-turn compaction that ends on a plain text answer never reopens the turn, with or
/// without the synthetic continuation setting.
#[tokio::test]
async fn terminal_text_answer_after_post_turn_compaction_does_not_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    for auto_continue in [true, false] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(RoutedSummaries {
            turns: MockProvider::script([
                answer("old answer ".repeat(1000)),
                answer("latest answer"),
                answer("final text answer"),
            ]),
            summaries: MockProvider::script([
                answer("Checkpoint for the rewrite"),
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
        config.compaction.auto_continue = auto_continue;
        config.compaction.enabled = false;
        config.compaction.async_enabled = false;
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator
            .start_run("terminal answer", temp.path())
            .await?;
        let agent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        for text in ["old request ".repeat(1000), "latest request".into()] {
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
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
                .count(),
            1,
            "auto_continue={auto_continue}"
        );
        assert!(
            !events.iter().any(|e| matches!(&e.payload,
                EventV1::RuntimeReminder(r) if r.kind == RuntimeReminderKind::CompactionResume)),
            "auto_continue={auto_continue}"
        );
        assert_eq!(provider.turns.call_count(), 3);
    }
    Ok(())
}
