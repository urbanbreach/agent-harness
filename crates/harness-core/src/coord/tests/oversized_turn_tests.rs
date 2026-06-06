use super::*;

pub(super) fn split_oversized_turn_pre_prompt_preserves_suffix_and_prefix_summary() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_pre_prompt_prefix_summary");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    let oversized_answer = format!(
        "PREFIX_ANCHOR {} {} SUFFIX_ANCHOR",
        "A".repeat(4_000),
        "B".repeat(7_900)
    );
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "earlier question".to_string(),
                assistant_response: "earlier answer".to_string(),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "latest oversized question".to_string(),
                assistant_response: oversized_answer,
                request_id: Some("req_latest".to_string()),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_latest".to_string()),
        trigger_reason: "pre_prompt".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    let updated = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("pre-prompt split compaction should succeed")
    .expect("pre-prompt split compaction should write checkpoint");

    assert_eq!(updated.updated_context.preserved_turns.len(), 1);
    let recent = &updated.updated_context.preserved_turns[0];
    assert!(recent.assistant_response.contains("SUFFIX_ANCHOR"));
    assert!(!recent.assistant_response.contains("PREFIX_ANCHOR"));
    assert!(recent
        .user_prompt
        .contains("earlier prefix is summarized in the checkpoint"));

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "pre_prompt" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("pre-prompt compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    let tail_boundary = checkpoint.tail_boundary.expect("tail boundary");
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    let prefix_summary = tail_boundary
        .split_prefix_summary
        .expect("split prefix summary");
    assert!(prefix_summary.contains("PREFIX_ANCHOR"));
    assert!(checkpoint.summary.contains("## Critical Context"));
    assert!(checkpoint.summary.contains("Split prefix summary"));
    assert!(checkpoint
        .summary
        .contains("Source facts: split prefix summary"));
}

pub(super) fn split_oversized_failed_provider_error_preserves_incomplete_suffix() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_failed_provider_error");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    let oversized_answer = format!(
        "FAILED_PREFIX {} {} FAILED_SUFFIX",
        "C".repeat(4_000),
        "D".repeat(7_900)
    );
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("earlier successful question", 'A'),
            ProviderConversationTurn {
                user_prompt: "failed latest question".to_string(),
                assistant_response: oversized_answer,
                status: ProviderConversationTurnStatus::Failed,
                failure_stage: Some("provider_error".to_string()),
                failure_reason: Some("provider exploded".to_string()),
                request_id: Some("req_failed".to_string()),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_failed".to_string()),
        trigger_reason: "failed_response".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("failed-response split compaction should succeed")
    .expect("failed-response split compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "failed_response" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("failed-response compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert_eq!(checkpoint.recent_turns.len(), 1);
    let suffix = &checkpoint.recent_turns[0];
    assert_eq!(suffix.status, ProviderConversationTurnStatus::Failed);
    assert_eq!(suffix.failure_stage.as_deref(), Some("provider_error"));
    assert_eq!(suffix.failure_reason.as_deref(), Some("provider exploded"));
    assert!(suffix.assistant_response.contains("FAILED_SUFFIX"));
    assert!(!suffix.assistant_response.contains("FAILED_PREFIX"));
    assert!(suffix
        .user_prompt
        .contains("earlier prefix is summarized in the checkpoint"));
    assert!(checkpoint
        .tail_boundary
        .as_ref()
        .and_then(|boundary| boundary.split_prefix_summary.as_deref())
        .is_some_and(|summary| summary.contains("FAILED_PREFIX")));
}

pub(super) fn split_oversized_turn_refuses_tool_failure_to_avoid_orphan_tools() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_refuses_tool_failure");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("earlier successful question", 'A'),
            ProviderConversationTurn {
                user_prompt: "tool failure latest question".to_string(),
                assistant_response: format!(
                    "TOOL_PREFIX {} {} TOOL_SUFFIX",
                    "E".repeat(4_000),
                    "F".repeat(7_900)
                ),
                status: ProviderConversationTurnStatus::Failed,
                failure_stage: Some("tool_failure".to_string()),
                failure_reason: Some("tool failed closed".to_string()),
                request_id: Some("req_tool_failed".to_string()),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_tool_failed".to_string()),
        trigger_reason: "failed_response".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("tool-failure summary-only compaction should succeed")
    .expect("tool-failure summary-only compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "failed_response" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("failed-response compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert!(checkpoint.recent_turns.is_empty());
    let tail_boundary = checkpoint.tail_boundary.expect("tail boundary");
    assert_eq!(tail_boundary.mode, "summary_only");
    assert_eq!(tail_boundary.split_prefix_summary, None);
    assert!(checkpoint.facts.compacted_turns.iter().any(|fact| {
        fact.status == ProviderConversationTurnStatus::Failed
            && fact.failure_stage.as_deref() == Some("tool_failure")
    }));
}

pub(super) fn split_oversized_turn_refuses_artifact_backed_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_refuses_artifact_turn");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![ProviderConversationTurn {
            user_prompt: "artifact backed latest question".to_string(),
            assistant_response: format!(
                "ARTIFACT_PREFIX {} {} ARTIFACT_SUFFIX",
                "G".repeat(4_000),
                "H".repeat(7_900)
            ),
            request_id: Some("req_artifact".to_string()),
            artifacts: vec![EventArtifactRef {
                path: "artifacts/toolcalls/toolcall_000001/result.txt".to_string(),
                digest: Some("digest-artifact".to_string()),
            }],
            ..ProviderConversationTurn::default()
        }]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_artifact".to_string()),
        trigger_reason: "overflow_retry".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("artifact-backed summary-only compaction should succeed")
    .expect("artifact-backed summary-only compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "overflow_retry" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("overflow compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert!(checkpoint.recent_turns.is_empty());
    let tail_boundary = checkpoint.tail_boundary.expect("tail boundary");
    assert_eq!(tail_boundary.mode, "summary_only");
    assert_eq!(tail_boundary.split_prefix_summary, None);
    assert!(checkpoint
        .facts
        .relevant_artifacts
        .iter()
        .any(|artifact| { artifact.path == "artifacts/toolcalls/toolcall_000001/result.txt" }));
}

