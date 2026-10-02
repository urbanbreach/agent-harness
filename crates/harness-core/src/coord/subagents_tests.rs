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

fn settled_metadata(reasoning: &str) -> Stream {
    Stream::DoneWithMetadata {
        usage: Some(CompletionUsage {
            prompt_tokens: 12,
            completion_tokens: 3,
            total_tokens: 15,
        }),
        metadata: Some(ProviderStreamFinishedMetadata {
            settled_reasoning: Some(vec![reasoning.into()]),
            usage_complete: Some(true),
            ..Default::default()
        }),
    }
}

struct LargeTool(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl Tool for LargeTool {
    fn id(&self) -> &str {
        "large"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn call(&self, ctx: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let path = ctx.resolve_workspace_path(std::path::Path::new("sample.txt"))?;
        ctx.tool_state.record_read(
            &path,
            blake3::hash(&std::fs::read(&path)?).to_hex().to_string(),
        )?;
        Ok(ToolResult::structured(
            "raw-output-marker\n".repeat(4000),
            json!({"session_id":ctx.actor.agent_id}),
        ))
    }
}

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
    let FinalizedStateResult::Available {
        state: source_state,
    } = coordinator.raw_finalized_state(source.clone()).await?
    else {
        return Err("source state unavailable".into());
    };
    assert_eq!(source_state.conversation_items.len(), 4);
    assert_eq!(
        source_state.conversation_items[0].attachments[0].bytes()?,
        b"faithful attachment"
    );
    assert_eq!(
        source_state.conversation_items[1]
            .message
            .assistant_tool_calls
            .as_ref()
            .ok_or("calls missing")?[0]
            .arguments_json,
        arguments
    );
    let raw = source_state.conversation_items[2]
        .raw_tool_result
        .as_ref()
        .ok_or("raw result missing")?;
    assert_eq!(raw.provider_tool_call_id.as_deref(), Some("call-a"));
    assert_ne!(raw.tool_call_id, "call-a");
    assert_eq!(
        raw.output
            .as_ref()
            .ok_or("raw output missing")?
            .display_text
            .len(),
        "raw-output-marker\n".len() * 4000
    );
    assert!(source_state.conversation_items[2]
        .message
        .content
        .contains("Output shortened"));
    assert_eq!(
        source_state.usage[0]
            .settled_reasoning
            .as_ref()
            .ok_or("reasoning missing")?[0],
        "settled-first"
    );
    assert_eq!(
        source_state
            .read_state
            .get(&temp.path().join("sample.txt"))
            .map(String::as_str),
        Some(blake3::hash(b"sample").to_hex().as_str())
    );
    assert!(source_state
        .model_request
        .as_ref()
        .ok_or("last logical input missing")?
        .messages
        .iter()
        .all(|m| m.role != harness_providers::MessageRole::System));
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
    restarted.stop_run().await?;
    let calls = calls + 1;
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
    // Digest, owner, version, missing and malformed bytes are independently rejected.
    for (change, reason) in [
        (0, FinalizedStateUnavailable::OwnerMismatch),
        (1, FinalizedStateUnavailable::UnsupportedVersion),
        (2, FinalizedStateUnavailable::Corrupt),
    ] {
        let mut changed = reference.clone();
        match change {
            0 => changed.state.owner = SubagentId("foreign".into()),
            1 => changed.payload_version = 2,
            _ => changed.byte_length += 1,
        }
        assert_eq!(
            resolve_finalized_state(
                &run.run_dir,
                &changed,
                run.run_id.as_str(),
                &SubagentId(source.clone()),
                &attempt
            ),
            FinalizedStateResult::Unavailable { reason }
        );
    }
    std::fs::write(&artifact, b"{}")?;
    assert_eq!(
        resolve_finalized_state(
            &run.run_dir,
            &reference,
            run.run_id.as_str(),
            &SubagentId(source.clone()),
            &attempt
        ),
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Corrupt
        }
    );
    std::fs::remove_file(&artifact)?;
    assert_eq!(
        resolve_finalized_state(
            &run.run_dir,
            &reference,
            run.run_id.as_str(),
            &SubagentId(source),
            &attempt
        ),
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Missing
        }
    );
    Ok(())
}

fn system_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, None)
}

