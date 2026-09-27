use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor, tool::ToolRegistry};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_stream::StreamExt;

pub(super) async fn settled(
    coordinator: &CoordinatorHandle,
    id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(event) if event.task_id.as_str() == id => return Ok(()),
                EventV1::TaskCancelled(event) if event.task_id.as_str() == id => {
                    return Err(EventStoreError::Invalid("turn failed"))
                }
                _ => {}
            }
        }
        Err(EventStoreError::Invalid("turn did not settle"))
    })
    .await??;
    Ok(())
}
#[tokio::test]
async fn resume_and_rewind_keep_history_without_reexecuting_tools(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(super::tests::CountTool(Arc::clone(&calls))));
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "call-1".into(),
                function_name: "count".into(),
                arguments_json: "{}".into(),
            },
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("kept answer".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("discarded answer".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("new answer".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["count".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("history", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let first = coordinator
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "keep prompt",
            crate::file_tag::SelectedPromptTags::default(),
            vec![crate::attachment_transport::AttachmentMetadata::from_bytes(
                "note",
                "text/plain",
                None,
                b"Attachment carried through resume.",
                None,
            )],
            Some("mock:selected".into()),
            Some(AgentModelSettings {
                reasoning_effort: Some("high".into()),
                reasoning_summary: Some("auto".into()),
                ..Default::default()
            }),
        )
        .await?;
    settled(&coordinator, &first).await?;
    let discarded = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "discarded prompt",
        )
        .await?;
    settled(&coordinator, &discarded).await?;
    coordinator.stop_run().await?;
    drop(coordinator);
    let prefix = std::fs::read(&run.events_path)?;
    let resumed = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "history")
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.call_count(), 3);
    assert_eq!(
        resumed
            .rewind_points()
            .await?
            .ok_or("missing rewind points")?
            .len(),
        2
    );
    resumed.rewind_conversation(discarded).await?;
    let replacement = resumed
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "replacement")
        .await?;
    settled(&resumed, &replacement).await?;
    let requests = provider.captured_requests().await;
    let request = requests.last().ok_or("no continuation")?;
    assert_eq!(request.model_id, "selected");
    assert_eq!(request.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(request.reasoning_summary.as_deref(), Some("auto"));
    let history = &request.messages;
    assert!(history.iter().any(|m| m.content == "kept answer"));
    assert!(history.iter().any(|m| m.content == "tool result"));
    assert!(history.iter().all(|m| !m.content.contains("discarded")));
    let attachments: Vec<_> = request.attachments.values().flatten().collect();
    assert_eq!(attachments.len(), 1);
    assert_eq!(
        attachments[0].bytes()?,
        b"Attachment carried through resume."
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    resumed.stop_run().await?;
    let journal = std::fs::read(&run.events_path)?;
    assert!(journal.starts_with(&prefix));
    assert!(!String::from_utf8_lossy(&journal).contains("Attachment carried through resume."));
    crate::session::CanonicalSessionProjection::from_event_history(&crate::store::read_events(
        &run.events_path,
    )?)?;
    let blob = run.artifacts_dir.join(format!(
        "{}.attachment",
        blake3::hash(b"Attachment carried through resume.")
    ));
    let original = std::fs::read(&blob)?;
    std::fs::write(&blob, "tampered")?;
    let corrupt = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    assert!(corrupt
        .resume_run(run.run_id.to_string(), "corrupt")
        .await
        .is_err());
    assert_eq!(std::fs::read(&run.events_path)?, journal);
    std::fs::write(blob, original)?;
    let mut metadata =
        crate::proj::load_run_metadata(&run.run_dir).ok_or("missing run metadata")?;
    metadata.mode_source = Some(crate::proj::SessionModeSource::ReplayOnly);
    crate::store::write_private_atomic(
        &run.run_dir.join("meta.json"),
        &serde_json::to_vec(&metadata)?,
    )?;
    let replay_only = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    assert!(replay_only
        .resume_run(run.run_id.to_string(), "must not resume")
        .await
        .is_err());
    assert_eq!(std::fs::read(&run.events_path)?, journal);
    assert_eq!(provider.call_count(), 4);
    Ok(())
}

#[tokio::test]
async fn split_compaction_retains_complete_tool_pairs_after_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::compaction_tests::{answer, SUMMARY};
    let temp = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(super::tests::CountTool(Arc::clone(&calls))));
    let call = |id: &str, text: String| {
        vec![
            Stream::TextDelta(text),
            Stream::ToolCallComplete {
                tool_call_id: id.into(),
                function_name: "count".into(),
                arguments_json: "{}".into(),
            },
            Stream::Done { usage: None },
        ]
    };
    let provider = Arc::new(MockProvider::script([
        call("old-call", "large earlier message ".repeat(1000)),
        call("kept-call", "recent call".into()),
        answer("done"),
        answer(SUMMARY),
        answer("continued"),
        answer("resumed"),
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    if let Some(profile) = config.agent_profiles.get_mut("default") {
        profile.toolset = vec!["count".into()];
    }
    config.compaction.split_oversized_turns = true;
    config.compaction.keep_recent_tokens = 40;
    config.compaction.suppress_auto_compaction = true;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("split compaction", temp.path())
        .await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "work",
        )
        .await?;
    settled(&coordinator, &turn).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(matches!(
        coordinator
            .compact_agent_context(agent.clone(), None, "manual")
            .await?,
        ManualCompactionOutcome::Compacted { .. }
    ));
    let next = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "continue",
        )
        .await?;
    settled(&coordinator, &next).await?;
    let requests = provider.captured_requests().await;
    let kept = &requests[4].messages;
    assert_eq!(kept.len(), 5);
    assert!(kept[0].content.contains(SUMMARY));
    assert_eq!(
        kept[1]
            .assistant_tool_calls
            .as_ref()
            .and_then(|c| c.first())
            .map(|c| c.tool_call_id.as_str()),
        Some("kept-call")
    );
    assert_eq!(kept[2].tool_call_id.as_deref(), Some("kept-call"));
    assert_eq!(kept[3].content, "done");
    coordinator.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "resumed")
        .await?;
    let next = resumed
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "resume")
        .await?;
    settled(&resumed, &next).await?;
    let requests = provider.captured_requests().await;
    assert_eq!(&requests[5].messages[..5], kept);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    resumed.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn inspection_and_catalog_are_read_only_and_reject_corrupt_history(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.session_mode_source = Some(crate::proj::SessionModeSource::InteractiveMock);
    let provider = Arc::new(MockProvider::default());
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("original", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "prompt",
        )
        .await?;
    settled(&coordinator, &turn).await?;
    assert!(!crate::proj::inspect_resume_plan(&run.run_dir).is_resumable);
    coordinator.update_session_title("renamed").await?;
    let observer = coordinator.event_store().await?;
    coordinator.stop_run().await?;
    assert!(observer.replay(1)?.next().await.is_some());
    let before = std::fs::read(&run.events_path)?;
    let plan = crate::proj::inspect_resume_plan(&run.run_dir);
    assert!(plan.is_resumable, "{:?}", plan.resume_disabled_reason);
    assert_eq!(plan.provider_model.as_deref(), Some("mock/default"));
    assert_eq!(
        plan.known_agents.get(&agent).map(String::as_str),
        Some("default")
    );
    let events = crate::store::read_events(&run.events_path)?;
    let metadata = crate::proj::read_run_metadata(&run.run_dir)?.ok_or("metadata missing")?;
    let catalog = crate::proj::project_session_catalog_entry(
        &events,
        run.run_id.as_str(),
        Some(&crate::proj::SessionCatalogMetadata::from(&metadata)),
        None,
        None,
    )?;
    assert_eq!(catalog.run_name.as_deref(), Some("renamed"));
    assert_eq!(catalog.status, Some(crate::proj::RunStatus::Finished));
    assert!(catalog.is_resumable);
    assert_eq!(
        catalog.mode_source,
        crate::proj::SessionModeSource::InteractiveMock
    );
    assert_eq!(std::fs::read(&run.events_path)?, before);
    assert_eq!(provider.call_count(), 1);
    for change in 0..3 {
        let mut malformed = events.clone();
        match change {
            0 => malformed[1].seq += 1,
            1 => malformed[1].run_id = "other-run".into(),
            _ => malformed[1].event_id = malformed[0].event_id.clone(),
        }
        assert!(crate::proj::project_run_summary(&malformed).is_err());
        assert!(crate::proj::project_resume_plan(&malformed, run.run_id.as_str()).is_err());
    }
    let mut replay_metadata = metadata;
    replay_metadata.mode_source = Some(crate::proj::SessionModeSource::ReplayOnly);
    crate::store::write_private_atomic(
        &run.run_dir.join(crate::proj::META_FILE_NAME),
        &serde_json::to_vec(&replay_metadata)?,
    )?;
    assert!(!crate::proj::inspect_resume_plan(&run.run_dir).is_resumable);
    std::fs::write(&run.events_path, b"{broken\n")?;
    let rejected = crate::proj::inspect_resume_plan(&run.run_dir);
    assert!(!rejected.is_resumable);
    assert!(rejected.resume_disabled_reason.is_some());
    assert_eq!(std::fs::read(&run.events_path)?, b"{broken\n");
    assert_eq!(provider.call_count(), 1);
    let absent = temp.path().join("absent");
    assert!(!crate::proj::inspect_resume_plan(&absent).is_resumable);
    assert!(!absent.exists());
    Ok(())
}
