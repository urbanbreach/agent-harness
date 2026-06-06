use super::*;

#[path = "provider_context_checkpoint_replay_tests.rs"]
mod provider_context_checkpoint_replay_tests;
pub(super) use provider_context_checkpoint_replay_tests::replay_equivalence_after_failed_turn_pre_prompt_compaction_resume as replay_replay_equivalence_after_failed_turn_pre_prompt_compaction_resume;

pub(super) fn legacy_provider_context_checkpoint_deserializes() {
    let body = r#"{
        "checkpoint_id": "checkpoint_legacy",
        "agent_id": "agent_000001",
        "run_id": "run_legacy",
        "through_seq": 7,
        "summary": "legacy summary",
        "summary_source": {
            "strategy": "deterministic_rolling_summary",
            "model_ref": "default:model-1",
            "previous_summary_used": false,
            "model_backed": false,
            "deterministic_fallback": true
        },
        "recent_turns": [
            {
                "user_prompt": "legacy question",
                "assistant_response": "legacy answer",
                "provider_messages": [
                    {"role": "assistant", "content": "legacy provider-shaped message"}
                ]
            }
        ]
    }"#;

    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(body).expect("legacy checkpoint should deserialize");

    assert_eq!(checkpoint.metadata.tokens_before_estimate, None);
    assert_eq!(checkpoint.metadata.tokens_after_estimate, None);
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(checkpoint.recent_turns[0].request_id, None);
    assert_eq!(checkpoint.recent_turns[0].artifacts, Vec::new());
    assert_eq!(checkpoint.recent_turns[0].messages, Vec::new());
    let source = checkpoint
        .summary_source
        .as_ref()
        .expect("legacy summary source should deserialize");
    assert_eq!(source.summary_contract_version, None);
    assert_eq!(source.summary_contract_enforced, None);
}

pub(super) fn provider_neutral_reconstruction_marks_continue_as_tool_message_failures() {
    assert_eq!(
        provider_tool_message_status("tool call `shell_run` failed: denied"),
        ToolCallStatus::Failed
    );
    assert_eq!(
        provider_tool_message_status("ok {\"command\":\"true\"}"),
        ToolCallStatus::Succeeded
    );

    let profile = AgentProfile {
        name: "alpha".to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "sys".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: crate::config::ToolFailureMode::ContinueAsToolMessage,
        toolset: vec!["shell.run".to_string()],
    };
    let messages = completion_messages_to_conversation_messages(
        &profile,
        "req_failed_tool",
        "agent_000001",
        &[
            CompletionMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: Some(vec![harness_providers::AssistantToolCall {
                    tool_call_id: "call_failed".to_string(),
                    function_name: "shell_run".to_string(),
                    arguments_json: r#"{"command":"false"}"#.to_string(),
                }]),
            },
            CompletionMessage {
                role: MessageRole::Tool,
                content: "tool call `shell_run` failed: denied".to_string(),
                name: Some("shell_run".to_string()),
                tool_call_id: Some("call_failed".to_string()),
                assistant_tool_calls: None,
            },
        ],
    );

    let ConversationMessage::ToolResult(tool_result) = &messages[1] else {
        panic!("expected tool result message");
    };
    assert_eq!(tool_result.status, ToolCallStatus::Failed);
    assert_eq!(tool_result.tool_id.as_deref(), Some("shell.run"));
}