pub(super) fn split_oversized_turn_refuses_provider_neutral_tool_messages() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_refuses_neutral_tool_turn");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    let assistant_response = format!(
        "NEUTRAL_TOOL_PREFIX {} {} NEUTRAL_TOOL_SUFFIX",
        "K".repeat(4_000),
        "L".repeat(7_900)
    );
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![ProviderConversationTurn {
            user_prompt: "neutral tool latest question".to_string(),
            assistant_response: assistant_response.clone(),
            request_id: Some("req_neutral_tool".to_string()),
            messages: vec![
                ConversationMessage::User(ConversationUserMessage {
                    request_id: "req_neutral_tool".to_string(),
                    text: "neutral tool latest question".to_string(),
                    seq: None,
                    agent_id: Some("agent_000001".to_string()),
                }),
                ConversationMessage::Assistant(ConversationAssistantMessage {
                    request_id: "req_neutral_tool".to_string(),
                    agent_id: Some("agent_000001".to_string()),
                    text: String::new(),
                    tool_calls: vec![ConversationToolCall {
                        tool_call_id: "toolcall_neutral".to_string(),
                        tool_id: "shell.run".to_string(),
                        args_summary: r#"{"command":"true"}"#.to_string(),
                        args_digest: "digest-args".to_string(),
                        seq: None,
                        metadata: None,
                    }],
                    stop_reason: None,
                    first_seq: None,
                    last_seq: None,
                    provider_id: None,
                    model_id: None,
                    output_digest: None,
                }),
                ConversationMessage::ToolResult(Box::new(ConversationToolResultMessage {
                    request_id: "req_neutral_tool".to_string(),
                    tool_call_id: "toolcall_neutral".to_string(),
                    tool_id: Some("shell.run".to_string()),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("ok".to_string()),
                    output_digest: Some("digest-output".to_string()),
                    output_json: None,
                    seq: None,
                    metadata: None,
                })),
                ConversationMessage::Assistant(ConversationAssistantMessage {
                    request_id: "req_neutral_tool".to_string(),
                    agent_id: Some("agent_000001".to_string()),
                    text: assistant_response,
                    tool_calls: Vec::new(),
                    stop_reason: None,
                    first_seq: None,
                    last_seq: None,
                    provider_id: None,
                    model_id: None,
                    output_digest: None,
                }),
            ],
            ..ProviderConversationTurn::default()
        }]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_neutral_tool".to_string()),
        trigger_reason: "overflow_retry".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("neutral tool summary-only compaction should succeed")
    .expect("neutral tool summary-only compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "overflow_retry" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("overflow compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert!(checkpoint.recent_turns.is_empty());
    let tail_boundary = checkpoint.tail_boundary.expect("tail boundary");
    assert_eq!(tail_boundary.mode, "summary_only");
    assert_eq!(tail_boundary.split_prefix_summary, None);
}

pub(super) fn split_oversized_turn_prefix_summary_in_checkpoint_facts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_prefix_summary_facts");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![ProviderConversationTurn {
            user_prompt: "latest oversized facts question".to_string(),
            assistant_response: format!(
                "FACT_PREFIX_ANCHOR {} {} FACT_SUFFIX_ANCHOR",
                "I".repeat(4_000),
                "J".repeat(7_900)
            ),
            request_id: Some("req_fact".to_string()),
            ..ProviderConversationTurn::default()
        }]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_fact".to_string()),
        trigger_reason: "overflow_retry".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("overflow split compaction should succeed")
    .expect("overflow split compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "overflow_retry" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("overflow compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint_json: serde_json::Value =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact json");
    assert_eq!(
        checkpoint_json["tail_boundary"]["mode"].as_str(),
        Some("split_oversized_turn_tail")
    );
    let split_prefix_summary = checkpoint_json["tail_boundary"]["split_prefix_summary"]
        .as_str()
        .expect("serialized split prefix summary");
    assert!(split_prefix_summary.contains("FACT_PREFIX_ANCHOR"));
    let summary = checkpoint_json["summary"].as_str().expect("summary");
    assert!(summary.contains("Split prefix summary"));
    assert!(summary.contains("Source facts: split prefix summary"));
    assert!(!checkpoint_json["recent_turns"][0]["assistant_response"]
        .as_str()
        .expect("recent assistant suffix")
        .contains("FACT_PREFIX_ANCHOR"));
}
