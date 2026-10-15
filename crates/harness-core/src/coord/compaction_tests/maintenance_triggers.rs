use super::maintenance::RoutedSummaries;
use super::*;
use harness_compaction::{shake::summarize_shake, ShakeMode};

#[tokio::test]
async fn manual_shake_journals_each_mode_and_restores_the_edited_context(
) -> Result<(), Box<dyn std::error::Error>> {
    for mode in [ShakeMode::Elide, ShakeMode::Images, ShakeMode::Thinking] {
        let temp = tempfile::tempdir()?;
        let old = format!(
            "before\n```text\n{}\n```\nafter",
            "heavy block ".repeat(700)
        );
        let provider = Arc::new(MockProvider::script([
            vec![
                Stream::TextDelta(old.clone()),
                Stream::DoneWithMetadata {
                    usage: None,
                    metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
                        settled_reasoning: Some(vec!["settled reasoning".into()]),
                        ..Default::default()
                    }),
                },
            ],
            answer("recent protected answer ".repeat(1_500)),
            answer("continued"),
            answer("resumed"),
        ]));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.compaction.enabled = false;
        let mut profile = crate::agent::AgentProfile::fallback("default");
        profile.model_ref = "mock:gpt-4.1".into();
        config.agent_profiles.insert("default".into(), profile);
        let coordinator = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run("shake modes", temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        let turn = coordinator
            .request_agent_turn_with_model_and_selected_tags_and_attachments(
                handle::system(),
                &agent,
                "original request",
                crate::file_tag::SelectedPromptTags::default(),
                vec![crate::attachment_transport::AttachmentMetadata::from_bytes(
                    "image",
                    "image/png",
                    None,
                    &base64::Engine::decode(&base64::engine::general_purpose::STANDARD, "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC")?,
                    None,
                )],
                None,
                None,
            )
            .await?;
        settled(&coordinator, &turn).await?;
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "recent request")
            .await?;
        settled(&coordinator, &turn).await?;
        let outcome = coordinator.shake_agent_context(&agent, mode).await?;
        let (blocks, images, thinking, kind) = match mode {
            ShakeMode::Elide => (1, 0, 0, crate::event::PruneKindRecord::Shake),
            ShakeMode::Images => (0, 1, 0, crate::event::PruneKindRecord::Images),
            ShakeMode::Thinking => (0, 0, 1, crate::event::PruneKindRecord::Thinking),
        };
        assert_eq!(
            outcome.summary,
            summarize_shake(mode, 0, blocks, images, thinking, outcome.tokens_freed)
        );
        let events = crate::store::read_events(&run.events_path)?;
        assert!(events.iter().any(|e| matches!(&e.payload, EventV1::ContextPruned(p) if p.kind == kind && p.edits.len() == 1)));
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "continue")
            .await?;
        settled(&coordinator, &turn).await?;
        let requests = provider.captured_requests().await;
        let kept = requests[2].messages.clone();
        if mode == ShakeMode::Elide {
            assert!(!kept.iter().any(|m| m.content == old));
        }
        if mode == ShakeMode::Images {
            assert!(requests[2].attachments.is_empty());
        }
        coordinator.stop_run().await?;
        let resumed = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        resumed
            .resume_run(run.run_id.to_string(), "shake resumed")
            .await?;
        let turn = resumed
            .request_agent_turn(handle::system(), &agent, "resume")
            .await?;
        settled(&resumed, &turn).await?;
        let requests = provider.captured_requests().await;
        assert_eq!(&requests[3].messages[..kept.len()], kept);
        if mode == ShakeMode::Images {
            assert!(requests[3].attachments.is_empty());
        }
        resumed.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn single_long_turn_compacts_at_a_tool_boundary_before_the_input_budget_fails(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let call = |id: &str| {
        vec![
            Stream::TextDelta("large assistant context ".repeat(1_000)),
            Stream::ToolCallComplete {
                tool_call_id: id.into(),
                function_name: "count".into(),
                arguments_json: "{}".into(),
            },
            Stream::Done { usage: None },
        ]
    };
    let provider = Arc::new(RoutedSummaries {
        turns: MockProvider::script([call("first"), call("second"), answer("long turn completed")]),
        summaries: MockProvider::script([
            answer("Earlier steps are complete."),
            answer("Long-turn checkpoint"),
        ]),
    });
    let settings = crate::config::load_config_from_str(
        r#"{ model: 'local/small', provider: { local: {
        type: 'openai_compatible', baseUrl: 'https://example.invalid',
        models: { small: { limit: { context: 16000, output: 6000 } } }
    } } }"#,
    )?;
    let target = crate::config::resolve_model_selection(&settings, "local/small", None)?.primary;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.agent_model_targets.insert("default".into(), target);
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.method_order = vec![CompactionMethod::Soft];
    config.compaction.keep_recent_tokens = 1;
    config.compaction.reserve_tokens = Some(1_000);
    config.compaction.threshold_tokens = std::num::NonZeroU32::new(10_000);
    config.compaction.async_enabled = false;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = crate::tool::ToolRegistry::new();
    registry.register(Arc::new(super::super::tests::CountTool(Arc::clone(&calls))));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["count".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("mid-turn compaction", temp.path())
        .await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    let turn = coordinator
        .request_agent_turn(handle::system(), &agent, "perform both steps")
        .await?;
    settled(&coordinator, &turn).await?;
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    let requests = provider.turns.captured_requests().await;
    assert_eq!(requests.len(), 3);
    assert!(requests[2]
        .messages
        .iter()
        .any(|m| m.content.contains("Earlier steps are complete.")));
    assert!(!requests[2].messages.iter().any(|m| m
        .assistant_tool_calls
        .as_ref()
        .is_some_and(|calls| calls.iter().any(|c| c.tool_call_id == "first"))));
    assert!(requests[2]
        .messages
        .iter()
        .any(|m| m.tool_call_id.as_deref() == Some("second")));
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    assert!(events.iter().any(|e| matches!(&e.payload, EventV1::SessionCompaction(c) if c.trigger_reason == "threshold" && c.method == Some(CompactionMethod::Soft))));
    Ok(())
}

