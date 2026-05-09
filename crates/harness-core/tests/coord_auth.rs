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
use harness_core::event::{ActorKind, EventActor, EventV1, PermissionDecision};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde_json::json;

mod common;

use common::{allow_all_permission_policy, load_events, supervisor_actor};

const SHELL_RUN_TOOL_ID: &str = "shell.run";
const EDIT_TOOL_ID: &str = "edit";
const TRUE_CMD_ARGS: &str = "true";

struct TestShellTool;
struct TestEditTool;

#[async_trait]
impl Tool for TestShellTool {
    fn id(&self) -> &str {
        SHELL_RUN_TOOL_ID
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("ok"))
    }
}

#[async_trait]
impl Tool for TestEditTool {
    fn id(&self) -> &str {
        EDIT_TOOL_ID
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("edit ok"))
    }
}

#[tokio::test]
async fn tool_auth_uses_derived_worker_category_not_caller_category() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let policy = allow_all_permission_policy()
        .with_category_override(
            "worker-deny",
            CategoryPermissions {
                shell: Some(PermissionMode::Deny),
                ..CategoryPermissions::default()
            },
        )
        .with_category_override(
            "spoof-allow",
            CategoryPermissions {
                shell: Some(PermissionMode::Allow),
                ..CategoryPermissions::default()
            },
        );

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile("worker-deny", vec!["shell.run".to_string()]),
        policy,
    );

    let run = coordinator
        .start_run("derived_category", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            SHELL_RUN_TOOL_ID,
            json!({"cmd": TRUE_CMD_ARGS}),
        )
        .await
        .expect_err("request should be denied by derived worker category");

    let denied_tool_call_id = match error {
        CoordinatorError::PermissionDenied(tool_call_id) => tool_call_id,
        other => panic!("expected PermissionDenied, got {other:?}"),
    };

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == PermissionDecision::Deny
                    && data.reason.as_deref() == Some("policy denied request (shell)")
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == denied_tool_call_id
        )
    }));
}

#[tokio::test]
async fn unknown_worker_agent_id_is_denied_closed() {
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile("worker-allow", vec!["shell.run".to_string()]),
        allow_all_permission_policy(),
    );

    let run = coordinator
        .start_run("unknown_worker", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some("agent_missing".to_string())),
            Some("spoof-allow".to_string()),
            SHELL_RUN_TOOL_ID,
            json!({"cmd": TRUE_CMD_ARGS}),
        )
        .await
        .expect_err("unknown worker id must fail closed");

    assert!(matches!(error, CoordinatorError::PolicyViolation(_)));

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PolicyViolationDetected(data) if data.policy == "unknown_worker_agent_id"
        )
    }));
}

#[tokio::test]
async fn worker_toolset_enforcement_blocks_non_allowlisted_tool() {
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile("worker-allow", vec!["edit".to_string()]),
        allow_all_permission_policy(),
    );

    let run = coordinator
        .start_run("toolset_enforcement", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            SHELL_RUN_TOOL_ID,
            json!({"cmd": TRUE_CMD_ARGS}),
        )
        .await
        .expect_err("tool outside worker toolset must be denied");

    assert!(matches!(error, CoordinatorError::PolicyViolation(_)));

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PolicyViolationDetected(data) if data.policy == "tool_not_in_toolset"
        )
    }));
}

#[tokio::test]
async fn edit_rename_requires_permission_for_destination_path() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let policy = allow_all_permission_policy().with_category_override(
        "plan",
        CategoryPermissions {
            edit: Some(PermissionMode::Deny),
            rules: PermissionRuleSet {
                edit: vec![PermissionSelectorRule {
                    selector: PermissionSelector::Prefix(".agent-harness/plans/".to_string()),
                    mode: PermissionMode::Allow,
                }],
                ..PermissionRuleSet::default()
            },
            ..CategoryPermissions::default()
        },
    );

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile("plan", vec![EDIT_TOOL_ID.to_string()]),
        policy,
    );

    let run = coordinator
        .start_run(
            "edit_rename_destination_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .expect("spawn worker");

    let error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            EDIT_TOOL_ID,
            json!({
                "filePath": ".agent-harness/plans/run_123.md",
                "rename": "src/new.rs"
            }),
        )
        .await
        .expect_err("rename destination outside plan scope must be denied");

    let denied_tool_call_id = match error {
        CoordinatorError::PermissionDenied(tool_call_id) => tool_call_id,
        other => panic!("expected PermissionDenied, got {other:?}"),
    };

    coordinator.stop_run().await.expect("stop run");

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
    registry.register(Arc::new(TestShellTool));
    registry.register(Arc::new(TestEditTool));
    Arc::new(registry)
}
