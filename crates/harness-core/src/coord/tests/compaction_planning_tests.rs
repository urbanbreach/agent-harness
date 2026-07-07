use super::*;
use crate::UnwrapOrAbort;

#[path = "compaction_planning_checkpoint_tests.rs"]
mod compaction_planning_checkpoint_tests;
pub(super) use compaction_planning_checkpoint_tests::{
    proactive_compaction_records_pruned_tool_artifacts_for_compacted_turns as checkpoint_proactive_compaction_records_pruned_tool_artifacts_for_compacted_turns,
    proactive_compaction_writes_checkpoint_artifact_and_updates_provider_context as checkpoint_proactive_compaction_writes_checkpoint_artifact_and_updates_provider_context,
};

pub(super) fn provider_context_compaction_request_returns_none_for_single_turn_manual_context() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_request_noop");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![long_turn("only question", 'A')]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000001".to_string()),
        trigger_reason: "manual".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let summary_decision = crate::coord::CompactionSummaryDecision::deterministic(&trigger);

    // act
    let decision = ProviderContextCompactionRequest::new(
        &run_state,
        trigger,
        &CompactionRuntimeConfig::default(),
        &summary_decision,
    )
    .plan(&redactor);

    // assert
    assert!(decision.is_none());
    assert!(read_events(&run_state.info.events_path).is_empty());
}

pub(super) fn provider_context_compaction_request_builds_checkpoint_decision_without_appending_events(
) {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_request_plan");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("planned first question", 'A'),
            long_turn("planned second question", 'B'),
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "pre_prompt".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: Some(512),
        estimate_source: None,
    };
    let summary_decision = crate::coord::CompactionSummaryDecision::deterministic(&trigger);

    // act
    let decision = ProviderContextCompactionRequest::new(
        &run_state,
        trigger,
        &CompactionRuntimeConfig::default(),
        &summary_decision,
    )
    .plan(&redactor)
    .unwrap_or_abort();

    // assert
    assert_eq!(
        decision.trigger.estimate_source.as_deref(),
        Some("estimated_context_and_prompt")
    );
    assert_eq!(decision.checkpoint.metadata.agent_id, "agent_000001");
    assert_eq!(decision.checkpoint.metadata.through_seq, 0);
    assert_eq!(decision.checkpoint.recent_turns.len(), 1);
    assert_eq!(
        decision.checkpoint.recent_turns[0].user_prompt,
        "planned second question"
    );
    assert_eq!(decision.checkpoint.facts.compacted_turns.len(), 1);
    assert!(decision.tokens_after_estimate < decision.tokens_before_estimate);
    assert!(decision.updated_context.compacted_summary.is_some());
    assert!(read_events(&run_state.info.events_path).is_empty());
}

pub(super) fn compaction_trigger_pre_prompt_uses_estimate_without_provider_usage() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_pre_prompt_estimate");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "pre-prompt first question".to_string(),
                assistant_response: "A".repeat(12_000),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "pre-prompt second question".to_string(),
                assistant_response: "B".repeat(12_000),
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
        through_request_id: Some("req_000003".to_string()),
        trigger_reason: "pre_prompt".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: Some(512),
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
    assert_eq!(written.trigger_reason, "pre_prompt");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).unwrap_or_abort();
    let checkpoint_json: serde_json::Value =
        serde_json::from_str(&checkpoint_body).unwrap_or_abort();
    assert_eq!(
        checkpoint_json
            .get("estimate_source")
            .and_then(serde_json::Value::as_str),
        Some("estimated_context_and_prompt")
    );
}

pub(super) fn compaction_trigger_uses_fallback_budget_without_model_metadata() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_fallback_budget");
    run_state.recorded_runtime_context = Some(RecordedRuntimeContext::from_profile_model(
        "no_metadata_profile",
        "mock:model-1",
    ));
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "fallback first question".to_string(),
                assistant_response: "A".repeat(12_000),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "fallback second question".to_string(),
                assistant_response: "B".repeat(12_000),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "no_metadata_profile".to_string(),
        model_ref: "mock:model-1".to_string(),
        provider_id: None,
        model_id: None,
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "proactive".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        fallback_input_tokens: 2_000,
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
    let checkpoint_json: serde_json::Value =
        serde_json::from_str(&checkpoint_body).unwrap_or_abort();
    assert_eq!(
        checkpoint_json
            .get("estimate_source")
            .and_then(serde_json::Value::as_str),
        Some("fallback_budget")
    );
}

pub(super) fn compaction_trigger_noops_below_estimated_threshold() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_below_estimated_threshold");
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "short first question".to_string(),
                assistant_response: "short first answer".to_string(),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "short second question".to_string(),
                assistant_response: "short second answer".to_string(),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "mock:model-1".to_string(),
        provider_id: None,
        model_id: None,
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "proactive".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: None,
        estimate_source: None,
    };

    let result = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &crate::coord::CompactionSummaryDecision::deterministic(&trigger),
    )
    .unwrap_or_abort();

    assert!(result.is_none());
    let events = read_events(&run_state.info.events_path);
    assert!(events.iter().all(|event| {
        !matches!(
            event.payload,
            EventV1::CompactionRequested(_)
                | EventV1::CompactionWritten(_)
                | EventV1::CompactionApplied(_)
        )
    }));
}