#[tokio::test]
async fn default_methods_use_handoff_for_post_turn_and_idle_but_soft_for_manual_compact(
) -> Result<(), Box<dyn std::error::Error>> {
    for mode in ["post_turn", "idle", "manual"] {
        let temp = tempfile::tempdir()?;
        let document = "Working memory for the next instance.";
        let mut script = vec![answer("old answer ".repeat(1_000))];
        script.push(answer(if mode == "post_turn" {
            "recent answer ".repeat(1_000)
        } else {
            "recent answer".into()
        }));
        script.push(answer(document));
        if mode == "manual" {
            script.push(answer("Rewrite checkpoint"));
        }
        script.push(answer("continued"));
        let provider = Arc::new(MockProvider::script(script));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.compaction.enabled = false;
        config.compaction.async_enabled = false;
        config.compaction.keep_recent_tokens = 100;
        config.compaction.reserve_tokens = Some(0);
        config.compaction.threshold_tokens = std::num::NonZeroU32::new(5_000);
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run(mode, temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(handle::system(), "default", None)
            .await?;
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "old request")
            .await?;
        settled(&coordinator, &turn).await?;
        if mode == "post_turn" {
            coordinator
                .call(|s| {
                    s.config.compaction.enabled = true;
                    Ok(())
                })
                .await?;
        }
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "recent request")
            .await?;
        settled(&coordinator, &turn).await?;
        if mode == "idle" {
            coordinator.idle_compact_agent_context(&agent).await?;
        }
        if mode == "manual" {
            coordinator
                .compact_agent_context(&agent, None, "manual")
                .await?;
        }
        let expected = if mode == "manual" {
            CompactionMethod::Soft
        } else {
            CompactionMethod::Handoff
        };
        let events = crate::store::read_events(&run.events_path)?;
        let compaction = events
            .iter()
            .find_map(|e| match &e.payload {
                EventV1::SessionCompaction(c) => Some(c),
                _ => None,
            })
            .ok_or("missing default-method compaction")?;
        assert_eq!(compaction.method, Some(expected), "{mode}");
        assert_eq!(
            compaction.trigger_reason,
            if mode == "post_turn" {
                "threshold"
            } else {
                mode
            }
        );
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, "continue")
            .await?;
        settled(&coordinator, &turn).await?;
        let requests = provider.captured_requests().await;
        assert!(requests
            .last()
            .ok_or("missing continuation")?
            .messages
            .iter()
            .any(|m| m.content == summary_message(Some(expected), &compaction.summary)));
        coordinator.stop_run().await?;
    }
    Ok(())
}
