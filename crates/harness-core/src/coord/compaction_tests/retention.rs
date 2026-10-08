use super::*;

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
        assert_eq!(requests[3].max_tokens, Some(8192));
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
            .any(|m| m.content.contains(&"old request ".repeat(1000))));
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
        let summary = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::SessionCompaction(compaction) => Some(&compaction.summary),
                _ => None,
            })
            .ok_or("missing compaction summary")?;
        assert!(summary.contains("## User Requests (verbatim)"));
        assert!(summary.contains("old request "));
        assert!(summary.contains("[truncated]"));
        assert!(summary.contains("latest request"));
        assert!(!summary.contains("## Current Todo List"));
        assert!(requests[4]
            .messages
            .iter()
            .any(|message| message.content == format!("Conversation summary:\n{summary}")));
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
            .any(|m| m.content == format!("Conversation summary:\n{summary}")));
        assert!(requests[5]
            .messages
            .iter()
            .any(|m| m.content == "latest answer"));
        assert!(!requests[5]
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

struct AppendixRedactor;
impl crate::redact::Redactor for AppendixRedactor {
    fn redact_text(&self, text: &str) -> String {
        let text = crate::redact::Redactor::redact_text(&DefaultRedactor::default(), text);
        if text.contains("## User Requests (verbatim)") {
            text.replace("appendix-redaction-sentinel", "[REDACTED]")
        } else {
            text
        }
    }
}

#[tokio::test]
async fn compaction_preserves_bounded_requests_steering_and_current_todos(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut script = vec![answer("old answer ".repeat(3000))];
    script.extend((1..28).map(|_| answer("settled")));
    script.extend([answer(SUMMARY), answer("continued")]);
    let provider = Arc::new(MockProvider::script(script));
    let mut registry = crate::tool::ToolRegistry::new();
    registry.register(Arc::new(CompactionTodos));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.compaction.keep_recent_tokens = 1;
    config.compaction.reserve_tokens = 0;
    config.compaction.suppress_auto_compaction = true;
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["todowrite".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(AppendixRedactor),
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
    assert!(summary.contains(FIRST));
    assert!(summary.contains(RECENT));
    assert!(!summary.contains("obsolete request"));
    assert!(!summary.contains("delivery-only sentinel"));
    assert!(!summary.contains("other-agent sentinel"));
    assert!(!summary.contains("runtime-only sentinel"));
    assert!(!summary.contains("completion-wake sentinel"));
    assert_eq!(summary.matches("[truncated]").count(), 10);
    assert!(!summary.contains(&"λ".repeat(2001)));
    assert!(!summary.contains("appendix-redaction-sentinel"));
    assert!(summary.contains("Credential check: [REDACTED]"));
    let appendix = summary
        .split_once("## User Requests (verbatim)")
        .ok_or("missing requests")?
        .1;
    assert!(appendix.len() < 13 * 1024);
    for item in [
        "[in_progress] Finish the parser",
        "[pending] Add integration coverage",
        "[completed] Choose a format",
    ] {
        assert!(summary.contains(item));
    }
    let turn = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "continue")
        .await?;
    super::history_tests::settled(&coordinator, &turn).await?;
    let requests = provider.captured_requests().await;
    let next = requests
        .last()
        .ok_or("missing provider request after compaction")?;
    assert!(next
        .messages
        .iter()
        .any(|message| message.content == format!("Conversation summary:\n{summary}")));
    coordinator.stop_run().await?;
    Ok(())
}
