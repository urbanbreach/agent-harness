use super::*;

#[tokio::test]
async fn compaction_keeps_recent_turns_and_restores_the_same_context_after_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    for child_session in [false, true] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(MockProvider::script([
            answer("old answer ".repeat(1000)),
            // A manual pass records the provider's last reported usage as tokens before.
            vec![
                Stream::TextDelta("latest answer".into()),
                Stream::Done {
                    usage: Some(harness_providers::CompletionUsage {
                        prompt_tokens: 6_000,
                        completion_tokens: 10,
                        total_tokens: 6_010,
                        ..Default::default()
                    }),
                },
            ],
            vec![Stream::Error {
                message: "summary unavailable".into(),
                category: None,
                remediation: None,
                retry_after_ms: None,
                envelope: None,
            }],
            answer(SUMMARY),
            answer("Rewrite checkpoint"),
            answer("after compaction"),
            answer("after resume"),
        ]));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
        config.compaction.keep_recent_tokens = 100;
        config.compaction.reserve_tokens = Some(0);
        config.compaction.enabled = false;
        config.compaction.method_order = vec![CompactionMethod::Soft];
        config.compaction.async_enabled = false;
        let mut registry = crate::tool::ToolRegistry::new();
        registry.register(Arc::new(CompactionTodos));
        config.tool_registry = Arc::new(registry);
        config.permission_policy = PermissionPolicy::allow_all();
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
            let todo = coordinator
                .request_tool_call(
                    EventActor::new(ActorKind::User, None),
                    None,
                    "todowrite",
                    serde_json::json!({"todos": [
                        {"content":"Parent-only todo", "status":"pending"}
                    ]}),
                )
                .await?;
            super::history_tests::settled(&coordinator, &todo).await?;
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
                .await,
            Err(CoordinatorError::CompactionRejected(
                super::super::compaction::ManualRejection::SessionTooSmall
            ))
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
            matches!(outcome, ManualCompactionOutcome::Compacted { tokens_before, tokens_after, .. } if tokens_after < tokens_before),
            "{outcome:?}"
        );
        assert!(std::fs::read(&run.events_path)?.starts_with(&before));
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 5);
        assert!(requests[3].tools.is_none());
        assert!(
            requests[3].attachments.is_empty(),
            "summary generation is text-only"
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
            .any(|m| m.content.contains(&"old request ".repeat(1000))));
        assert!(requests[5].attachments.is_empty());
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
        let summary = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::SessionCompaction(compaction) => Some(&compaction.summary),
                _ => None,
            })
            .ok_or("missing compaction summary")?;
        assert!(requests[5].messages.iter().any(
            |message| message.content == summary_message(Some(CompactionMethod::Soft), summary)
        ));
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
        assert_eq!(
            &requests[6].messages[..requests[5].messages.len()],
            requests[5].messages
        );
        assert!(requests[6]
            .messages
            .iter()
            .any(|m| m.content == summary_message(Some(CompactionMethod::Soft), summary)));
        assert!(requests[6]
            .messages
            .iter()
            .any(|m| m.content == "latest answer"));
        assert!(!requests[6]
            .messages
            .iter()
            .any(|m| m.content.contains(&"old request ".repeat(1000))));
        resumed.stop_run().await?;
    }
    Ok(())
}

struct CompactionTodos;
#[async_trait::async_trait]
impl crate::tool::Tool for CompactionTodos {
    fn id(&self) -> &'static str {
        "todowrite"
    }
    fn parameters_json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    fn capability(&self) -> crate::tool::ToolCapability {
        crate::tool::ToolCapability::ReadFs
    }
    async fn call(
        &self,
        _: crate::tool::ToolContext,
        args: serde_json::Value,
    ) -> Result<ToolResult, crate::tool::ToolError> {
        Ok(ToolResult::structured(args.to_string(), args))
    }
}

