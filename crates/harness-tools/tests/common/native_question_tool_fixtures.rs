use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::clock::RealClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::EventV1;
use harness_core::perm::PermissionDecision;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolContext, ToolError, ToolResult, ToolRunState};
use harness_tools::coordinator_registry;
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};

#[path = "mod.rs"]
mod common;

use common::{
    allow_all_permission_policy, read_events, setup_workspace_fixture,
    wait_for_question_permission as wait_for_question_permission_event, worker_actor,
};

async fn wait_for_question_permission(path: &Path) -> String {
    wait_for_question_permission_event(path, None, Duration::from_secs(5)).await
}

fn question_tool_context(
    coordinator: CoordinatorHandle,
    run_id: &str,
    workspace_root: &Path,
    artifacts_dir: &Path,
    tool_call_id: &str,
) -> ToolContext {
    ToolContext {
        run_id: run_id.to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: artifacts_dir.to_path_buf(),
        actor: worker_actor("agent-worker"),
        category: Some("deep".to_string()),
        tool_call_id: tool_call_id.to_string(),
        current_model_ref: None,
        current_model_settings: None,
        tool_state: ToolRunState::default(),
        coordinator,
    }
}

fn spawn_question_coordinator(session_dir: PathBuf, ask_timeout_ms: u64) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = allow_all_permission_policy().with_ask_timeout_ms(ask_timeout_ms);
    spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn question_tool() -> Arc<dyn Tool> {
    coordinator_registry(Default::default())
        .get("question")
        .expect("question tool")
}

fn spawn_question_tool_call(
    coordinator: CoordinatorHandle,
    run_id: &str,
    workspace_root: &Path,
    artifacts_dir: &Path,
    tool_call_id: &str,
    args: Value,
) -> tokio::task::JoinHandle<Result<ToolResult, ToolError>> {
    let question_tool = question_tool();
    let context = question_tool_context(
        coordinator,
        run_id,
        workspace_root,
        artifacts_dir,
        tool_call_id,
    );
    tokio::spawn(async move { question_tool.call(context, args).await })
}
