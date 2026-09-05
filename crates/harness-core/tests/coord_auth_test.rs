use harness_core::UnwrapOrAbort;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{
    PermissionMode, PermissionRuleSet, PermissionSelector, PermissionSelectorRule,
    ProfilePermissions,
};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle,
};
use harness_core::event::{ActorKind, EventActor, EventV1, PermissionDecision, ToolCallStatus};
use harness_core::perm::{PermissionDecision as PolicyPermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde_json::json;

mod common;

use common::{
    allow_all_permission_policy, load_events, supervisor_actor, wait_for_tool_call_finish,
};

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
async fn worker_tool_auth_uses_profile_name_not_caller_category() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let policy = allow_all_permission_policy()
        .with_profile_override(
            "worker",
            ProfilePermissions {
                shell: Some(PermissionMode::Allow),
                ..ProfilePermissions::default()
            },
        )
        .with_profile_override(
            "spoof-deny",
            ProfilePermissions {
                shell: Some(PermissionMode::Deny),
                ..ProfilePermissions::default()
            },
        );

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(vec!["shell.run".to_string()]),
        policy,
    );

    let run = coordinator
        .start_run(
            "profile_name_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-deny".to_string()),
            SHELL_RUN_TOOL_ID,
            json!({"cmd": TRUE_CMD_ARGS}),
        )
        .await
        .expect("profile name permission should allow the request");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id
        )
    }));
}

#[tokio::test]
async fn supervisor_tool_auth_ignores_caller_category_for_permission_routing() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let policy = allow_all_permission_policy()
        .with_profile_override(
            "default",
            ProfilePermissions {
                shell: Some(PermissionMode::Allow),
                ..ProfilePermissions::default()
            },
        )
        .with_profile_override(
            "spoof-deny",
            ProfilePermissions {
                shell: Some(PermissionMode::Deny),
                ..ProfilePermissions::default()
            },
        );
    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(vec!["shell.run".to_string()]),
        policy,
    );
    let run = coordinator
        .start_run(
            "supervisor_profile_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("spoof-deny".to_string()),
            SHELL_RUN_TOOL_ID,
            json!({"cmd": TRUE_CMD_ARGS}),
        )
        .await
        .expect("default profile permission should allow the request");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;
    coordinator.stop_run().await.unwrap_or_abort();

    // act
    let events = load_events(&run.events_path);
    // assert
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id
    )));
}

#[tokio::test]
async fn unknown_worker_agent_id_is_denied_closed() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(vec!["shell.run".to_string()]),
        allow_all_permission_policy(),
    );

    let run = coordinator
        .start_run("unknown_worker", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();

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

    coordinator.stop_run().await.unwrap_or_abort();

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
    let temp_dir = tempfile::tempdir().unwrap_or_abort();

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(vec!["edit".to_string()]),
        allow_all_permission_policy(),
    );

    let run = coordinator
        .start_run("toolset_enforcement", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();

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

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PolicyViolationDetected(data) if data.policy == "tool_not_in_toolset"
        )
    }));
}

