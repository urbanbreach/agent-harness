use super::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    subagent::*,
    tool::{Tool, ToolCapability, ToolContext, ToolError},
};
use harness_providers::{
    mock::MockProvider, CompletionUsage, ProviderStreamEvent as Stream,
    ProviderStreamFinishedMetadata,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_stream::StreamExt;

mod cancellation;
mod delegation;
mod fixtures;
#[macro_use]
mod finalized_checks;
mod history;
mod native_resume;
mod storage;
use fixtures::*;

#[tokio::test]
async fn actual_finalized_state_survives_presentation_bounding_copy_and_readonly_reload(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("sample.txt"), "sample")?;
    let arguments = "{ \"path\" : \"sample.txt\", \"value\" : \"call-a\" }";
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "call-a".into(),
                function_name: "large".into(),
                arguments_json: arguments.into(),
            },
            settled_metadata("settled-first"),
        ],
        vec![
            Stream::TextDelta("final answer".into()),
            settled_metadata("settled-second"),
        ],
        vec![
            Stream::TextDelta("resumed answer".into()),
            settled_metadata("settled-third"),
        ],
        vec![Stream::error("actual provider failure")],
        vec![settled_metadata("settled-empty")],
        vec![
            Stream::TextDelta("awake answer".into()),
            settled_metadata("settled-wake"),
        ],
        vec![
            Stream::TextDelta("parent settled answer".into()),
            settled_metadata("parent-settled"),
        ],
        vec![
            Stream::TextDelta("forked answer".into()),
            settled_metadata("fork-settled"),
        ],
        vec![Stream::error("root dispatched failure")],
        vec![settled_metadata("root empty")],
        vec![
            Stream::TextDelta("reloaded answer".into()),
            settled_metadata("reload"),
        ],
        vec![
            Stream::TextDelta("reloaded child answer".into()),
            settled_metadata("child reload"),
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.permission_policy = PermissionPolicy::allow_all();
    let mut registry = ToolRegistry::new();
    let tool_calls = Arc::new(AtomicUsize::new(0));
    registry.register(Arc::new(LargeTool(Arc::clone(&tool_calls))));
    config.tool_registry = Arc::new(registry);
    let mut profile = AgentProfile::fallback("child");
    profile.toolset = vec!["large".into()];
    profile.system_prompt = "current child definition".into();
    config.agent_profiles.insert("child".into(), profile);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("finalized", temp.path()).await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let source = coordinator
        .spawn_agent(system_actor(), "child", Some(parent.clone()))
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let source_prompt = format!("source prompt literal identity: {source}");
    let attempt = coordinator
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            system_actor(),
            source.clone(),
            source_prompt.clone(),
            crate::file_tag::SelectedPromptTags::default(),
            vec![crate::attachment_transport::AttachmentMetadata::from_bytes(
                "note",
                "text/plain",
                None,
                b"faithful attachment",
                None,
            )],
            None,
            None,
        )
        .await?;
    wait_terminal(&mut events, &attempt).await?;
    assert_prompt_turns(&coordinator, &source, 1).await?;
    let FinalizedStateResult::Available {
        state: source_state,
    } = coordinator.raw_finalized_state(source.clone()).await?
    else {
        return Err("source state unavailable".into());
    };
    assert_raw_source_integrity!(source_state, arguments, temp.path());
    let target = coordinator
        .spawn_agent(system_actor(), "child", Some(parent.clone()))
        .await?;
    assert_ne!(target, source);
    coordinator
        .initialize_agent_from_finalized(
            system_actor(),
            target.clone(),
            source.clone(),
            FinalizedContextCopy::Resume,
        )
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let resumed = coordinator
        .request_agent_turn(system_actor(), target.clone(), "new prompt")
        .await?;
    wait_terminal(&mut events, &resumed).await?;
    assert_prompt_turns(&coordinator, &target, 1).await?;
    let requests = provider.captured_requests().await;
    let inherited = requests.last().ok_or("resumed request missing")?;
    assert_eq!(inherited.messages[0].content, "current child definition");
    assert!(inherited
        .messages
        .iter()
        .any(|m| m.content == source_prompt));
    assert!(inherited
        .messages
        .iter()
        .any(|m| m.content == "final answer"));
    assert!(inherited.messages.iter().any(|m| m
        .assistant_tool_calls
        .as_ref()
        .is_some_and(|c| c[0].arguments_json == arguments)));
    assert_eq!(
        coordinator.raw_finalized_state(source.clone()).await?,
        FinalizedStateResult::Available {
            state: source_state.clone()
        }
    );
    let mut events = coordinator.subscribe_new_events().await?;
    let failed = coordinator
        .request_agent_turn(system_actor(), source.clone(), "failed followup")
        .await?;
    wait_cancelled(&mut events, &failed).await?;
    assert_eq!(
        coordinator.raw_finalized_state(source.clone()).await?,
        FinalizedStateResult::Available {
            state: source_state.clone()
        }
    );
    let mut events = coordinator.subscribe_new_events().await?;
    let empty = coordinator
        .request_agent_turn(system_actor(), source.clone(), "empty followup")
        .await?;
    wait_terminal(&mut events, &empty).await?;
    assert_eq!(
        coordinator.raw_finalized_state(source.clone()).await?,
        FinalizedStateResult::Available {
            state: source_state.clone()
        }
    );
    let history = coordinator.subagent_history().await?;
    assert!(history.records[&source].lifecycle.is_finished());
    let reference = history.finalized[&source].clone();
    let artifact = run.run_dir.join(reference.artifact_path());
    let original_bytes = std::fs::read(&artifact)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&artifact)?.permissions().mode() & 0o777,
            0o600
        );
    }
    let journal = crate::store::read_events(&run.events_path)?;
    assert!(!std::fs::read_to_string(&run.events_path)?.contains("settled-first"));
    let calls = provider.call_count();
    let names_before: Vec<_> = std::fs::read_dir(&run.artifacts_dir)?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<Result<_, _>>()?;
    let decoded = SubagentHistory::from_events(&journal);
    assert!(decoded.records[&source].lifecycle.is_finished());
    assert_eq!(
        resolve_finalized_state(
            &run.run_dir,
            &reference,
            run.run_id.as_str(),
            &SubagentId(source.clone()),
            &attempt
        ),
        FinalizedStateResult::Available {
            state: source_state
        }
    );
    assert_eq!(provider.call_count(), calls);
    assert_eq!(std::fs::read(&artifact)?, original_bytes);
    assert_eq!(
        std::fs::read_dir(&run.artifacts_dir)?.count(),
        names_before.len()
    );
    coordinator
        .retire_subagent_attempt(system_actor(), source.clone())
        .await?;
    assert!(coordinator.subagent_history().await?.records[&source].retired);
    coordinator
        .initialize_agent_from_finalized(
            system_actor(),
            source.clone(),
            source.clone(),
            FinalizedContextCopy::Wake,
        )
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let wake = coordinator
        .request_agent_turn(system_actor(), source.clone(), "wake prompt")
        .await?;
    wait_terminal(&mut events, &wake).await?;
    assert_prompt_turns(&coordinator, &source, 4).await?;
    let FinalizedStateResult::Available { state: awake } =
        coordinator.raw_finalized_state(source.clone()).await?
    else {
        return Err("wake state missing".into());
    };
    assert_eq!(awake.owner_agent_id.0, source);
    assert_eq!(awake.attempt_id, wake);
    assert_eq!(awake.source_reference.as_deref(), Some(&reference));
    assert!(!coordinator.subagent_history().await?.records[&source].retired);
    assert_eq!(std::fs::read(&artifact)?, original_bytes);
    let mut events = coordinator.subscribe_new_events().await?;
    let parent_turn = coordinator
        .request_agent_turn(system_actor(), parent.clone(), "parent logical source")
        .await?;
    wait_terminal(&mut events, &parent_turn).await?;
    let fork = coordinator
        .spawn_agent(system_actor(), "child", Some(parent.clone()))
        .await?;
    coordinator
        .initialize_agent_from_finalized(
            system_actor(),
            fork.clone(),
            parent.clone(),
            FinalizedContextCopy::Fork,
        )
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let fork_turn = coordinator
        .request_agent_turn(system_actor(), fork.clone(), "fork prompt")
        .await?;
    wait_terminal(&mut events, &fork_turn).await?;
    let FinalizedStateResult::Available { state: forked } =
        coordinator.raw_finalized_state(fork).await?
    else {
        return Err("forked state missing".into());
    };
    assert!(forked
        .conversation_items
        .iter()
        .any(|item| item.message.content == "parent logical source"));
    assert!(forked
        .conversation_items
        .iter()
        .any(|item| item.message.content == "parent settled answer"));
    assert!(
        forked.read_state.is_empty(),
        "fork copies the parent owner's reads, not another child's reads"
    );
    for (prompt, empty) in [("root failed suffix", false), ("root empty suffix", true)] {
        let mut events = coordinator.subscribe_new_events().await?;
        let id = coordinator
            .request_agent_turn(system_actor(), parent.clone(), prompt)
            .await?;
        if empty {
            wait_terminal(&mut events, &id).await?;
        } else {
            wait_cancelled(&mut events, &id).await?;
        }
    }
    let expected_root = coordinator
        .call({
            let parent = parent.clone();
            move |s| Ok(s.agents[&parent].messages.messages())
        })
        .await?;
    let calls = provider.call_count();
    coordinator.stop_run().await?;
    drop(coordinator);
    let restarted = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    restarted
        .resume_run(run.run_id.to_string(), "reload")
        .await?;
    assert_eq!(provider.call_count(), calls);
    assert!(matches!(
        restarted.raw_finalized_state(target.clone()).await?,
        FinalizedStateResult::Available { .. }
    ));
    assert!(
        matches!(
            restarted.raw_finalized_state(parent.clone()).await?,
            FinalizedStateResult::Available { .. }
        ),
        "failed and empty suffixes preserve the reusable completed root state"
    );
    let mut events = restarted.subscribe_new_events().await?;
    let id = restarted
        .request_agent_turn(system_actor(), parent.clone(), "reload suffix")
        .await?;
    wait_terminal(&mut events, &id).await?;
    let requests = provider.captured_requests().await;
    let input = &requests.last().ok_or("reload input missing")?.messages;
    assert_eq!(
        &input[..input.len() - 1],
        expected_root.as_slice(),
        "reusable state must not rewind the root's current conversation"
    );
    let mut events = restarted.subscribe_new_events().await?;
    let child_turn = restarted
        .request_agent_turn(system_actor(), source.clone(), "reload child suffix")
        .await?;
    wait_terminal(&mut events, &child_turn).await?;
    assert_prompt_turns(&restarted, &source, 5).await?;
    restarted.stop_run().await?;
    let calls = calls + 2;
    // Direct continuation of an initialized child projection validates its original owner.
    let projected = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    projected
        .resume_run(target.clone(), "direct initialized child")
        .await?;
    assert!(matches!(
        projected.raw_finalized_state(target.clone()).await?,
        FinalizedStateResult::Available { .. }
    ));
    assert_eq!(provider.call_count(), calls);
    projected.stop_run().await?;
    let source_journal = std::fs::read(&run.events_path)?;
    let source_events = crate::store::read_events(&run.events_path)?;
    let source_artifacts: BTreeMap<_, _> = std::fs::read_dir(&run.artifacts_dir)?
        .map(|entry| {
            let entry = entry?;
            Ok((entry.file_name(), std::fs::read(entry.path())?))
        })
        .collect::<Result<_, std::io::Error>>()?;
    use crate::session_lineage::*;
    for (name, source_kind) in [
        (
            "state-clone",
            ChildSessionMaterializationSourceKind::DiskRunDirectory,
        ),
        (
            "state-fork",
            ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
        ),
    ] {
        let stable = if source_kind == ChildSessionMaterializationSourceKind::DiskRunDirectory {
            latest_clone_stable_prefix(&source_events)?
        } else {
            validate_tui_fork_stable_prefix(&source_events, source_events.len() as u64)?
        };
        let materialized = materialize_child_session_as(
            ChildSessionMaterializationRequest {
                source_run_dir: &run.run_dir,
                events: &source_events,
                stable_prefix: &stable,
                source_kind,
            },
            Some(name),
        )?;
        let destination_events =
            crate::store::read_events(&materialized.child_run_dir.join("events.jsonl"))?;
        let target_id = destination_events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::AgentContextInitialized(initialized)
                    if initialized.mode == FinalizedContextCopy::Resume =>
                {
                    Some(initialized.agent_id.0.clone())
                }
                _ => None,
            })
            .ok_or("materialized resume initializer missing")?;
        assert_ne!(target_id, target);
        let destination = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        destination
            .resume_run(name, "materialized continuation")
            .await?;
        let FinalizedStateResult::Available { state } =
            destination.raw_finalized_state(target_id).await?
        else {
            return Err("materialized exact child state unavailable".into());
        };
        assert_eq!(state.owner_run_id, name);
        assert!(
            state
                .conversation_items
                .iter()
                .any(|item| item.message.content == source_prompt),
            "literal content identities must not be remapped"
        );
        assert_eq!(
            state.conversation_items[0].attachments[0].bytes()?,
            b"faithful attachment"
        );
        let result = state
            .conversation_items
            .iter()
            .find_map(|item| item.raw_tool_result.as_ref())
            .and_then(|result| result.output.as_ref())
            .ok_or("materialized raw result missing")?;
        assert_eq!(
            result.structured_json.as_ref().ok_or("raw data missing")?["session_id"],
            source
        );
        assert_eq!(
            destination.raw_finalized_state(parent.clone()).await?,
            FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::LegacySummaryOnly
            },
            "later summary-only root history cannot become an exact branch source"
        );
        assert_eq!(provider.call_count(), calls);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 1);
        destination.stop_run().await?;
        assert_eq!(std::fs::read(&run.events_path)?, source_journal);
        for (path, bytes) in &source_artifacts {
            assert_eq!(&std::fs::read(run.artifacts_dir.join(path))?, bytes);
        }
    }
    assert_finalized_reference_rejections!(
        &run.run_dir,
        run.run_id.as_str(),
        source,
        &attempt,
        &reference,
        &artifact
    );
    Ok(())
}
