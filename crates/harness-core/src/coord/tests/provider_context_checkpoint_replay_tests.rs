use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn replay_equivalence_after_failed_turn_pre_prompt_compaction_resume() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_replay_equivalence_failed_pre_prompt";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000012.json";
    let checkpoint_path = run_dir.join(checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().unwrap_or_abort()).unwrap_or_abort();
    let checkpoint = ProviderContextCheckpoint {
        metadata: ProviderContextCheckpointMetadata {
            checkpoint_id: "checkpoint_000012".to_string(),
            agent_id: "agent_000001".to_string(),
            run_id: run_id.to_string().into(),
            through_seq: 11,
            through_request_id: Some("req_000002".to_string()),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            tokens_before: None,
            tokens_before_estimate: Some(12_000),
            tokens_after_estimate: Some(3_200),
            summary_tokens_estimate: Some(600),
            compacted_turns: Some(1),
            preserved_turns: Some(1),
            reduction_tokens_estimate: Some(8_800),
            reduction_percent_estimate: Some(73),
            trigger_reason: Some("pre_prompt".to_string()),
        },
        summary: "## Goal\n- Continue after mixed success/failure compaction.\n\n## Constraints\n- Preserve incomplete turns.\n\n## Progress\n- First turn completed.\n\n## Key Decisions\n- Replay loads checkpoint artifacts only.\n\n## Next Steps\n1. Resume from checkpoint.\n\n## Critical Context\n- Successful first turn was compacted.".to_string(),
        recent_turns: vec![ProviderConversationTurn {
            user_prompt: "partial failing question".to_string(),
            assistant_response: "partial provider answer before error".to_string(),
            status: ProviderConversationTurnStatus::Failed,
            failure_stage: Some("provider_error".to_string()),
            failure_reason: Some("provider exploded".to_string()),
            request_id: Some("req_000002".into()),
            first_seq: Some(6),
            last_seq: Some(9),
            ..ProviderConversationTurn::default()
        }],
        pruned_tool_artifacts: Vec::new(),
        facts: ProviderCompactionFacts {
            compacted_turns: vec![crate::agent::ProviderCompactionTurnFact {
                request_id: Some("req_000001".into()),
                first_seq: Some(2),
                last_seq: Some(5),
                user_excerpt: "first successful question".to_string(),
                assistant_excerpt: "first successful answer".to_string(),
                ..Default::default()
            }],
            read_files: vec![ProviderFileOperationFact {
                path: "src/read_before_failure.rs".to_string(),
                operation: "read".to_string(),
                first_seq: Some(4),
                last_seq: Some(4),
                sources: vec!["tool:toolcall_read".to_string()],
                summary: Some("read before failure".to_string()),
            }],
            modified_files: vec![ProviderFileOperationFact {
                path: "src/modified_before_failure.rs".to_string(),
                operation: "modified".to_string(),
                first_seq: Some(5),
                last_seq: Some(5),
                sources: vec!["edit:edit_000001".to_string()],
                summary: Some("modified before failure".to_string()),
            }],
            touched_files: vec![
                "src/modified_before_failure.rs".to_string(),
                "src/read_before_failure.rs".to_string(),
            ],
            operation_facts: vec![
                "read src/read_before_failure.rs via tool:toolcall_read".to_string(),
                "modified src/modified_before_failure.rs via edit:edit_000001".to_string(),
            ],
            ..ProviderCompactionFacts::default()
        },
        tail_boundary: Some(ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 700,
            preserved_from_request_id: Some("req_000002".to_string()),
            preserved_from_seq: Some(6),
            split_prefix_summary: None,
            note: None,
        }),
        summary_source: Some(ProviderCompactionSummarySource {
            strategy: "deterministic_rolling_summary".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            previous_summary_used: false,
            model_backed: false,
            deterministic_fallback: true,
            summary_contract_version: Some(crate::coord::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
            summary_contract_enforced: Some(true),
        }),
        timeline_entry: None,
    };
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&checkpoint).unwrap_or_abort(),
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
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                2,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".into(),
                    text: "first successful question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "first successful question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_read".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("read before failure".to_string()),
                    output_digest: Some("digest-read".to_string()),
                    output_json: Some(json!({ "path": "src/read_before_failure.rs" })),
                    metadata: Some(tool_metadata("read")),
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string().into(),
                    result_summary: "first successful answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000002".into(),
                    text: "partial failing question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".into(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "partial failing question".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_000002".into(),
                    delta: "partial provider answer before error".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                9,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000002".into(),
                    finish_reason: "error".to_string(),
                    output_digest: None,
                    usage: None,
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                10,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::EditApplied(EditAppliedEvent {
                    edit_id: "edit_000001".to_string(),
                    path: "src/modified_before_failure.rs".to_string(),
                    new_file_digest: "digest-modified".to_string(),
                    diff_rel_path: None,
                    diff_digest: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                11,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000002".to_string().into(),
                    reason: "provider exploded".to_string(),
                    task_scope: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                12,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000012".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.to_string(),
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 2048,
                    trigger_reason: "pre_prompt".to_string(),
                    through_seq: 11,
                    through_request_id: Some("req_000002".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: None,
                    tokens_before_estimate: Some(12_000),
                    tokens_after_estimate: Some(3_200),
                    summary_tokens_estimate: Some(600),
                    compacted_turns: Some(1),
                    reduction_tokens_estimate: Some(8_800),
                    reduction_percent_estimate: Some(73),
                    estimate_source: Some("estimated_context_and_prompt".to_string()),
                    summary_source: None,
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                13,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000012".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 11,
                    through_request_id: Some("req_000002".to_string()),
                    tokens_before_estimate: Some(12_000),
                    tokens_after_estimate: Some(3_200),
                    summary_tokens_estimate: Some(600),
                    compacted_turns: Some(1),
                    preserved_turns: Some(1),
                    reduction_tokens_estimate: Some(8_800),
                    reduction_percent_estimate: Some(73),
                    estimate_source: Some("estimated_context_and_prompt".to_string()),
                }),
            ),
        ],
    );

    let expected_context = ProviderContext::from_checkpoint(checkpoint.clone());
    let restored = restore_provider_context_from_history(temp_dir.path(), run_id).unwrap_or_abort();
    let restored_context = restored.get("agent_000001").unwrap_or_abort();

    assert_eq!(
        restored_context.compacted_summary,
        expected_context.compacted_summary
    );
    let restored_summary = restored_context
        .compacted_summary
        .as_deref()
        .unwrap_or_abort();
    assert!(restored_summary.contains("src/read_before_failure.rs"));
    assert!(restored_summary.contains("src/modified_before_failure.rs"));
    assert_eq!(
        restored_context.preserved_turns,
        expected_context.preserved_turns
    );
    assert_eq!(restored_context.preserved_turns.len(), 1);
    assert_eq!(
        restored_context.preserved_turns[0].status,
        ProviderConversationTurnStatus::Failed
    );
    assert_eq!(
        restored_context.preserved_turns[0].failure_stage.as_deref(),
        Some("provider_error")
    );
    assert_eq!(
        restored_context.checkpoint, expected_context.checkpoint,
        "checkpoint metadata should replay exactly"
    );
    assert_eq!(checkpoint.facts.read_files.len(), 1);
    assert_eq!(checkpoint.facts.modified_files.len(), 1);
}