#[tokio::test]
async fn shell_permission_summary_redacts_command_secrets() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Ask,
        PermissionMode::Allow,
    )
    .with_ask_timeout_ms(1);
    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(vec![SHELL_RUN_TOOL_ID.to_string()]),
        policy,
    );
    let run = coordinator
        .start_run(
            "shell_permission_redaction",
            temp_dir.path().join("workspace"),
        )
        .await
        .unwrap_or_abort();
    let command = "curl -H 'Authorization: Bearer sk-secret1234567890' https://example.test";

    // act
    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("worker-allow".to_string()),
            SHELL_RUN_TOOL_ID,
            json!({"command": command}),
        )
        .await
        .unwrap_or_abort();
    let permission_id = load_events(&run.events_path)
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    coordinator
        .resolve_permission(
            permission_id,
            PolicyPermissionDecision::Deny,
            Some("test denied".to_string()),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;
    coordinator.stop_run().await.unwrap_or_abort();

    // assert
    let events = load_events(&run.events_path);
    let summary = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(tool_call_id.as_str()) =>
            {
                Some(data.summary.as_str())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert!(!summary.contains("sk-secret1234567890"));
    assert!(summary.contains("Bearer [REDACTED]"));
}

#[tokio::test]
async fn edit_rename_requires_permission_for_destination_path() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let policy = allow_all_permission_policy().with_profile_override(
        "worker",
        ProfilePermissions {
            edit: Some(PermissionMode::Deny),
            rules: PermissionRuleSet {
                edit: vec![PermissionSelectorRule {
                    selector: PermissionSelector::Prefix(".agent-harness/plans/".to_string()),
                    mode: PermissionMode::Allow,
                }],
                ..PermissionRuleSet::default()
            },
            ..ProfilePermissions::default()
        },
    );

    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(vec![EDIT_TOOL_ID.to_string()]),
        policy,
    );

    let run = coordinator
        .start_run(
            "edit_rename_destination_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();
    let active_plan = harness_core::plan::plan_file_display_path(run.run_id.as_str());

    let error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            EDIT_TOOL_ID,
            json!({
                "filePath": active_plan,
                "rename": "src/new.rs"
            }),
        )
        .await
        .expect_err("rename destination outside plan scope must be denied");

    let denied_tool_call_id = match error {
        CoordinatorError::PermissionDenied(tool_call_id) => tool_call_id,
        other => panic!("expected PermissionDenied, got {other:?}"),
    };

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == PermissionDecision::Deny
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
async fn profile_edit_rules_allow_path_prefix_and_deny_outside() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let policy = allow_all_permission_policy().with_profile_override(
        "worker",
        ProfilePermissions {
            edit: Some(PermissionMode::Deny),
            rules: PermissionRuleSet {
                edit: vec![
                    PermissionSelectorRule {
                        selector: PermissionSelector::CatchAll,
                        mode: PermissionMode::Deny,
                    },
                    PermissionSelectorRule {
                        selector: PermissionSelector::Prefix(".agent-harness/plans/".to_string()),
                        mode: PermissionMode::Allow,
                    },
                ],
                ..PermissionRuleSet::default()
            },
            ..ProfilePermissions::default()
        },
    );

    let workspace = temp_dir.path().join("workspace");
    let coordinator = test_coordinator(
        temp_dir.path(),
        worker_profile(vec![EDIT_TOOL_ID.to_string()]),
        policy,
    );

    let run = coordinator
        .start_run("active_plan_edit", &workspace)
        .await
        .unwrap_or_abort();
    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .unwrap_or_abort();
    let active_plan = harness_core::plan::plan_file_display_path(run.run_id.as_str());

    let allowed_tool_call_id = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id.clone())),
            Some("spoof-allow".to_string()),
            EDIT_TOOL_ID,
            json!({ "filePath": active_plan }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &allowed_tool_call_id).await;

    let denied_error = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            EDIT_TOOL_ID,
            json!({ "filePath": "src/lib.rs" }),
        )
        .await
        .expect_err("path outside the allowed prefix must be denied");

    let denied_tool_call_id = match denied_error {
        CoordinatorError::PermissionDenied(tool_call_id) => tool_call_id,
        other => panic!("expected PermissionDenied, got {other:?}"),
    };

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::ToolCallFinished(data)
            if data.tool_call_id.as_str() == allowed_tool_call_id && data.status == ToolCallStatus::Succeeded
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::PermissionResolved(data)
            if data.decision == PermissionDecision::Deny
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == denied_tool_call_id
    )));
}

fn worker_profile(toolset: Vec<String>) -> AgentProfile {
    AgentProfile {
        name: "worker".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "worker-prompt".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset,
        permission_ruleset: Vec::new(),
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
