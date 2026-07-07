use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn restore_provider_context_from_history_uses_checkpoint_then_replays_post_checkpoint_turns(
) {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_restore_checkpointed_context";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000010.json".to_string();
    let checkpoint_path = run_dir.join(&checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&ProviderContextCheckpoint {
            metadata: ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_000010".to_string(),
                agent_id: "agent_000001".to_string(),
                run_id: run_id.to_string(),
                through_seq: 9,
                through_request_id: Some("req_000002".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("model-1".to_string()),
                tokens_before: Some(3_900),
                tokens_before_estimate: None,
                tokens_after_estimate: None,
                summary_tokens_estimate: None,
                compacted_turns: None,
                preserved_turns: None,
                reduction_tokens_estimate: None,
                reduction_percent_estimate: None,
                trigger_reason: Some("proactive".to_string()),
            },
            summary: "Earlier checkpoint summary".to_string(),
            recent_turns: vec![ProviderConversationTurn {
                user_prompt: "second question".to_string(),
                assistant_response: "second answer".to_string(),
                ..ProviderConversationTurn::default()
            }],
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts::default(),
            tail_boundary: None,
            summary_source: None,
            timeline_entry: None,
        })
        .unwrap_or_abort(),
    )
    .unwrap_or_abort();

    write_restore_history_fixture(
        temp_dir.path(),
        run_id,
        &[
            restore_fixture_event(
                run_id,
                1,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                2,
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
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001".to_string(),
                    delta: "first answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "first answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(crate::event::UserMessageSubmittedEvent {
                    request_id: "req_000002".to_string(),
                    text: "second question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
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
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000002".to_string(),
                    delta: "second answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string(),
                    result_summary: "second answer".to_string(),
                    result_digest: "digest-task-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                9,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.clone(),
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 512,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000002".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: Some(3_900),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                    summary_source: None,
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                10,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000002".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                11,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(crate::event::UserMessageSubmittedEvent {
                    request_id: "req_000003".to_string(),
                    text: "third question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                12,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000003"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000003".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "third question".to_string(),
                    request_digest: "digest-3".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                13,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000003"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000003".to_string(),
                    delta: "third answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                14,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000003"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000003".to_string(),
                    result_summary: "third answer".to_string(),
                    result_digest: "digest-task-3".to_string(),
                    metadata: None,
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id).unwrap_or_abort();
    let context = restored.get("agent_000001").unwrap_or_abort();
    assert_eq!(
        context.compacted_summary.as_deref(),
        Some("Earlier checkpoint summary")
    );
    assert_eq!(context.preserved_turns.len(), 2);
    assert_eq!(context.preserved_turns[0].user_prompt, "second question");
    assert_eq!(context.preserved_turns[1].user_prompt, "third question");
}

pub(crate) fn failed_turn_context_resume_reconstructs_failed_turn_after_checkpoint() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_restore_failed_turn_after_checkpoint";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000010.json".to_string();
    let checkpoint_path = run_dir.join(&checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&ProviderContextCheckpoint {
            metadata: ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_000010".to_string(),
                agent_id: "agent_000001".to_string(),
                run_id: run_id.to_string(),
                through_seq: 5,
                through_request_id: Some("req_000001".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("model-1".to_string()),
                tokens_before: Some(3_900),
                tokens_before_estimate: None,
                tokens_after_estimate: None,
                summary_tokens_estimate: None,
                compacted_turns: None,
                preserved_turns: None,
                reduction_tokens_estimate: None,
                reduction_percent_estimate: None,
                trigger_reason: Some("proactive".to_string()),
            },
            summary: "Earlier checkpoint summary".to_string(),
            recent_turns: vec![ProviderConversationTurn {
                user_prompt: "first question".to_string(),
                assistant_response: "first answer".to_string(),
                request_id: Some("req_000001".to_string()),
                ..ProviderConversationTurn::default()
            }],
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts::default(),
            tail_boundary: None,
            summary_source: None,
            timeline_entry: None,
        })
        .unwrap_or_abort(),
    )
    .unwrap_or_abort();

    write_restore_history_fixture(
        temp_dir.path(),
        run_id,
        &[
            restore_fixture_event(
                run_id,
                1,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                2,
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
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_000001".to_string(),
                    delta: "first answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "first answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.clone(),
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 512,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 5,
                    through_request_id: Some("req_000001".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: Some(3_900),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                    summary_source: None,
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 5,
                    through_request_id: Some("req_000001".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000002".to_string(),
                    text: "failed question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000002".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:model-1".to_string()),
                }),
            ),
            restore_fixture_event(
                run_id,
                9,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002_provider".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "failed question".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                10,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_000002_provider".to_string(),
                    delta: "partial failed answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                11,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000002_provider".to_string(),
                    finish_reason: "error".to_string(),
                    output_digest: None,
                    usage: None,
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                12,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000002".to_string(),
                    reason: "provider exploded".to_string(),
                    task_scope: Some(crate::event::TaskTerminalScope::AgentTurn),
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id).unwrap_or_abort();
    let context = restored.get("agent_000001").unwrap_or_abort();
    assert_eq!(context.preserved_turns.len(), 2);
    let failed_turn = &context.preserved_turns[1];
    assert_eq!(failed_turn.user_prompt, "failed question");
    assert_eq!(failed_turn.assistant_response, "partial failed answer");
    assert_eq!(failed_turn.status, ProviderConversationTurnStatus::Failed);
    assert_eq!(failed_turn.failure_stage.as_deref(), Some("provider_error"));
    assert_eq!(
        failed_turn.failure_reason.as_deref(),
        Some("provider exploded")
    );
    assert_eq!(
        failed_turn.request_id.as_deref(),
        Some("req_000002_provider")
    );
    assert_eq!(failed_turn.first_seq, Some(7));
    assert_eq!(failed_turn.last_seq, Some(12));
}
