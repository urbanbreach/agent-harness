use harness_core::UnwrapOrAbort;
use std::path::{Path, PathBuf};
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
use harness_core::event::{ActorKind, EventActor, EventV1, PermissionDecision, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde_json::json;

mod common;

use common::{
    allow_all_permission_policy, load_events, supervisor_actor, wait_for_tool_call_finish,
};

const AST_GREP_REPLACE_TOOL_ID: &str = "ast_grep_replace";
const PATH_SCOPED_WORKER_CATEGORY: &str = "worker-path-scoped-edit";

struct TestAstGrepReplaceTool;

#[async_trait]
impl Tool for TestAstGrepReplaceTool {
    fn id(&self) -> &str {
        AST_GREP_REPLACE_TOOL_ID
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("replace ok"))
    }
}

#[tokio::test]
async fn ast_grep_replace_list_paths_are_permission_checked_before_tool_execution() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(),
        path_scoped_edit_permission_policy(),
    );

    let run = coordinator
        .start_run(
            "ast_grep_replace_path_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    // act
    let error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            AST_GREP_REPLACE_TOOL_ID,
            json!({
                "pattern": "fn $NAME",
                "rewrite": "fn $NAME",
                "paths": ["src/lib.rs"]
            }),
        )
        .await
        .expect_err("path-denied ast_grep_replace must not execute");

    let denied_tool_call_id = match error {
        CoordinatorError::PermissionDenied(tool_call_id) => tool_call_id,
        other => panic!("expected PermissionDenied, got {other:?}"),
    };

    coordinator.stop_run().await.unwrap_or_abort();
    let events = load_events(&run.events_path);

    // assert
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == PermissionDecision::Deny
                    && data.reason.as_deref().is_some_and(|reason|
                        reason.contains("policy denied request")
                            && reason.contains("edit")
                    )
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == denied_tool_call_id
        )
    }));
}

#[tokio::test]
async fn ast_grep_replace_allowed_list_paths_execute_normally() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(),
        path_scoped_edit_permission_policy(),
    );

    let run = coordinator
        .start_run(
            "ast_grep_replace_allowed_path_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    // act
    let allowed_tool_call_id = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            AST_GREP_REPLACE_TOOL_ID,
            json!({
                "pattern": "fn $NAME",
                "rewrite": "fn $NAME",
                "paths": ["docs/readme.md"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &allowed_tool_call_id).await;

    coordinator.stop_run().await.unwrap_or_abort();
    let events = load_events(&run.events_path);

    // assert
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == allowed_tool_call_id
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == allowed_tool_call_id
                    && data.status == ToolCallStatus::Succeeded
        )
    }));
}

fn path_scoped_edit_permission_policy() -> PermissionPolicy {
    allow_all_permission_policy().with_category_override(
        PATH_SCOPED_WORKER_CATEGORY,
        CategoryPermissions {
            edit: Some(PermissionMode::Allow),
            rules: PermissionRuleSet {
                edit: vec![PermissionSelectorRule {
                    selector: PermissionSelector::Prefix("src/".to_string()),
                    mode: PermissionMode::Deny,
                }],
                ..PermissionRuleSet::default()
            },
            ..CategoryPermissions::default()
        },
    )
}

fn worker_profile() -> AgentProfile {
    AgentProfile {
        name: "worker".to_string(),
        category: PATH_SCOPED_WORKER_CATEGORY.to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "worker-prompt".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![AST_GREP_REPLACE_TOOL_ID.to_string()],
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
    registry.register(Arc::new(TestAstGrepReplaceTool));
    Arc::new(registry)
}
