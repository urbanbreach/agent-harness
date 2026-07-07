use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn proactive_compaction_writes_checkpoint_artifact_and_updates_provider_context() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_proactive");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("first question", 'A'),
            long_turn("second question", 'B'),
        ]),
    );

    let updated = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
            prompt_tokens_estimate: None,
            estimate_source: None,
        },
        &CompactionRuntimeConfig::default(),
        &crate::coord::CompactionSummaryDecision::deterministic(&ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
            prompt_tokens_estimate: None,
            estimate_source: None,
        }),
    )
    .unwrap_or_abort()
    .unwrap_or_abort();

    assert!(updated.updated_context.compacted_summary.is_some());
    assert_eq!(updated.updated_context.preserved_turns.len(), 1);
    assert_eq!(
        updated.updated_context.preserved_turns[0].user_prompt,
        "second question"
    );

    let events = read_events(&run_state.info.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::CompactionRequested(_))));
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::CompactionApplied(_))));
    assert!(written.tokens_before_estimate.is_some());
    assert!(written.tokens_after_estimate.is_some());
    assert!(written.tokens_after_estimate < written.tokens_before_estimate);
    assert_eq!(written.compacted_turns, Some(1));

    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).unwrap_or_abort();
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).unwrap_or_abort();
    assert_eq!(checkpoint.metadata.agent_id, "agent_000001");
    assert_eq!(
        checkpoint.metadata.tokens_before_estimate,
        written.tokens_before_estimate
    );
    assert_eq!(
        checkpoint.metadata.tokens_after_estimate,
        written.tokens_after_estimate
    );
    assert_eq!(
        checkpoint.metadata.summary_tokens_estimate,
        written.summary_tokens_estimate
    );
    assert!(checkpoint.metadata.reduction_tokens_estimate.is_some());
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(checkpoint.facts.compacted_turns.len(), 1);
    assert_eq!(
        checkpoint.facts.compacted_turns[0].user_excerpt,
        "first question"
    );
    assert_eq!(
        checkpoint
            .tail_boundary
            .as_ref()
            .map(|boundary| boundary.mode.as_str()),
        Some("whole_turn_tail")
    );
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .map(|source| source.strategy.as_str()),
        Some("deterministic_rolling_summary")
    );
    assert_eq!(
        checkpoint
            .timeline_entry
            .as_ref()
            .map(|entry| entry.entry_type.as_str()),
        Some("proactive_compaction")
    );
    let compaction_config = CompactionRuntimeConfig::default();
    for heading in provider_context_summary_required_headings(&compaction_config) {
        assert!(
            checkpoint.summary.contains(heading),
            "summary missing structured heading {heading}:\n{}",
            checkpoint.summary
        );
    }
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_version),
        Some(crate::coord::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION)
    );
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_enforced),
        Some(true)
    );
    assert!(checkpoint.summary.contains("first question"));
}

pub(crate) fn proactive_compaction_records_pruned_tool_artifacts_for_compacted_turns() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    write_restore_history_fixture(
        temp_dir.path(),
        "run_compaction_pruned_artifacts",
        &[
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                1,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                2,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".to_string(),
                    text: "first question".to_string(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "first question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                4,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("req_000001"),
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/toolcalls/toolcall_000001/result.txt".to_string(),
                    digest: "digest-artifact-1".to_string(),
                    bytes: 42,
                    tool_call_id: Some("toolcall_000001".to_string()),
                    tool_metadata: None,
                    metadata: std::collections::BTreeMap::new(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: first_answer.clone(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                6,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000002".to_string(),
                    text: "second question".to_string(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "second question".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string(),
                    result_summary: second_answer.clone(),
                    result_digest: "digest-task-2".to_string(),
                    metadata: None,
                }),
            ),
        ],
    );
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_pruned_artifacts");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "first question".to_string(),
                assistant_response: first_answer.clone(),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "second question".to_string(),
                assistant_response: second_answer.clone(),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    run_state.next_event_seq = 9;

    let updated = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
            prompt_tokens_estimate: None,
            estimate_source: None,
        },
        &CompactionRuntimeConfig::default(),
        &crate::coord::CompactionSummaryDecision::deterministic(&ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
            prompt_tokens_estimate: None,
            estimate_source: None,
        }),
    )
    .unwrap_or_abort()
    .unwrap_or_abort();

    assert!(updated.updated_context.compacted_summary.is_some());
    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .unwrap_or_abort();
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).unwrap_or_abort();
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).unwrap_or_abort();
    assert_eq!(checkpoint.pruned_tool_artifacts.len(), 1);
    assert_eq!(
        checkpoint.pruned_tool_artifacts[0].path,
        "artifacts/toolcalls/toolcall_000001/result.txt"
    );
    assert_eq!(
        checkpoint.pruned_tool_artifacts[0].digest.as_deref(),
        Some("digest-artifact-1")
    );
    assert!(checkpoint
        .summary
        .contains("artifacts/toolcalls/toolcall_000001/result.txt"));
}
