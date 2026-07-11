use super::*;
use crate::UnwrapOrAbort;

#[path = "operational_memory_context_tests.rs"]
mod operational_memory_context_tests;
pub(super) use operational_memory_context_tests::{
    compaction_preserves_file_tool_skill_todo_and_plan_context as context_compaction_preserves_file_tool_skill_todo_and_plan_context,
    operational_memory_records_read_and_modified_files_from_events as context_operational_memory_records_read_and_modified_files_from_events,
};

pub(super) fn operational_memory_redacts_secret_shaped_facts() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    let raw_secret_path = "src/sk-AbCdEf0123456789-token.rs";
    let raw_bearer = "Bearer abc.def-ghi_123";
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_redacts_secrets",
        &operational_memory_history_events(
            "run_operational_memory_redacts_secrets",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_secret_read".into(),
                tool_id: "read".to_string(),
                args_summary: format!("read {raw_secret_path}"),
                args_digest: "digest-secret-read".to_string(),
                metadata: Some(tool_metadata("read")),
            })],
            vec![EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_secret_read".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(format!("read token-like path with {raw_bearer}")),
                output_digest: Some("digest-secret-output".to_string()),
                output_json: Some(json!({ "path": raw_secret_path })),
                metadata: Some(tool_metadata("read")),
            })],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_redacts_secrets",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );
    let checkpoint_json = serde_json::to_string(&checkpoint).unwrap_or_abort();

    assert!(!checkpoint_json.contains("sk-AbCdEf0123456789"));
    assert!(!checkpoint_json.contains("Bearer abc.def-ghi_123"));
    assert!(checkpoint_json.contains("[REDACTED_API_KEY]"));
    assert!(checkpoint_json.contains("Bearer [REDACTED]"));
    assert!(checkpoint
        .facts
        .read_files
        .iter()
        .all(|fact| !fact.path.contains("sk-AbCdEf0123456789")));
    assert!(checkpoint
        .facts
        .operation_facts
        .iter()
        .all(|fact| !fact.contains("Bearer abc.def-ghi_123")));
    assert!(!checkpoint.summary.contains("sk-AbCdEf0123456789"));
    assert!(!checkpoint.summary.contains("Bearer abc.def-ghi_123"));
}

pub(super) fn operational_memory_dedupes_sorts_and_caps_paths() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    let matches = (0..55)
        .rev()
        .map(|index| json!({ "path": format!("src/file_{index:03}.rs") }))
        .chain(std::iter::once(json!({ "path": "src/file_000.rs" })))
        .collect::<Vec<_>>();
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_caps",
        &operational_memory_history_events(
            "run_operational_memory_caps",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_grep".into(),
                tool_id: "grep".to_string(),
                args_summary: "grep files".to_string(),
                args_digest: "digest-grep".to_string(),
                metadata: Some(tool_metadata("grep")),
            })],
            vec![EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_grep".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("grep completed".to_string()),
                output_digest: Some("digest-grep-output".to_string()),
                output_json: Some(json!({
                    "matches": matches,
                    "path": "/outside/workspace/ignored.rs"
                })),
                metadata: Some(tool_metadata("grep")),
            })],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_caps",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );

    assert_eq!(checkpoint.facts.read_files.len(), 50);
    assert_eq!(checkpoint.facts.read_files[0].path, "src/file_000.rs");
    assert_eq!(checkpoint.facts.read_files[49].path, "src/file_049.rs");
    assert!(checkpoint
        .facts
        .read_files
        .iter()
        .all(|fact| fact.operation == "read"));
    assert_eq!(checkpoint.facts.touched_files.len(), 50);
    assert!(checkpoint
        .facts
        .operation_facts
        .iter()
        .any(|fact| fact == "5 additional read file(s) omitted"));
}