async fn wait_terminal(
    events: &mut crate::store::EventStream,
    attempt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(e) if e.task_id.as_str() == attempt => return Ok(()),
                EventV1::TaskCancelled(e) if e.task_id.as_str() == attempt => {
                    return Err(EventStoreError::Invalid("turn failed"))
                }
                _ => {}
            }
        }
        Err(EventStoreError::Invalid("missing terminal"))
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn finalized_reference_append_failure_never_publishes_available_state(
) -> Result<(), Box<dyn std::error::Error>> {
    for artifact_failure in [true, false] {
        assert_finalized_storage_failure(artifact_failure).await?;
    }
    Ok(())
}
async fn assert_finalized_storage_failure(
    artifact_failure: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("complete".into()),
        settled_metadata("settled"),
    ]]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("append failure", temp.path()).await?;
    let agent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    coordinator
        .call(move |s| {
            let inner = s.store.clone().ok_or(CoordinatorError::RunNotStarted)?;
            let artifacts = s.info()?.artifacts_dir.clone();
            s.store = Some(Arc::new(super::tests::InterceptStore {
                inner,
                before_append: Box::new(move |event| {
                    if artifact_failure
                        && matches!(event.payload, EventV1::ProviderRequestStarted(_))
                    {
                        obstruct_artifact_directory(&artifacts)?;
                    }
                    if !artifact_failure && matches!(event.payload, EventV1::FinalizedAgentState(_))
                    {
                        Err(EventStoreError::Io(std::io::Error::other(
                            "finalized append failure",
                        )))
                    } else {
                        Ok(())
                    }
                }),
            }));
            Ok(())
        })
        .await?;
    let mut events = coordinator.event_store().await?.subscribe_runtime(1)?;
    coordinator
        .request_agent_turn(system_actor(), agent.clone(), "complete")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if matches!(event?, RuntimeEvent::Live(e) if matches!(e.payload, LiveEventV1::RuntimeWarning { .. })) { return Ok::<_, EventStoreError>(()); }
        }
        Err(EventStoreError::Invalid("failure not published"))
    }).await??;
    assert!(matches!(
        coordinator.raw_finalized_state(agent.clone()).await?,
        FinalizedStateResult::Unavailable { .. }
    ));
    assert!(coordinator.subagent_history().await?.finalized.is_empty());
    assert!(coordinator
        .request_agent_turn(system_actor(), agent.clone(), "must reject")
        .await
        .is_err());
    coordinator
        .call(move |s| {
            assert!(
                !s.agents[&agent].busy,
                "removed completion cannot strand a busy actor"
            );
            assert!(s.running.is_empty());
            assert!(s.fault.is_some());
            Ok(())
        })
        .await?;
    let journal = crate::store::read_events(&run.events_path)?;
    assert!(journal
        .iter()
        .all(|e| !matches!(e.payload, EventV1::FinalizedAgentState(_))));
    assert_eq!(
        run.artifacts_dir.is_dir(),
        !artifact_failure,
        "an orphan blob is not authority and failed artifact IO is fail-closed"
    );
    assert!(coordinator.stop_run().await.is_err());
    Ok(())
}
fn obstruct_artifact_directory(path: &std::path::Path) -> Result<(), EventStoreError> {
    if path.is_dir() {
        std::fs::remove_dir(path)?;
    }
    std::fs::write(path, b"not a directory")?;
    Ok(())
}

