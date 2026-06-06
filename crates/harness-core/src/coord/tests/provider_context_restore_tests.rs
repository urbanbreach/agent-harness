use super::*;

#[path = "provider_context_restore_checkpoint_tests.rs"]
mod provider_context_restore_checkpoint_tests;
pub(super) use provider_context_restore_checkpoint_tests::{
    failed_turn_context_resume_reconstructs_failed_turn_after_checkpoint as checkpoint_failed_turn_context_resume_reconstructs_failed_turn_after_checkpoint,
    restore_provider_context_from_history_uses_checkpoint_then_replays_post_checkpoint_turns as checkpoint_restore_provider_context_from_history_uses_checkpoint_then_replays_post_checkpoint_turns,
};

pub(super) fn restore_provider_context_uses_task_completed_summary_for_iterative_history() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_iterative_restore";
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
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "first question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001".to_string(),
                    delta: "calling tool".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001_iter_02"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001_iter_02".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "tool result follow-up".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001_iter_02"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001_iter_02".to_string(),
                    delta: "final answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "final answer".to_string(),
                    result_digest: "digest-task".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore provider context");
    let turns = restored
        .get("agent_000001")
        .expect("agent should have restored history");
    assert_eq!(turns.preserved_turns.len(), 1);
    assert_eq!(turns.preserved_turns[0].user_prompt, "first question");
    assert_eq!(turns.preserved_turns[0].assistant_response, "final answer");
}

pub(super) fn failed_response_compaction_does_not_double_compact_same_request() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let agent_id = "agent_000001".to_string();
    let task_id = "task_000001".to_string();
    let request_id = "req_000001".to_string();
    let overflow_context = ProviderContext::from_turns(vec![long_turn("overflow question", 'A')]);

    let mut unchanged = test_run_state(temp_dir.path(), "run_failed_terminal_guard_unchanged");
    unchanged
        .provider_context_by_agent
        .insert(agent_id.clone(), overflow_context.clone());
    unchanged
        .overflow_retry_compacted_context_by_attempt
        .insert(
            (task_id.clone(), request_id.clone()),
            overflow_context.clone(),
        );
    let failed_request = FailedTerminalCompactionRequest {
        task_id: task_id.clone(),
        agent_id: agent_id.clone(),
        request_id: request_id.clone(),
        trigger_reason: "failed_response".to_string(),
    };
    assert!(
        !unchanged.failed_terminal_compaction_attempt_should_run(&failed_request),
        "unchanged context already checkpointed by overflow retry should not compact again"
    );
    let aborted_request = FailedTerminalCompactionRequest {
        trigger_reason: "aborted_response".to_string(),
        ..failed_request.clone()
    };
    assert!(
        !unchanged.failed_terminal_compaction_attempt_should_run(&aborted_request),
        "same task/request must not run both failed and aborted terminal compaction"
    );

    let mut changed = test_run_state(temp_dir.path(), "run_failed_terminal_guard_changed");
    changed
        .provider_context_by_agent
        .insert(agent_id.clone(), overflow_context.clone());
    changed
        .overflow_retry_compacted_context_by_attempt
        .insert((task_id.clone(), request_id.clone()), overflow_context);
    changed
        .provider_context_by_agent
        .get_mut(&agent_id)
        .expect("agent context")
        .push_turn(ProviderConversationTurn {
            user_prompt: "failed retry question".to_string(),
            assistant_response: "partial retry output".to_string(),
            status: ProviderConversationTurnStatus::Failed,
            failure_stage: Some("overflow_retry_failed".to_string()),
            failure_reason: Some("overflow persisted".to_string()),
            request_id: Some("req_provider_retry".to_string()),
            ..ProviderConversationTurn::default()
        });
    assert!(
        changed.failed_terminal_compaction_attempt_should_run(&failed_request),
        "appending the failed turn materially changes context and permits terminal compaction"
    );
    assert!(
        !changed.failed_terminal_compaction_attempt_should_run(&aborted_request),
        "terminal compaction is still one-shot per task/request after a real attempt"
    );
}

pub(super) fn failed_turn_context_does_not_duplicate_completed_turns() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_restore_no_duplicate_completed_turn";
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
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".to_string(),
                    text: "completed question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:model-1".to_string()),
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001_provider".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "completed question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_000001_provider".to_string(),
                    delta: "completed answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000001_provider".to_string(),
                    finish_reason: "done".to_string(),
                    output_digest: Some("digest-out-1".to_string()),
                    usage: None,
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "completed answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000001".to_string(),
                    reason: "late cancellation after completed turn".to_string(),
                    task_scope: Some(crate::event::TaskTerminalScope::AgentTurn),
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore provider context");
    let context = restored
        .get("agent_000001")
        .expect("agent context should be restored");
    assert_eq!(context.preserved_turns.len(), 1);
    assert_eq!(context.preserved_turns[0].user_prompt, "completed question");
    assert_eq!(
        context.preserved_turns[0].assistant_response,
        "completed answer"
    );
    assert_eq!(
        context.preserved_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
}

pub(super) fn restore_provider_context_from_history_rejects_checkpoint_metadata_mismatch() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_restore_checkpoint_metadata_mismatch";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000010.json".to_string();
    let checkpoint_path = run_dir.join(&checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().expect("checkpoint parent"))
        .expect("create checkpoint directory");
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&ProviderContextCheckpoint {
            metadata: ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_000010".to_string(),
                agent_id: "agent_000001".to_string(),
                run_id: "wrong_run".to_string(),
                through_seq: 8,
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
        .expect("serialize checkpoint"),
    )
    .expect("write checkpoint artifact");

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
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel,
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
                3,
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
        ],
    );

    let err = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect_err("restore should reject mismatched checkpoint metadata");
    assert!(matches!(err, CoordinatorError::ResumeRestoreFailed { .. }));
}
