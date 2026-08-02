use harness_core::UnwrapOrAbort;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::clock::FakeClock;
use harness_core::config::PermissionMode;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{EventV1, PermissionDecision};
use harness_core::perm::{PermissionDecision as RuntimePermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_core::ToolResultExt;
use serde::Deserialize;
use serde_json::{json, Value};

mod common;

use common::{load_events, supervisor_actor, wait_for_tool_call_finish};

struct TestShellTool;

#[async_trait]
impl Tool for TestShellTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, _ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("shell ok"))
    }
}

struct TestBatchTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    tool_calls: Vec<BatchCall>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchCall {
    tool: String,
    parameters: Value,
}

#[async_trait]
impl Tool for TestBatchTool {
    fn id(&self) -> &str {
        "tool.batch"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BatchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        for tool_call in args.tool_calls {
            ctx.coordinator
                .request_tool_call(
                    ctx.actor.clone(),
                    ctx.category.clone(),
                    tool_call.tool,
                    tool_call.parameters,
                )
                .await
                .tool_err("nested batch tool failed")?;
        }

        Ok(ToolResult::text("batch ok"))
    }
}

#[tokio::test]
async fn batch_inherits_nested_tool_permissions_without_bypass() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run(
            "batch_permission_inheritance",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let batch_tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            None,
            "tool.batch",
            json!({
                "tool_calls": [{
                    "tool": "shell.run",
                    "parameters": {"cmd": "true"},
                }],
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    let events = load_events(&run.events_path);
    let nested_tool_call_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data)
                if data.tool_id == "shell.run"
                    && data.tool_call_id.as_str() != batch_tool_call_id =>
            {
                Some(data.tool_call_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    let (nested_permission_id, nested_permission_summary) = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(nested_tool_call_id.as_str()) =>
            {
                Some((data.permission_id.clone(), data.summary.clone()))
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert!(
        nested_permission_summary.contains("tool=shell.run"),
        "nested permission prompt should name effective child tool: {nested_permission_summary}"
    );

    coordinator
        .resolve_permission(
            nested_permission_id,
            RuntimePermissionDecision::Deny,
            Some("nested batch denial".to_string()),
        )
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == batch_tool_call_id
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == PermissionDecision::Deny
                    && data.reason.as_deref() == Some("nested batch denial")
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == nested_tool_call_id
        )
    }));
}

fn test_coordinator(session_dir: &Path) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Ask,
        PermissionMode::Allow,
    )
    .with_ask_timeout_ms(5_000);
    config.tool_registry = test_tool_registry();

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn test_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestBatchTool));
    registry.register(Arc::new(TestShellTool));
    Arc::new(registry)
}
