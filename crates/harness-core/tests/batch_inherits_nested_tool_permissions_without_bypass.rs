use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::clock::FakeClock;
use harness_core::config::PermissionMode;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

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
                .map_err(|err| ToolError::Execution(format!("nested batch tool failed: {err}")))?;
        }

        Ok(ToolResult::text("batch ok"))
    }
}

#[tokio::test]
async fn batch_inherits_nested_tool_permissions_without_bypass() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run(
            "batch_permission_inheritance",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

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
        .expect("batch tool call should be accepted for execution");

    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let nested_tool_call_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data)
                if data.tool_id == "shell.run" && data.tool_call_id != batch_tool_call_id =>
            {
                Some(data.tool_call_id.clone())
            }
            _ => None,
        })
        .expect("nested shell call should be requested through the coordinator");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == batch_tool_call_id
        )
    }));
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
            EventV1::ToolCallStarted(data) if data.tool_call_id == nested_tool_call_id
        )
    }));
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()))
}

fn test_coordinator(session_dir: &Path) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
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

fn load_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events file");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event jsonl line"))
        .collect()
}

async fn wait_for_tool_call_finish(events_path: &Path, tool_call_id: &str) {
    for _ in 0..40 {
        if load_events(events_path).iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data) if data.tool_call_id == tool_call_id
            )
        }) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("timed out waiting for tool call {tool_call_id} to finish");
}
