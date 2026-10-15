use crate::coord::{
    compaction_tests::{answer, SUMMARY},
    handle,
    history_tests::settled,
    spawn_coordinator, CoordinatorConfig,
};
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_compaction::CompactionMethod;
use harness_providers::{mock::MockProvider, MessageRole};
use std::sync::Arc;

fn config(
    temp: &std::path::Path,
    provider: &Arc<MockProvider>,
    method: CompactionMethod,
) -> CoordinatorConfig {
    let mut config = CoordinatorConfig::new(temp.join("sessions"));
    config.provider = Arc::<MockProvider>::clone(provider);
    config.compaction.method_order = vec![method];
    config.compaction.async_enabled = false;
    config.compaction.keep_recent_tokens = 100;
    config.compaction.reserve_tokens = Some(0);
    config.compaction.fallback_input_tokens = 32_000;
    config.compaction.threshold_tokens = std::num::NonZeroU32::new(5_000);
    config.compaction.enabled = false;
    config
}

/// Summary-side requests send the live system prompt with the summarization guidance, no
/// tools, no temperature, a unique side session and the main session's cache identity.
#[tokio::test]
async fn side_requests_inherit_the_live_prompt_and_cache_identity(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        answer("old answer"),
        answer("latest answer"),
        answer(SUMMARY),
        answer("A short display summary. "),
    ]));
    let mut config = config(temp.path(), &provider, CompactionMethod::Soft);
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.system_prompt = "You are the test agent.".into();
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("side", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for prompt in ["old request ".repeat(1000), "latest request".into()] {
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, prompt)
            .await?;
        settled(&coordinator, &turn).await?;
    }
    coordinator
        .compact_agent_context(&agent, None, "manual")
        .await?;
    let requests = provider.captured_requests().await;
    let main = &requests[0];
    let main_system = &main.messages[0];
    assert_eq!(main_system.role, MessageRole::System);
    assert!(main_system.content.contains("You are the test agent."));
    let side = &requests[2..];
    assert_eq!(side.len(), 2, "history summary and display summary");
    for request in side {
        let system = &request.messages[0];
        assert!(system.content.starts_with(&main_system.content));
        assert!(system
            .content
            .ends_with(harness_compaction::prompts::summarization_system_prompt()));
        assert!(request.tools.is_none() && request.temperature.is_none());
        assert!(!request.context.main_turn);
        assert_eq!(
            request.context.cache_retention,
            main.context.cache_retention
        );
        assert_eq!(request.context.cache_key, main.context.session_id);
        assert!(request
            .context
            .session_id
            .as_deref()
            .is_some_and(|id| id.contains(":side:")));
    }
    assert_ne!(side[0].context.session_id, side[1].context.session_id);
    // The display summary is kept exactly as generated, trailing whitespace included.
    let events = crate::store::read_events(&run.events_path)?;
    assert!(events.iter().any(|e| matches!(&e.payload,
        crate::event::EventV1::SessionCompaction(c)
            if c.short_summary.as_deref() == Some("A short display summary. "))));
    Ok(())
}

/// Handoff documents are saved only for automatic handoffs with the setting on, as the
/// document plus one newline in a time-stamped file; a manual handoff never saves.
#[tokio::test]
async fn handoff_documents_are_saved_only_for_automatic_passes_when_enabled(
) -> Result<(), Box<dyn std::error::Error>> {
    let document = "My working memory.\nNext Steps: continue.";
    for (automatic, setting) in [(true, true), (true, false), (false, true)] {
        let temp = tempfile::tempdir()?;
        let mut script = vec![
            answer("old answer ".repeat(1000)),
            answer("latest answer"),
            answer(document),
        ];
        if automatic {
            script.push(answer("continued"));
        }
        let provider = Arc::new(MockProvider::script(script));
        let mut config = config(temp.path(), &provider, CompactionMethod::Handoff);
        config.compaction.handoff_save_to_disk = setting;
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run("handoff", temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        for prompt in ["old request ".repeat(1000), "latest request".into()] {
            let turn = coordinator
                .request_agent_turn(handle::system(), &agent, prompt)
                .await?;
            settled(&coordinator, &turn).await?;
        }
        if automatic {
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
        } else {
            coordinator.handoff_agent_context(&agent, None).await?;
        }
        coordinator.stop_run().await?;
        let saved: Vec<_> = std::fs::read_dir(&run.artifacts_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("handoff-"))
            .collect();
        assert_eq!(saved.len(), usize::from(automatic && setting));
        if let Some(file) = saved.first() {
            let name = file.file_name().to_string_lossy().into_owned();
            assert!(name.starts_with("handoff-") && name.ends_with(".md"));
            assert!(!name.trim_end_matches(".md").contains([':', '.']));
            assert_eq!(
                std::fs::read_to_string(file.path())?,
                format!("{document}\n")
            );
        }
    }
    Ok(())
}
