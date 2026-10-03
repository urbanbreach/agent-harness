use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::{
        ModelLimitProvenance, ResolvedModelCatalogEntry, ResolvedModelLimits, ResolvedModelTarget,
        ShellAllowlist, SubagentCatalogAuthority, SubagentCatalogModel, SubagentModelCatalog,
    },
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionDecision, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
    subagent::{FinalizedStateResult, RawFinalizedState},
};
use harness_providers::{
    mock::MockProvider, CompletionUsage, ProviderStreamEvent as Stream,
    ProviderStreamFinishedMetadata,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio_stream::StreamExt;

fn done() -> Stream {
    Stream::DoneWithMetadata {
        usage: Some(CompletionUsage {
            prompt_tokens: 12,
            completion_tokens: 3,
            total_tokens: 15,
        }),
        metadata: Some(ProviderStreamFinishedMetadata {
            settled_reasoning: Some(Vec::new()),
            usage_complete: Some(true),
            ..Default::default()
        }),
    }
}

fn config(path: &std::path::Path, provider: Arc<MockProvider>) -> CoordinatorConfig {
    let registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    let mut parent = AgentProfile::fallback("default");
    parent.model_ref = "mock:chosen".into();
    parent.toolset = registry.tool_ids();
    let mut config = CoordinatorConfig::new(path.join("sessions"));
    config.provider = provider;
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = Arc::new(registry);
    config.agent_profiles.insert("default".into(), parent);
    let resolution = harness_core::model_resolution::resolve_model(
        harness_core::model_resolution::ModelResolutionInput {
            provider: "mock",
            model: "chosen",
            metadata_family: None,
            input_modalities: &[],
            supports_tool_calls: Some(true),
            supports_reasoning_summaries: Some(true),
        },
    );
    let limits = ResolvedModelLimits::from_values(
        Some(100_000),
        Some(98_000),
        Some(4096),
        ModelLimitProvenance::explicit("deterministic adapter fixture"),
    );
    let catalog = ResolvedModelCatalogEntry {
        provider: "mock".into(),
        provider_display_label: "Mock".into(),
        provider_backend_label: None,
        model: "chosen".into(),
        model_display_label: "Chosen".into(),
        variant: None,
        variant_display_label: None,
        display_label: "Mock Chosen".into(),
        token_window_label: None,
        limits: limits.clone(),
        description: None,
        reasoning_effort: None,
        text_verbosity: None,
        recommended_for: None,
        thinking: None,
        supports_reasoning_summaries: true,
        resolution: resolution.clone(),
    };
    config.agent_model_targets.insert(
        "default".into(),
        ResolvedModelTarget {
            model_ref: "mock:chosen".into(),
            provider: "mock".into(),
            model: "chosen".into(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            limits,
            resolution,
            catalog_entry: Some(Box::new(catalog.clone())),
        },
    );
    config.model_catalog = Arc::from([catalog]);
    config.subagent_model_catalog = Some(SubagentModelCatalog {
        authority: SubagentCatalogAuthority::Complete,
        models: vec![SubagentCatalogModel {
            id: "mock:chosen".into(),
            family: Some("test".into()),
            picker_eligible: true,
        }],
    });
    config
}

fn child_id(value: &Value) -> Result<String, Box<dyn std::error::Error>> {
    Ok(value["subagent_id"]
        .as_str()
        .ok_or("missing subagent_id")?
        .into())
}

fn available(
    value: FinalizedStateResult,
) -> Result<Box<RawFinalizedState>, Box<dyn std::error::Error>> {
    match value {
        FinalizedStateResult::Available { state } => Ok(state),
        FinalizedStateResult::Unavailable { reason } => {
            Err(format!("finalized source unavailable: {reason:?}").into())
        }
    }
}

#[path = "tasks/resume_history.rs"]
mod resume_history;
#[path = "tasks/skill_permissions.rs"]
mod skill_permissions;
#[path = "tasks/spawn_approval.rs"]
mod spawn_approval;
#[path = "tasks/spawn_failures.rs"]
mod spawn_failures;