#[tokio::test]
async fn missing_normalized_fields_and_policy_modified_state_are_not_exact(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([
        vec![
            Stream::TextDelta("answer".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("sk-policy-rejected-secret-value".into()),
            settled_metadata("safe"),
        ],
    ]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("fidelity", temp.path()).await?;
    let first = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let second = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    for (agent, reason) in [
        (first, FinalizedStateUnavailable::UnsupportedReasoning),
        (second, FinalizedStateUnavailable::PolicyModified),
    ] {
        let mut events = coordinator.subscribe_new_events().await?;
        let id = coordinator
            .request_agent_turn(system_actor(), agent.clone(), "prompt")
            .await?;
        wait_terminal(&mut events, &id).await?;
        assert_eq!(
            coordinator.raw_finalized_state(agent).await?,
            FinalizedStateResult::Unavailable { reason }
        );
    }
    assert!(!std::fs::read_to_string(&run.events_path)?.contains("provider_reasoning_delta"));
    for entry in std::fs::read_dir(&run.artifacts_dir)? {
        assert!(
            !std::fs::read_to_string(entry?.path())?.contains("sk-policy-rejected-secret-value")
        );
    }
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn accepted_context_is_applied_before_projection_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    for initialize in [false, true] {
        assert_context_projection_failure(initialize).await?;
    }
    Ok(())
}
async fn assert_context_projection_failure(
    initialize: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let cwd = temp.path().join("effective");
    std::fs::create_dir(&cwd)?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("source answer".into()),
        settled_metadata("source"),
    ]]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("apply context", temp.path()).await?;
    let source = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    coordinator
        .set_agent_execution_cwd(system_actor(), source.clone(), cwd.clone())
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let turn = coordinator
        .request_agent_turn(system_actor(), source.clone(), "source")
        .await?;
    wait_terminal(&mut events, &turn).await?;
    let target = coordinator
        .spawn_agent(system_actor(), "default", Some(source.clone()))
        .await?;
    let child_events = temp
        .path()
        .join("sessions")
        .join(&target)
        .join("events.jsonl");
    std::fs::remove_file(&child_events)?;
    std::fs::create_dir(&child_events)?;
    let mut events = coordinator.subscribe_new_events().await?;
    let result = if initialize {
        coordinator
            .initialize_agent_from_finalized(
                system_actor(),
                target.clone(),
                source,
                FinalizedContextCopy::Fork,
            )
            .await
    } else {
        coordinator
            .set_agent_execution_cwd(system_actor(), target.clone(), cwd.clone())
            .await
    };
    assert!(
        result.is_err(),
        "projection IO must fail closed after authoritative append"
    );
    let recorded = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(event) = events.next().await {
                let event = event?;
                if matches!(&event.payload, EventV1::AgentContextInitialized(e) if e.agent_id.0 == target)
                    || matches!(&event.payload, EventV1::AgentExecutionContextChanged(e) if e.agent_id.0 == target) {
                    return Ok::<_, EventStoreError>(event);
                }
            }
            Err(EventStoreError::Invalid("accepted event missing"))
        }).await??;
    assert!(crate::store::read_events(&run.events_path)?.contains(&recorded));
    coordinator
        .call(move |s| {
            let state = &s.agents[&target];
            assert!(s.fault.is_some());
            if initialize {
                assert!(state
                    .messages
                    .entries
                    .iter()
                    .any(|e| e.message.content == "source answer"));
                assert!(state.source_reference.is_some());
            } else {
                assert_eq!(state.cwd, cwd);
                assert_eq!(state.execution.effective_cwd, cwd.to_string_lossy());
            }
            Ok(())
        })
        .await?;
    assert!(coordinator.stop_run().await.is_err());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn changed_canonical_cwd_is_rejected_before_provider_or_tool_dispatch(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let accepted = temp.path().join("accepted");
    let moved = temp.path().join("moved");
    let denied = temp.path().join("denied");
    std::fs::create_dir(&accepted)?;
    std::fs::create_dir(&denied)?;
    let provider = Arc::new(MockProvider::script([]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.permission_policy = PermissionPolicy::from_rules(vec![
        crate::perm::PermissionRule {
            permission: "*".into(),
            pattern: crate::config::PermissionSelector::CatchAll,
            action: crate::perm::PermissionAction::Allow,
        },
        crate::perm::PermissionRule {
            permission: "read".into(),
            pattern: crate::config::PermissionSelector::Exact("denied".into()),
            action: crate::perm::PermissionAction::Deny,
        },
    ])?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(LargeTool(Arc::clone(&calls))));
    config.tool_registry = Arc::new(registry);
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["large".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("changed cwd", temp.path()).await?;
    let agent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    assert!(coordinator
        .set_agent_execution_cwd(system_actor(), agent.clone(), denied.clone())
        .await
        .is_err());
    coordinator
        .set_agent_execution_cwd(system_actor(), agent.clone(), accepted.clone())
        .await?;
    std::fs::rename(&accepted, &moved)?;
    std::os::unix::fs::symlink(&denied, &accepted)?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent.clone()));
    assert!(coordinator
        .request_tool_call(actor, None, "large", json!({}))
        .await
        .is_err());
    assert!(coordinator
        .request_agent_turn(system_actor(), agent.clone(), "must not dispatch")
        .await
        .is_err());
    assert_eq!(provider.call_count(), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    coordinator
        .call(move |s| {
            assert_eq!(s.agents[&agent].cwd, accepted);
            assert_eq!(s.agents[&agent].generation, 0);
            Ok(())
        })
        .await?;
    coordinator.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn versioned_fold_backs_deferred_finishes_expires_and_seals_usage(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(MockProvider::script([vec![
        Stream::TextDelta("answer".into()),
        settled_metadata("settled"),
    ]]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("fold", temp.path()).await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let child = coordinator
        .spawn_agent(system_actor(), "default", Some(parent))
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let id = coordinator
        .request_agent_turn(system_actor(), child.clone(), "prompt")
        .await?;
    wait_terminal(&mut events, &id).await?;
    let journal = crate::store::read_events(&run.events_path)?;
    let transitions: Vec<_> = journal
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::SubagentTransition(e) => Some(e.as_ref().clone()),
            _ => None,
        })
        .collect();
    let spawn = transitions.first().ok_or("spawn missing")?;
    let finish = transitions.last().ok_or("finish missing")?;
    let mut fold = SubagentHistory::default();
    assert!(fold.apply_transition(finish, 0));
    assert_eq!(fold.deferred_count(), 1);
    assert!(fold.apply_transition(spawn, 1));
    assert!(fold.records[&child].lifecycle.is_finished());
    assert_eq!(fold.records[&child].accounting, finish.accounting);
    assert!(!fold.apply_transition(spawn, 2));
    assert!(!fold.apply_transition(finish, 2));
    for attemptless in [false, true] {
        let mut future_finish = finish.clone();
        future_finish.generation = spawn.generation + 1;
        future_finish.notification_seq = Some(5);
        if attemptless {
            future_finish.attempt_id = None;
        }
        let mut mismatched = SubagentHistory::default();
        assert!(mismatched.apply_transition(&future_finish, 0));
        assert!(mismatched.apply_transition(spawn, 1));
        assert!(!mismatched.records[&child].lifecycle.is_finished());
        assert_eq!(mismatched.records[&child].accounting, None);
        assert!(!mismatched.finalized.contains_key(&child));
        if attemptless {
            let mut next_spawn = spawn.clone();
            next_spawn.generation += 1;
            next_spawn.attempt_id = Some("later-native-attempt".into());
            next_spawn.notification_seq = Some(4);
            assert!(mismatched.apply_transition(&next_spawn, 2));
            assert!(mismatched.records[&child].lifecycle.is_finished());
            assert_eq!(mismatched.records[&child].accounting, finish.accounting);
            assert!(
                !mismatched.finalized.contains_key(&child),
                "a prior attempt/generation reference cannot become the later attempt's state"
            );
        }
    }
    let mut wrong_reference = finish.clone();
    wrong_reference
        .finalized_state
        .as_mut()
        .ok_or("finish reference missing")?
        .generation += 1;
    let mut reference_fold = SubagentHistory::default();
    assert!(reference_fold.apply_transition(&wrong_reference, 0));
    assert!(reference_fold.apply_transition(spawn, 1));
    assert!(!reference_fold.finalized.contains_key(&child));
    let mut stale = finish.clone();
    stale.generation = 0;
    assert!(!fold.apply_transition(&stale, 2));
    let mut retired = finish.clone();
    retired.transition = SubagentTransitionKind::Retired;
    assert!(fold.apply_transition(&retired, 3));
    assert!(!fold.apply_transition(finish, 4));
    let mut deferred = SubagentHistory::default();
    for n in 0..257 {
        let mut event = finish.clone();
        event.child_id = SubagentId(format!("deferred-{n}"));
        event.finalized_state = None;
        assert!(deferred.apply_transition(&event, 0));
    }
    assert_eq!(deferred.deferred_count(), 256);
    deferred.expire(60_000);
    assert_eq!(deferred.deferred_count(), 0);
    assert!(deferred.records.values().all(|r| !r
        .lifecycle
        .retains_attempt(&SubagentAttemptKey::Id(id.clone()))));
    let legacy: Vec<_> = journal
        .iter()
        .filter(|e| {
            !matches!(
                e.payload,
                EventV1::SubagentTransition(_) | EventV1::FinalizedAgentState(_)
            )
        })
        .cloned()
        .collect();
    let legacy_bytes = serde_json::to_vec(&legacy)?;
    let decoded: Vec<EventEnvelopeV1> = serde_json::from_slice(&legacy_bytes)?;
    let legacy_fold = SubagentHistory::from_events(&decoded);
    assert!(legacy_fold.records[&child].legacy);
    assert!(legacy_fold.records[&child].lifecycle.is_finished());
    assert!(legacy_fold.finalized.is_empty());
    assert_eq!(serde_json::to_vec(&decoded)?, legacy_bytes);
    coordinator.stop_run().await?;
    Ok(())
}

struct HeldProvider;
#[async_trait::async_trait]
impl harness_providers::Provider for HeldProvider {
    async fn stream_completion(
        &self,
        _: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        Box::pin(tokio_stream::pending())
    }
}

struct Delegate;
#[async_trait::async_trait]
impl Tool for Delegate {
    fn id(&self) -> &str {
        "delegate"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        ctx.coordinator
            .delegate_task(
                ctx.tool_call_id.to_string(),
                ChildTaskRequest {
                    session_id: args["session_id"].as_str().map(str::to_owned),
                    profile: "child".into(),
                    prompt: args["prompt"].as_str().unwrap_or("held child").into(),
                    description: "foreground child".into(),
                    run_in_background: false,
                },
            )
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))
    }
}

