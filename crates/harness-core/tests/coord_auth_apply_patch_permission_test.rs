use harness_core::UnwrapOrAbort;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{
    CategoryPermissions, PermissionMode, PermissionRuleSet, PermissionSelector,
    PermissionSelectorRule,
};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
};
use harness_core::event::{ActorKind, EventActor, EventV1, PermissionDecision};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde_json::json;

mod common;

use common::{allow_all_permission_policy, load_events, supervisor_actor};

const APPLY_PATCH_TOOL_ID: &str = "apply_patch";

struct TestApplyPatchTool;

#[async_trait]
impl Tool for TestApplyPatchTool {
    fn id(&self) -> &str {
        APPLY_PATCH_TOOL_ID
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("apply patch ok"))
    }
}

#[tokio::test]
async fn apply_patch_requires_permission_for_patch_text_paths() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let policy = allow_all_permission_policy().with_category_override(
        "docs-only",
        CategoryPermissions {
            edit: Some(PermissionMode::Deny),
            rules: PermissionRuleSet {
                edit: vec![PermissionSelectorRule {
                    selector: PermissionSelector::Prefix("docs/".to_string()),
                    mode: PermissionMode::Allow,
                }],
                ..PermissionRuleSet::default()
            },
            ..CategoryPermissions::default()
        },
    );

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile("docs-only", vec![APPLY_PATCH_TOOL_ID.to_string()]),
        policy,
    );

    let run = coordinator
        .start_run(
            "apply_patch_path_permission",
            temp_dir.path().join("workspace"),
        )
        .await
        .unwrap_or_abort();
    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    // act
    let denied_error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            APPLY_PATCH_TOOL_ID,
            json!({
                "patchText": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch"
            }),
        )
        .await
        .expect_err("apply_patch outside docs scope must be denied");

    let denied_tool_call_id = match denied_error {
        CoordinatorError::PermissionDenied(tool_call_id) => tool_call_id,
        other => panic!("expected PermissionDenied, got {other:?}"),
    };

    coordinator.stop_run().await.unwrap_or_abort();

    // assert
    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == PermissionDecision::Deny
                    && data.reason.as_deref() == Some("policy denied request (edit_fs)")
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == denied_tool_call_id
        )
    }));
}

fn worker_profile(category: &str, toolset: Vec<String>) -> AgentProfile {
    AgentProfile {
        name: "worker".to_string(),
        category: category.to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "worker-prompt".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset,
    }
}

fn test_coordinator(
    session_dir: &Path,
    worker_profile: AgentProfile,
    permission_policy: PermissionPolicy,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = permission_policy;
    config.tool_registry = test_tool_registry();
    config
        .agent_profiles
        .insert("worker".to_string(), worker_profile);

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn test_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestApplyPatchTool));
    Arc::new(registry)
}
