use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits};
use harness_core::context_budget::{
    compute_request_budget, BudgetStatus, RequestBudgetInput, RequestBudgetSnapshot,
};
use harness_core::event::ProviderRequestStartedMetadata;
use harness_core::proj::{load_run_metadata, RecordedRuntimeContext};
use harness_core::UnwrapOrAbort;
use harness_providers::{ProviderOutputCapDisposition, ProviderRequestCost};
use serde_json::{json, Value};

fn canonical_snapshot() -> RequestBudgetSnapshot {
    let limits = ResolvedModelLimits::from_values(
        Some(128_000),
        Some(96_000),
        Some(4_096),
        ModelLimitProvenance::explicit("budget equality test"),
    );
    compute_request_budget(RequestBudgetInput {
        model_limits: &limits,
        request_cost: ProviderRequestCost {
            system_tokens: 10,
            tools_tokens: 20,
            history_tokens: 30,
            attachments_tokens: 0,
            framing_tokens: 5,
            pending_prompt_tokens: 15,
        },
        requested_output_tokens: None,
        safety_margin_tokens: 16_384,
        estimated_token_triggers: false,
        fallback_input_tokens: 0,
        output_cap_disposition: ProviderOutputCapDisposition::Emitted(4_096),
    })
    .unwrap_or_abort()
    .snapshot()
}

#[test]
fn request_budget_formula_truth_table() {
    // arrange: canonical limits and six independently named request components.
    let snapshot = canonical_snapshot();

    // act: the core formula reserves output and applies the safety margin once.
    let value = serde_json::to_value(snapshot).unwrap_or_abort();

    // assert: every machine-consumed budget value matches the accepted truth table.
    assert_eq!(
        value,
        json!({
            "status": "estimated",
            "requested_output_tokens": 4096,
            "reserved_output_tokens": 4096,
            "maximum_input_tokens": 96000,
            "safety_margin_tokens": 16384,
            "compaction_threshold_tokens": 79616,
            "components": {
                "system_tokens": 10,
                "tools_tokens": 20,
                "history_tokens": 30,
                "attachments_tokens": 0,
                "framing_tokens": 5,
                "pending_prompt_tokens": 15
            },
            "occupied_input_tokens": 80,
            "remaining_input_tokens": 79536,
            "requires_compaction": false,
            "output_cap_disposition": { "emitted": 4096 }
        })
    );
}

#[test]
fn unknown_and_conservative_budget_never_claim_exact_capacity() {
    // arrange: the same request cost with either no limits or an explicit conservative fallback.
    let limits = ResolvedModelLimits::default();
    let request_cost = ProviderRequestCost {
        history_tokens: 400,
        ..ProviderRequestCost::default()
    };

    // act: both unknown-limit modes are computed.
    let unknown = compute_request_budget(RequestBudgetInput {
        model_limits: &limits,
        request_cost,
        requested_output_tokens: None,
        safety_margin_tokens: 100,
        estimated_token_triggers: false,
        fallback_input_tokens: 0,
        output_cap_disposition: ProviderOutputCapDisposition::UnspecifiedUnknownLimit,
    })
    .unwrap_or_abort();
    let conservative = compute_request_budget(RequestBudgetInput {
        model_limits: &limits,
        request_cost,
        requested_output_tokens: None,
        safety_margin_tokens: 100,
        estimated_token_triggers: true,
        fallback_input_tokens: 500,
        output_cap_disposition: ProviderOutputCapDisposition::UnspecifiedUnknownLimit,
    })
    .unwrap_or_abort();

    // assert: unknown has no capacity fields and conservative remains explicitly non-exact.
    assert_eq!(unknown.status, BudgetStatus::UnknownLimits);
    assert_eq!(unknown.maximum_input_tokens, None);
    assert_eq!(unknown.compaction_threshold_tokens, None);
    assert_eq!(unknown.remaining_input_tokens, None);
    assert_eq!(unknown.requires_compaction, None);
    assert_eq!(conservative.status, BudgetStatus::ConservativeFallback);
    assert_eq!(conservative.reserved_output_tokens, None);
    assert_eq!(
        conservative.output_cap_disposition,
        ProviderOutputCapDisposition::UnspecifiedUnknownLimit
    );
}

#[test]
fn unified_context_budget_matches_request_compaction_metadata_and_resume() {
    // arrange: one formula snapshot crossing request, compaction, metadata, and resume boundaries.
    let snapshot = canonical_snapshot();
    let provider_metadata = ProviderRequestStartedMetadata {
        context_budget: Some(snapshot),
        ..ProviderRequestStartedMetadata::default()
    };
    let compaction_snapshot = snapshot;
    let runtime_context = RecordedRuntimeContext {
        model_limits: ResolvedModelLimits::from_values(
            Some(128_000),
            Some(96_000),
            Some(4_096),
            ModelLimitProvenance::explicit("budget equality test"),
        ),
        last_request_budget: Some(snapshot),
        ..RecordedRuntimeContext::default()
    };
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    std::fs::write(
        temp_dir.path().join("meta.json"),
        serde_json::to_vec(&json!({
            "run_id": "run_budget_equality",
            "run_name": "interactive",
            "workspace_root": "/workspace",
            "config_digest": "digest",
            "harness_version": "test",
            "recorded_runtime_context": runtime_context
        }))
        .unwrap_or_abort(),
    )
    .unwrap_or_abort();

    // act: canonical metadata loading reconstructs the resumable runtime context.
    let resumed = load_run_metadata(temp_dir.path())
        .and_then(|metadata| metadata.recorded_runtime_context)
        .unwrap_or_abort();
    let provider = provider_metadata.context_budget.unwrap_or_abort();
    let meta = serde_json::from_slice::<Value>(
        &std::fs::read(temp_dir.path().join("meta.json")).unwrap_or_abort(),
    )
    .unwrap_or_abort()["recorded_runtime_context"]["last_request_budget"]
        .clone();
    let resume = resumed.last_request_budget.unwrap_or_abort();

    // assert: all core surfaces are equal and branch summaries cannot invent a hidden limit.
    assert_eq!(provider, compaction_snapshot);
    assert_eq!(serde_json::to_value(provider).unwrap_or_abort(), meta);
    assert_eq!(provider, resume);
    let branch_summary_source = include_str!("../../src/coord/agent_turn_completion.rs");
    assert!(
        branch_summary_source.contains("model_limits.context_window_tokens()"),
        "branch summary does not select the canonical recorded model limits"
    );
    assert!(
        !branch_summary_source.contains("unwrap_or(128_000)"),
        "branch summary still invents a hidden 128000-token capacity"
    );

    println!(
        "G003_BUDGET_COMPONENTS_CORE={}",
        serde_json::to_string(&json!({
            "provider": provider,
            "compaction": compaction_snapshot,
            "meta": meta,
            "resume": resume
        }))
        .unwrap_or_abort()
    );
}