#[tokio::test]
async fn direct_child_projection_validates_original_finalized_and_initialized_owners(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::TextDelta("root source".into()),
            settled_metadata("root"),
        ],
        vec![
            Stream::TextDelta("child source".into()),
            settled_metadata("child"),
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("projection owners", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let child = coordinator
        .spawn_agent(system_actor(), "default", Some(parent.clone()))
        .await?;
    for agent in [&parent, &child] {
        let mut events = coordinator.subscribe_new_events().await?;
        let turn = coordinator
            .request_agent_turn(system_actor(), agent.clone(), "source")
            .await?;
        wait_terminal(&mut events, &turn).await?;
    }
    let initialized = coordinator
        .spawn_agent(system_actor(), "default", Some(parent.clone()))
        .await?;
    coordinator
        .initialize_agent_from_finalized(
            system_actor(),
            initialized.clone(),
            parent,
            FinalizedContextCopy::Fork,
        )
        .await?;
    coordinator.stop_run().await?;
    let original = std::fs::read(&run.events_path)?;
    for (id, copied) in [(&child, false), (&initialized, true)] {
        let resumed = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        resumed.resume_run(id.clone(), "direct child").await?;
        if copied {
            let id = id.clone();
            resumed
                .call(move |s| {
                    assert!(s.agents[&id]
                        .messages
                        .entries
                        .iter()
                        .any(|e| e.message.content == "root source"));
                    Ok(())
                })
                .await?;
        } else {
            let FinalizedStateResult::Available { state } =
                resumed.raw_finalized_state(id.clone()).await?
            else {
                return Err("original-owner child state unavailable".into());
            };
            assert_eq!(state.owner_run_id, run.run_id.to_string());
        }
        assert_eq!(provider.call_count(), 2);
        resumed.stop_run().await?;
        assert_eq!(std::fs::read(&run.events_path)?, original);
    }
    // A copied valid blob and fabricated lineage cannot authorize a foreign original event.
    let metadata = temp
        .path()
        .join("sessions")
        .join(&child)
        .join(crate::proj::META_FILE_NAME);
    let value: Value = serde_json::from_slice(&std::fs::read(&metadata)?)?;
    for field in ["parent_session_id", "parent_run_id"] {
        let mut changed = value.clone();
        changed["harness_lineage"][field] = "foreign".into();
        std::fs::write(&metadata, serde_json::to_vec(&changed)?)?;
        let denied = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        assert!(denied
            .resume_run(child.clone(), "reject foreign owner")
            .await
            .is_err());
        assert!(denied.run_info().await.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn delegation_continuation_retains_committed_raw_state(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("sample.txt"), "sample")?;
    let arguments = "{ \"path\" : \"sample.txt\" }";
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "raw-call".into(),
                function_name: "large".into(),
                arguments_json: arguments.into(),
            },
            settled_metadata("first"),
        ],
        vec![
            Stream::TextDelta("child answer".into()),
            settled_metadata("second"),
        ],
        vec![
            Stream::TextDelta("continued answer".into()),
            settled_metadata("third"),
        ],
        vec![
            Stream::TextDelta("summary source".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("summary continued".into()),
            settled_metadata("complete new metadata"),
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.permission_policy = PermissionPolicy::allow_all();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Delegate));
    registry.register(Arc::new(LargeTool(Arc::new(AtomicUsize::new(0)))));
    config.tool_registry = Arc::new(registry);
    let mut root = AgentProfile::fallback("default");
    root.toolset = vec!["delegate".into()];
    config.agent_profiles.insert("default".into(), root);
    let mut child = AgentProfile::fallback("child");
    child.toolset = vec!["large".into()];
    config.agent_profiles.insert("child".into(), child);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("continuation", temp.path()).await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let first = coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "delegate".into(),
            json!({"prompt":"source"}),
        )
        .await?;
    let child = first
        .structured_json
        .as_ref()
        .and_then(|v| v["session_id"].as_str())
        .ok_or("child missing")?
        .to_owned();
    let FinalizedStateResult::Available { state: original } =
        coordinator.raw_finalized_state(child.clone()).await?
    else {
        return Err("source unavailable".into());
    };
    coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "delegate".into(),
            json!({"session_id":child,"prompt":"continue"}),
        )
        .await?;
    let FinalizedStateResult::Available { state: continued } =
        coordinator.raw_finalized_state(child).await?
    else {
        return Err("exact continuation unavailable".into());
    };
    assert_eq!(
        &continued.conversation_items[..original.conversation_items.len()],
        original.conversation_items.as_slice()
    );
    assert_eq!(
        &continued.usage[..original.usage.len()],
        original.usage.as_slice()
    );
    let events = crate::store::read_events(&coordinator.run_info().await?.events_path)?;
    let historical =
        super::history::messages(&events, &continued.owner_agent_id.0, false, "", temp.path())?;
    assert_eq!(
        historical.unavailable,
        Some(FinalizedStateUnavailable::LegacySummaryOnly),
        "presentation reconstruction is intrinsically summary-only"
    );
    let first = coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "delegate".into(),
            json!({"prompt":"summary source"}),
        )
        .await?;
    let summary_child = first
        .structured_json
        .as_ref()
        .and_then(|v| v["session_id"].as_str())
        .ok_or("summary child missing")?
        .to_owned();
    assert!(matches!(
        coordinator
            .raw_finalized_state(summary_child.clone())
            .await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::UnsupportedReasoning
        }
    ));
    coordinator
        .execute_tool(
            actor,
            None,
            None,
            "delegate".into(),
            json!({"session_id":summary_child,"prompt":"summary continuation"}),
        )
        .await?;
    assert_eq!(
        coordinator
            .raw_finalized_state(summary_child.clone())
            .await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::LegacySummaryOnly
        }
    );
    let history = coordinator.subagent_history().await?;
    assert_eq!(
        history.finalized[&summary_child].state.fidelity,
        FinalizedStateFidelity::SummaryOnly
    );
    coordinator.stop_run().await?;
    Ok(())
}

