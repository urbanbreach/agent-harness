//! Shared fixtures for OC permission parity RED tests (T2).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::clock::FakeClock;
use harness_core::config::PermissionMode;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_core::UnwrapOrAbort;
use serde_json::{json, Value};

use super::common::load_events;

pub const KIND_READ: &str = "read";
pub const KIND_EXTERNAL_DIRECTORY: &str = "external_directory";
pub const KIND_DOOM_LOOP: &str = "doom_loop";

struct TestReadTool;

#[async_trait]
impl Tool for TestReadTool {
    fn id(&self) -> &str {
        "read"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let path = args_json
            .get("filePath")
            .or_else(|| args_json.get("path"))
            .and_then(Value::as_str)
            .unwrap_or("<missing>");
        Ok(ToolResult::text(format!("read ok {path}")))
    }
}

struct TestShellTool;

#[async_trait]
impl Tool for TestShellTool {
    fn id(&self) -> &str {
        "bash"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("bash ok {args_json}")))
    }
}

struct TestEditTool;

#[async_trait]
impl Tool for TestEditTool {
    fn id(&self) -> &str {
        "edit"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("edit ok {args_json}")))
    }
}

fn parity_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestReadTool));
    registry.register(Arc::new(TestShellTool));
    registry.register(Arc::new(TestEditTool));
    Arc::new(registry)
}

fn allow_tools_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
    .with_ask_timeout_ms(30_000)
}

pub fn parity_coordinator(session_dir: &Path) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = allow_tools_policy();
    config.tool_registry = parity_tool_registry();

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

pub async fn wait_for_tool_settled(
    events_path: &Path,
    tool_call_id: &str,
    max_yields: usize,
) -> Vec<EventEnvelopeV1> {
    for _ in 0..max_yields {
        let events = load_events(events_path);
        let permission_requested = events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id)
            )
        });
        let finished = events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id
            )
        });
        if permission_requested || finished {
            return events;
        }
        tokio::task::yield_now().await;
    }
    load_events(events_path)
}

pub fn permission_kinds_for_tool_call(
    events: &[EventEnvelopeV1],
    tool_call_id: &str,
) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id) =>
            {
                Some(data.kind.clone())
            }
            _ => None,
        })
        .collect()
}

pub fn tool_finished_status(
    events: &[EventEnvelopeV1],
    tool_call_id: &str,
) -> Option<ToolCallStatus> {
    events.iter().find_map(|event| match &event.payload {
        EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id => {
            Some(data.status)
        }
        _ => None,
    })
}

pub async fn request_read(coordinator: &CoordinatorHandle, file_path: impl Into<Value>) -> String {
    coordinator
        .request_tool_call(
            super::common::supervisor_actor(),
            None,
            "read",
            json!({"filePath": file_path.into()}),
        )
        .await
        .unwrap_or_abort()
}

pub async fn request_bash(coordinator: &CoordinatorHandle, command: &str) -> String {
    coordinator
        .request_tool_call(
            super::common::supervisor_actor(),
            None,
            "bash",
            json!({"command": command}),
        )
        .await
        .unwrap_or_abort()
}

pub async fn request_edit_external(coordinator: &CoordinatorHandle, path: PathBuf) -> String {
    coordinator
        .request_tool_call(
            super::common::supervisor_actor(),
            None,
            "edit",
            json!({
                "filePath": path,
                "oldString": "a",
                "newString": "b",
            }),
        )
        .await
        .unwrap_or_abort()
}