#[tokio::test]
async fn compaction_preserves_bounded_requests_steering_and_current_todos(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut script = vec![answer("old answer ".repeat(3000))];
    script.extend((1..28).map(|_| answer("settled")));
    script.push(answer("continued"));
    let provider = Arc::new(super::maintenance::RoutedSummaries {
        turns: MockProvider::script(script),
        summaries: MockProvider::script(std::iter::repeat_n(answer(SUMMARY), 4)),
    });
    let mut registry = crate::tool::ToolRegistry::new();
    registry.register(Arc::new(CompactionTodos));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.compaction.keep_recent_tokens = 100;
    config.compaction.reserve_tokens = Some(0);
    config.compaction.enabled = false;
    config.compaction.method_order = vec![CompactionMethod::Soft];
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["todowrite".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("verbatim", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let todo = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(agent.clone())),
            None,
            "todowrite",
            serde_json::json!({"todos": [
                {"content":"Finish the parser", "status":"in_progress"},
                {"content":"Add integration coverage", "status":"pending"},
                {"content":"Choose a format", "status":"completed"},
                {"content":"Credential check: appendix-redaction-sentinel", "status":"pending"}
            ]}),
        )
        .await?;
    super::history_tests::settled(&coordinator, &todo).await?;
    const FIRST: &str =
        "Keep every user's request exactly as written.\nDo not paraphrase this line.";
    const RECENT: &str = "Steering request: keep the parser streaming, please.";
    for index in 0..13 {
        let text = match index {
            0 => FIRST.into(),
            1 | 2 => format!("obsolete request {index}"),
            _ => format!("long request {index}: {} tail-{index}", "λ".repeat(2200)),
        };
        let turn = coordinator
            .request_agent_turn(EventActor::new(ActorKind::User, None), agent.clone(), text)
            .await?;
        super::history_tests::settled(&coordinator, &turn).await?;
    }
    // Idle steering is submitted as a normal prompt by its caller.
    let turn_id = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            RECENT,
        )
        .await?;
    super::history_tests::settled(&coordinator, &turn_id).await?;
    let delivery = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::Worker, Some(agent.clone())),
            agent.clone(),
            r#"<agent_message sender="child">delivery-only sentinel</agent_message>"#,
        )
        .await?;
    super::history_tests::settled(&coordinator, &delivery).await?;
    for index in 0..12 {
        let wake = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::Worker, Some(agent.clone())),
                agent.clone(),
                format!("<system-reminder>completion-wake sentinel {index}</system-reminder>"),
            )
            .await?;
        super::history_tests::settled(&coordinator, &wake).await?;
    }
    let other = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            other,
            "other-agent sentinel",
        )
        .await?;
    super::history_tests::settled(&coordinator, &turn).await?;
    let (id, request) = (agent.clone(), delivery.clone());
    coordinator
        .call(move |runtime| {
            runtime.emit_reminder(
                &id,
                &request,
                RuntimeReminderKind::TodoContinuation,
                "runtime-only sentinel",
                None,
            )?;
            Ok(())
        })
        .await?;
    let owner = agent.clone();
    let todos_before = coordinator
        .call(move |s| Ok(s.root_todo_items(&owner)))
        .await?;
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
            EventV1::SessionCompaction(compaction) => Some(&compaction.summary),
            _ => None,
        })
        .ok_or("missing compaction summary")?;
    let requests = provider.summaries.captured_requests().await;
    let summaries: Vec<_> = requests
        .iter()
        .filter(|request| {
            request.messages.first().is_some_and(|m| {
                m.content == harness_compaction::prompts::summarization_system_prompt()
            })
        })
        .collect();
    assert!(!summaries.is_empty(), "missing summary requests");
    assert!(summaries
        .iter()
        .any(|r| r.messages[1].content.contains(FIRST)));
    assert!(summaries
        .iter()
        .any(|r| r.messages[1].content.contains(RECENT)));
    for request in summaries {
        let input = &request.messages[1].content;
        assert!(!input.contains("other-agent sentinel"));
        assert!(
            harness_compaction::tokenizer::Counter::byte_estimating(None).count_text(input)
                < harness_compaction::request::summary_input_budget(None, 0)
        );
    }
    let owner = agent.clone();
    let todos_after = coordinator
        .call(move |s| Ok(s.root_todo_items(&owner)))
        .await?;
    assert_eq!(todos_before, todos_after);
    let turn = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "continue")
        .await?;
    super::history_tests::settled(&coordinator, &turn).await?;
    let requests = provider.turns.captured_requests().await;
    let next = requests
        .last()
        .ok_or("missing provider request after compaction")?;
    assert!(next
        .messages
        .iter()
        .any(|message| message.content == summary_message(Some(CompactionMethod::Soft), summary)));
    coordinator.stop_run().await?;
    Ok(())
}