struct DelegatingProvider {
    child_release: Arc<tokio::sync::Notify>,
    parent_calls: AtomicUsize,
}
#[async_trait::async_trait]
impl harness_providers::Provider for DelegatingProvider {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        if request
            .messages
            .last()
            .is_some_and(|m| m.content == "held child")
        {
            let (tx, rx) = mpsc::channel(2);
            let release = Arc::clone(&self.child_release);
            tokio::spawn(async move {
                release.notified().await;
                let _ = tx
                    .send(Stream::TextDelta("surviving child answer".into()))
                    .await;
                let _ = tx.send(settled_metadata("survivor")).await;
            });
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
        } else if self.parent_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Box::pin(tokio_stream::iter(vec![
                Stream::ToolCallComplete {
                    tool_call_id: "foreground".into(),
                    function_name: "delegate".into(),
                    arguments_json: "{}".into(),
                },
                settled_metadata("parent call"),
            ]))
        } else {
            Box::pin(tokio_stream::iter(vec![
                Stream::TextDelta("parent released".into()),
                settled_metadata("parent release"),
            ]))
        }
    }
}

#[tokio::test]
async fn foreground_delegate_reply_releases_on_waiter_and_prompt_cancel(
) -> Result<(), Box<dyn std::error::Error>> {
    for cancel_prompt in [false, true] {
        let temp = tempfile::tempdir()?;
        let release = Arc::new(tokio::sync::Notify::new());
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::new(DelegatingProvider {
            child_release: Arc::clone(&release),
            parent_calls: AtomicUsize::new(0),
        });
        config.provider_model_concurrency = 2;
        config.permission_policy = PermissionPolicy::allow_all();
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(Delegate));
        config.tool_registry = Arc::new(registry);
        let mut root = AgentProfile::fallback("default");
        root.toolset = vec!["delegate".into()];
        config.agent_profiles.insert("default".into(), root);
        config
            .agent_profiles
            .insert("child".into(), AgentProfile::fallback("child"));
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run("real waiter", temp.path()).await?;
        let parent = coordinator
            .spawn_agent(system_actor(), "default", None)
            .await?;
        let mut events = coordinator.subscribe_new_events().await?;
        let prompt = coordinator
            .request_agent_turn(system_actor(), parent.clone(), "delegate")
            .await?;
        let (child, child_attempt, waiter) =
            wait_delegated_child_started(&mut events, &parent).await?;
        let mut events = coordinator.subscribe_new_events().await?;
        coordinator
            .request_subagent_cancel(
                system_actor(),
                if cancel_prompt {
                    SubagentCommandRequest::ParentPromptCancel {
                        prompt_id: prompt.clone(),
                    }
                } else {
                    SubagentCommandRequest::WaiterCancel {
                        waiter_id: waiter.clone(),
                    }
                },
            )
            .await?;
        wait_cancelled(&mut events, &waiter).await?;
        if cancel_prompt {
            wait_cancelled(&mut events, &prompt).await?;
        } else {
            wait_terminal(&mut events, &prompt).await?;
        }
        assert!(matches!(
            coordinator.raw_finalized_state(child.clone()).await?,
            FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::Active
            }
        ));
        let mut events = coordinator.subscribe_new_events().await?;
        release.notify_one();
        wait_terminal(&mut events, &child_attempt).await?;
        assert!(matches!(
            coordinator.raw_finalized_state(child).await?,
            FinalizedStateResult::Available { .. }
        ));
        coordinator.stop_run().await?;
    }
    Ok(())
}