pub(super) fn provider_context_checkpoint_legacy_round_trips_with_new_defaults() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_legacy_checkpoint_round_trip";
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_legacy.json";
    let checkpoint_path = temp_dir.path().join(run_id).join(checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().expect("checkpoint parent"))
        .expect("create checkpoint directory");
    fs::write(
        &checkpoint_path,
        r#"{
            "checkpoint_id": "checkpoint_legacy",
            "agent_id": "agent_000001",
            "run_id": "run_legacy_checkpoint_round_trip",
            "through_seq": 4,
            "through_request_id": "req_000001",
            "summary": "legacy summary that must survive",
            "summary_source": {
                "strategy": "deterministic_rolling_summary",
                "model_ref": "default:model-1",
                "previous_summary_used": false,
                "model_backed": false,
                "deterministic_fallback": true
            },
            "recent_turns": [
                {
                    "user_prompt": "legacy recent question",
                    "assistant_response": "legacy recent answer",
                    "request_id": "req_000001"
                }
            ]
        }"#,
    )
    .expect("write legacy checkpoint artifact");
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
                Some("compaction:agent_000001"),
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_legacy".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.to_string(),
                    artifact_digest: None,
                    artifact_bytes: 512,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 4,
                    through_request_id: Some("req_000001".to_string()),
                    provider_id: None,
                    model_id: None,
                    tokens_before: None,
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
                Some("compaction:agent_000001"),
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_legacy".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 4,
                    through_request_id: Some("req_000001".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: Some(1),
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore legacy checkpoint context");
    let mut restored_context = restored
        .get("agent_000001")
        .cloned()
        .expect("restored agent context");
    assert_eq!(
        restored_context.compacted_summary.as_deref(),
        Some("legacy summary that must survive")
    );
    assert_eq!(restored_context.preserved_turns.len(), 1);
    assert_eq!(
        restored_context.preserved_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
    restored_context.push_turn(long_turn("new follow-up question", 'N'));

    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_legacy_checkpoint_round_trip_again");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state
        .provider_context_by_agent
        .insert("agent_000001".to_string(), restored_context);
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "manual".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("second compaction should succeed")
    .expect("second compaction should write a checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .expect("new compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(checkpoint_path).expect("read new checkpoint");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse new checkpoint");
    assert!(checkpoint
        .summary
        .contains("legacy summary that must survive"));
    assert!(checkpoint.summary.contains("legacy recent question"));
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(
        checkpoint.recent_turns[0].user_prompt,
        "new follow-up question"
    );
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
    assert_eq!(
        checkpoint.facts.previous_checkpoint_id.as_deref(),
        Some("checkpoint_legacy")
    );
    assert!(checkpoint.facts.read_files.is_empty());
    assert!(checkpoint.facts.modified_files.is_empty());
    assert!(checkpoint.facts.operation_facts.is_empty());
}

pub(super) fn failed_turn_status_defaults_to_completed_for_legacy_checkpoint() {
    let body = r#"{
        "checkpoint_id": "checkpoint_legacy",
        "agent_id": "agent_000001",
        "run_id": "run_legacy",
        "through_seq": 7,
        "summary": "legacy summary",
        "recent_turns": [
            {
                "user_prompt": "legacy question",
                "assistant_response": "legacy answer"
            }
        ],
        "facts": {
            "compacted_turns": [
                {
                    "user_excerpt": "old question",
                    "assistant_excerpt": "old answer"
                }
            ]
        }
    }"#;

    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(body).expect("legacy checkpoint should deserialize");

    assert_eq!(
        checkpoint.recent_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
    assert_eq!(checkpoint.recent_turns[0].failure_stage, None);
    assert_eq!(checkpoint.recent_turns[0].failure_reason, None);
    assert_eq!(
        checkpoint.facts.compacted_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
    let serialized = serde_json::to_value(&checkpoint).expect("serialize checkpoint");
    assert_eq!(
        serialized["recent_turns"][0].get("status"),
        None,
        "completed recent turns should omit status"
    );
    assert_eq!(
        serialized["facts"]["compacted_turns"][0].get("status"),
        None,
        "completed compacted turn facts should omit status"
    );
}

pub(super) fn compaction_turn_facts_include_failed_turn_status() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_failed_fact_status");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "first question".to_string(),
                assistant_response: format!("partial answer {}", "A".repeat(6_000)),
                status: ProviderConversationTurnStatus::Failed,
                failure_stage: Some("provider_error".to_string()),
                failure_reason: Some(format!(
                    "provider failed with sk-ABCDE12345ABCDE {}",
                    "details ".repeat(80)
                )),
                ..ProviderConversationTurn::default()
            },
            long_turn("second question", 'B'),
        ]),
    );

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
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("failed-turn compaction should succeed")
    .expect("failed-turn compaction should write a checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .expect("compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    assert!(!checkpoint_body.contains("sk-ABCDE12345ABCDE"));
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    let compacted_fact = checkpoint
        .facts
        .compacted_turns
        .first()
        .expect("compacted failed turn fact");
    assert_eq!(
        compacted_fact.status,
        ProviderConversationTurnStatus::Failed
    );
    assert_eq!(
        compacted_fact.failure_stage.as_deref(),
        Some("provider_error")
    );
    let reason = compacted_fact
        .failure_reason
        .as_deref()
        .expect("failure reason should be retained");
    assert!(reason.contains("[REDACTED_API_KEY]"));
    assert!(
        reason.chars().count()
            <= crate::coord::PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS + 1
    );
}