pub(super) fn structured_summary_contract_can_be_disabled_for_legacy_headings() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_legacy_contract");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("legacy first question", 'A'),
            long_turn("legacy second question", 'B'),
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
    let compaction_config = CompactionRuntimeConfig {
        structured_summary_contract: false,
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
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).unwrap_or_abort();

    for heading in provider_context_summary_required_headings(&compaction_config) {
        assert!(
            checkpoint.summary.contains(heading),
            "legacy summary missing structured heading {heading}:\n{}",
            checkpoint.summary
        );
    }
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_enforced),
        Some(false)
    );
}

pub(super) fn deterministic_summary_uses_required_harness_sections() {
    let summary_source = ProviderCompactionSummarySource {
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
    };
    let config = CompactionRuntimeConfig::default();

    let summary = build_provider_context_summary(
        None,
        &[ProviderConversationTurn {
            user_prompt: "first compacted question".to_string(),
            assistant_response: "first compacted answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        &[],
        &ProviderCompactionFacts::default(),
        &ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 10,
            preserved_from_request_id: None,
            preserved_from_seq: None,
            split_prefix_summary: None,
            note: None,
        },
        &summary_source,
        &config,
    );

    for heading in provider_context_summary_required_headings(&config) {
        assert!(
            summary.contains(heading),
            "Harness summary missing required heading {heading}:\n{summary}"
        );
    }
    let legacy_config = CompactionRuntimeConfig {
        structured_summary_contract: false,
        ..CompactionRuntimeConfig::default()
    };
    assert!(!summary.contains(provider_context_summary_required_headings(&legacy_config)[1]));
    assert!(!summary.contains(provider_context_summary_required_headings(&legacy_config)[9]));
    assert!(summary.contains("first compacted question"));
}

pub(super) fn model_summary_validation_rejects_missing_required_harness_section() {
    let config = CompactionRuntimeConfig::default();
    let plan = provider_context_compaction_plan_fixture();
    let omitted_heading = provider_context_summary_required_headings(&config)
        .last()
        .copied()
        .unwrap_or_abort();
    let mut headings = provider_context_summary_required_headings(&config)
        .iter()
        .copied()
        .filter(|heading| *heading != omitted_heading)
        .map(|heading| format!("{heading}\n- content"))
        .collect::<Vec<_>>()
        .join("\n\n");
    headings.push_str("\n\ncompact enough");

    let err = validate_model_compaction_summary(&headings, 20_000, &plan, &config)
        .expect_err("summary missing a Harness heading must be rejected");

    assert!(err.contains(omitted_heading));
    let prompt = build_model_compaction_prompt(None, &plan, "draft", &config);
    for heading in provider_context_summary_required_headings(&config) {
        assert!(prompt.contains(heading));
    }
}

pub(super) fn repeated_compaction_updates_existing_summary_without_legacy_append_format() {
    let summary = build_provider_context_summary(
        Some("## Goal\n- Keep existing constraints\n## Next Steps\n1. Continue"),
        &[ProviderConversationTurn {
            user_prompt: "new compacted question".to_string(),
            assistant_response: "new compacted answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        &[],
        &ProviderCompactionFacts::default(),
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
            previous_summary_used: true,
            model_backed: false,
            deterministic_fallback: true,
            summary_contract_version: Some(crate::coord::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
            summary_contract_enforced: Some(true),
        },
        &CompactionRuntimeConfig::default(),
    );

    assert!(summary.contains("## Goal"));
    assert!(!summary.contains("Previous Summary"));
    assert!(summary.contains("Keep existing constraints"));
    assert!(summary.contains("new compacted question"));
    assert!(!summary.contains("Earlier checkpoint summary:"));
}

pub(super) fn compaction_summary_override_uses_explicit_hook_prefix_only() {
    let batch = HookExecutionBatch {
        hook_executions: vec![
            HookExecutionMetadata {
                hook_name: "ignored".to_string(),
                status: HookExecutionStatus::Succeeded,
                hook_event: Some("compaction_requested".to_string()),
                command_digest: None,
                output_digest: None,
                output_summary: Some("ordinary hook output".to_string()),
                duration_ms: Some(1),
            },
            HookExecutionMetadata {
                hook_name: "summary".to_string(),
                status: HookExecutionStatus::Succeeded,
                hook_event: Some("compaction_requested".to_string()),
                command_digest: None,
                output_digest: None,
                output_summary: Some("compaction_summary: custom compacted recap".to_string()),
                duration_ms: Some(1),
            },
        ],
        critical_failure: None,
    };

    assert_eq!(
        compaction_summary_override_from_hooks(&batch.hook_executions).as_deref(),
        Some("custom compacted recap")
    );
}

fn provider_context_compaction_plan_fixture() -> ProviderContextCompactionPlan {
    ProviderContextCompactionPlan {
        older_turns: vec![ProviderConversationTurn {
            user_prompt: "model validation compacted question".to_string(),
            assistant_response: "model validation compacted answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        recent_turns: vec![ProviderConversationTurn {
            user_prompt: "recent question".to_string(),
            assistant_response: "recent answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        pruned_tool_artifacts: Vec::new(),
        facts: ProviderCompactionFacts::default(),
        tail_boundary: ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 10,
            preserved_from_request_id: None,
            preserved_from_seq: None,
            split_prefix_summary: None,
            note: None,
        },
    }
}