async fn wait_delegated_child_started(
    events: &mut crate::store::EventStream,
    parent: &str,
) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    let started = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut waiter = None;
        while let Some(event) = events.next().await {
            let event = event?;
            if let Some(e) = match &event.payload {
                EventV1::ToolCallRequested(e) if e.tool_id == "delegate" => Some(e),
                _ => None,
            } {
                waiter = Some(e.tool_call_id.to_string());
            }
            if matches!(event.payload, EventV1::ProviderRequestStarted(_))
                && event
                    .actor
                    .agent_id
                    .as_deref()
                    .is_some_and(|id| id != parent)
            {
                return Ok::<_, EventStoreError>((
                    event
                        .actor
                        .agent_id
                        .ok_or(EventStoreError::Invalid("child missing"))?,
                    event
                        .correlation_id
                        .ok_or(EventStoreError::Invalid("attempt missing"))?,
                    waiter.ok_or(EventStoreError::Invalid("waiter missing"))?,
                ));
            }
        }
        Err(EventStoreError::Invalid("delegation not started"))
    })
    .await??;
    Ok(started)
}

struct Waiter;
#[async_trait::async_trait]
impl Tool for Waiter {
    fn id(&self) -> &str {
        "waiter"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn call(&self, context: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        context.cancellation.cancelled().await;
        Err(ToolError::Cancelled)
    }
}

#[tokio::test]
async fn cancellation_intents_keep_child_prompt_waiter_and_shutdown_scopes_distinct(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(HeldProvider);
    config.provider_model_concurrency = 5;
    config.permission_policy = PermissionPolicy::allow_all();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Waiter));
    config.tool_registry = Arc::new(registry);
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["waiter".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("cancel scopes", temp.path()).await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let child = coordinator
        .spawn_agent(system_actor(), "default", Some(parent.clone()))
        .await?;
    let descendant = coordinator
        .spawn_agent(system_actor(), "default", Some(child.clone()))
        .await?;
    let owned_descendant = coordinator
        .spawn_agent(system_actor(), "default", Some(child.clone()))
        .await?;
    let other = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let mut attempts = BTreeMap::new();
    for id in [&parent, &child, &descendant, &owned_descendant, &other] {
        attempts.insert(
            id.clone(),
            coordinator
                .request_agent_turn(system_actor(), id.clone(), "held")
                .await?,
        );
    }
    let waiter = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(parent.clone())),
            None,
            "waiter",
            json!({}),
        )
        .await?;
    let remaining = attempts.clone();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut started = std::collections::BTreeSet::new();
        while let Some(event) = events.next().await {
            let event = event?;
            let request = matches!(event.payload, EventV1::ProviderRequestStarted(_))
                .then_some(event.correlation_id)
                .flatten();
            started.extend(request);
            if matches!(event.payload, EventV1::ToolCallStarted(_)) {
                started.insert(waiter.clone());
            }
            if remaining.values().all(|id| started.contains(id)) && started.contains(&waiter) {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("held work did not start"))
    })
    .await??;
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::WaiterCancel {
                waiter_id: waiter.clone(),
            },
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCancelled(e) if e.task_id.as_str() == waiter) {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("waiter did not cancel"))
    })
    .await??;
    assert!(matches!(
        coordinator.raw_finalized_state(descendant.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let original_ancestry = coordinator.subagent_history().await?.records[&descendant]
        .metadata
        .as_ref()
        .ok_or("descendant metadata missing")?
        .ancestry
        .clone();
    coordinator
        .reparent_subagent_to_root(system_actor(), descendant.clone(), InjectedSubagentDepth(0))
        .await?;
    let routed = coordinator.subagent_history().await?;
    let metadata = routed.records[&descendant]
        .metadata
        .as_ref()
        .ok_or("routed metadata missing")?;
    assert_eq!(metadata.ancestry, original_ancestry);
    assert_eq!(metadata.injected_depth, InjectedSubagentDepth(0));
    assert!(matches!(
        &metadata.execution_owner,
        SubagentExecutionOwner::RootSession { .. }
    ));
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ExplicitChildKill {
                child_id: SubagentId(child.clone()),
            },
        )
        .await?;
    wait_cancelled(&mut events, &attempts[&child]).await?;
    assert!(matches!(
        coordinator.raw_finalized_state(descendant.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    assert!(coordinator
        .request_agent_turn(system_actor(), child.clone(), "cannot wake")
        .await
        .is_err());
    assert!(coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ChildSessionCancel {
                session_id: descendant.clone(),
                descendants: vec![SubagentId(other.clone())]
            }
        )
        .await
        .is_err());
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ParentPromptCancel {
                prompt_id: attempts[&parent].clone(),
            },
        )
        .await?;
    wait_cancelled(&mut events, &attempts[&parent]).await?;
    assert!(matches!(
        coordinator.raw_finalized_state(descendant.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let mut events = coordinator.subscribe_new_events().await?;
    let cancelled = coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ChildSessionCancel {
                session_id: child.clone(),
                descendants: vec![SubagentId(owned_descendant.clone())],
            },
        )
        .await?;
    assert!(cancelled.contains(&SubagentId(owned_descendant.clone())));
    assert!(
        !cancelled.contains(&SubagentId(descendant.clone())),
        "root-reparented execution is not an ancestry cancellation edge"
    );
    wait_cancelled(&mut events, &attempts[&owned_descendant]).await?;
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ChildSessionCancel {
                session_id: descendant.clone(),
                descendants: Vec::new(),
            },
        )
        .await?;
    wait_cancelled(&mut events, &attempts[&descendant]).await?;
    coordinator
        .request_subagent_cancel(
            system_actor(),
            SubagentCommandRequest::ParentSessionStop {
                session_id: parent.clone(),
            },
        )
        .await?;
    assert!(coordinator
        .request_agent_turn(system_actor(), parent, "admission stopped")
        .await
        .is_err());
    assert!(matches!(
        coordinator.raw_finalized_state(other.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::Active
        }
    ));
    let mut events = coordinator.subscribe_new_events().await?;
    coordinator
        .request_subagent_cancel(system_actor(), SubagentCommandRequest::RootShutdown)
        .await?;
    wait_cancelled(&mut events, &attempts[&other]).await?;
    assert!(coordinator.run_info().await.is_err());
    let history = crate::store::read_events(&run.events_path)?;
    assert_eq!(
        history
            .iter()
            .filter(|e| matches!(e.payload, EventV1::SubagentCancelRequested(_)))
            .count(),
        7
    );
    assert!(history
        .iter()
        .all(|e| !matches!(e.payload, EventV1::ConversationRewound(_))));
    Ok(())
}

async fn wait_cancelled(
    events: &mut crate::store::EventStream,
    attempt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCancelled(e) if e.task_id.as_str() == attempt)
            {
                return Ok::<_, EventStoreError>(());
            }
        }
        Err(EventStoreError::Invalid("cancelled terminal missing"))
    })
    .await??;
    Ok(())
}