pub(super) fn operational_memory_ignores_freeform_path_like_output() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_freeform",
        &operational_memory_history_events(
            "run_operational_memory_freeform",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_read".into(),
                tool_id: "read".to_string(),
                args_summary: "read output".to_string(),
                args_digest: "digest-read".to_string(),
                metadata: Some(tool_metadata("read")),
            })],
            vec![EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_read".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(
                    "free-form text mentions src/freeform.rs and /workspace/project/src/secret.rs"
                        .to_string(),
                ),
                output_digest: Some("digest-output".to_string()),
                output_json: Some(json!({ "message": "src/not-a-path-field.rs" })),
                metadata: Some(tool_metadata("read")),
            })],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_freeform",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );

    assert!(checkpoint.facts.read_files.is_empty());
    assert!(checkpoint.facts.modified_files.is_empty());
    assert!(checkpoint.facts.touched_files.is_empty());
}

pub(super) fn operational_memory_preserves_touched_files_legacy_union() {
    let facts = ProviderCompactionFacts {
        read_files: vec![ProviderFileOperationFact {
            path: "src/read.rs".to_string(),
            operation: "read".to_string(),
            first_seq: Some(2),
            last_seq: Some(2),
            sources: vec!["tool:read".to_string()],
            summary: None,
        }],
        modified_files: vec![ProviderFileOperationFact {
            path: "src/modified.rs".to_string(),
            operation: "modified".to_string(),
            first_seq: Some(3),
            last_seq: Some(3),
            sources: vec!["edit:edit".to_string()],
            summary: None,
        }],
        ..ProviderCompactionFacts::default()
    };

    let summary = build_provider_context_summary(
        None,
        &[ProviderConversationTurn {
            user_prompt: "question".to_string(),
            assistant_response: "answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        &[],
        &facts,
        &ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 10,
            preserved_from_request_id: None,
            preserved_from_seq: None,
            split_prefix_summary: None,
            note: None,
        },
        &ProviderCompactionSummarySource {
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
        },
        &CompactionRuntimeConfig::default(),
    );

    assert!(summary.contains("src/read.rs"));
    assert!(summary.contains("src/modified.rs"));
}

pub(super) fn operational_memory_resume_loads_checkpoint_facts_without_filesystem_scan() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_operational_memory_resume";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000010.json".to_string();
    let checkpoint_path = run_dir.join(&checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&ProviderContextCheckpoint {
            metadata: checkpoint_metadata_for_run(run_id, "checkpoint_000010", 9),
            summary: "Earlier checkpoint summary".to_string(),
            recent_turns: Vec::new(),
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts {
                read_files: vec![ProviderFileOperationFact {
                    path: "src/restored_read.rs".to_string(),
                    operation: "read".to_string(),
                    first_seq: Some(4),
                    last_seq: Some(4),
                    sources: vec!["tool:toolcall_read".to_string()],
                    summary: Some("read source file".to_string()),
                }],
                modified_files: vec![ProviderFileOperationFact {
                    path: "src/restored_modified.rs".to_string(),
                    operation: "modified".to_string(),
                    first_seq: Some(5),
                    last_seq: Some(5),
                    sources: vec!["edit:edit_000001".to_string()],
                    summary: Some("modified source file".to_string()),
                }],
                touched_files: vec![
                    "src/restored_modified.rs".to_string(),
                    "src/restored_read.rs".to_string(),
                ],
                operation_facts: vec!["restored operational fact".to_string()],
                ..ProviderCompactionFacts::default()
            },
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
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                2,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel,
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 100,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000001".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: None,
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                    summary_source: None,
                    preserved_turns: 0,
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 9,
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
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id).unwrap_or_abort();
    let summary = restored
        .get("agent_000001")
        .and_then(|context| context.compacted_summary.as_deref())
        .unwrap_or_abort();
    assert!(summary.contains("## Operational Memory"));
    assert!(summary.contains("src/restored_read.rs"));
    assert!(summary.contains("src/restored_modified.rs"));
    assert!(summary.contains("restored operational fact"));
}

fn compact_operational_memory_fixture(
    session_dir: &Path,
    run_id: &str,
    first_answer: &str,
    second_answer: &str,
    clock: &FakeClock,
    redactor: &DefaultRedactor,
) -> ProviderContextCheckpoint {
    let mut run_state = test_run_state(session_dir, run_id);
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "first question".to_string(),
                assistant_response: first_answer.to_string(),
                request_id: Some("req_000001".into()),
                first_seq: Some(2),
                last_seq: None,
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "second question".to_string(),
                assistant_response: second_answer.to_string(),
                request_id: Some("req_000002".into()),
                first_seq: None,
                last_seq: None,
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    run_state.next_event_seq = read_events(&run_state.info.events_path)
        .last()
        .map(|event| event.seq.saturating_add(1))
        .unwrap_or(1);
    let trigger = ProviderCompactionTrigger {
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
    };
    compact_provider_context(
        clock,
        redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .unwrap_or_abort()
    .unwrap_or_abort();

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
    serde_json::from_str(&checkpoint_body).unwrap_or_abort()
}

fn operational_memory_history_events(
    run_id: &str,
    first_answer: &str,
    second_answer: &str,
    before_finish: Vec<EventV1>,
    after_finish: Vec<EventV1>,
) -> Vec<EventEnvelopeV1> {
    let mut events = vec![
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
                text: "first question".to_string(),
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
                prompt_summary: "first question".to_string(),
                request_digest: "digest-1".to_string(),
                metadata: None,
            }),
        ),
    ];
    let mut seq = 4_u64;
    for payload in before_finish.into_iter().chain(after_finish) {
        events.push(restore_fixture_event(
            run_id,
            seq,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            payload,
        ));
        seq += 1;
    }
    events.push(restore_fixture_event(
        run_id,
        seq,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        Some("req_000001"),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string().into(),
            result_summary: first_answer.to_string(),
            result_digest: "digest-task-1".to_string(),
            metadata: None,
        }),
    ));
    seq += 1;
    events.push(restore_fixture_event(
        run_id,
        seq,
        EventActor::new(ActorKind::User, None),
        None,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_000002".into(),
            text: "second question".to_string(),
        }),
    ));
    seq += 1;
    events.push(restore_fixture_event(
        run_id,
        seq,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        Some("req_000002"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_000002".into(),
            provider_id: "default".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "second question".to_string(),
            request_digest: "digest-2".to_string(),
            metadata: None,
        }),
    ));
    seq += 1;
    events.push(restore_fixture_event(
        run_id,
        seq,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        Some("req_000002"),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000002".to_string().into(),
            result_summary: second_answer.to_string(),
            result_digest: "digest-task-2".to_string(),
            metadata: None,
        }),
    ));
    events.sort_by_key(|event| event.seq);
    events
}

fn tool_metadata(canonical_tool_id: &str) -> ToolCallMetadata {
    ToolCallMetadata {
        canonical_tool_id: Some(canonical_tool_id.to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing: None,
        hook_executions: Vec::new(),
    }
}

fn checkpoint_metadata_for_run(
    run_id: &str,
    checkpoint_id: &str,
    through_seq: u64,
) -> ProviderContextCheckpointMetadata {
    ProviderContextCheckpointMetadata {
        checkpoint_id: checkpoint_id.to_string(),
        agent_id: "agent_000001".to_string(),
        run_id: run_id.to_string(),
        through_seq,
        through_request_id: Some("req_000001".to_string()),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        tokens_before: None,
        tokens_before_estimate: None,
        tokens_after_estimate: None,
        summary_tokens_estimate: None,
        compacted_turns: None,
        preserved_turns: None,
        reduction_tokens_estimate: None,
        reduction_percent_estimate: None,
        trigger_reason: Some("proactive".to_string()),
    }
}
