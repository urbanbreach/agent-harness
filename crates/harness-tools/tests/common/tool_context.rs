use std::path::Path;

use harness_core::clock::RealClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{ToolContext, ToolRunState};

use super::worker_actor;

pub fn test_context(workspace_root: &Path, run_id: &str, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        std::sync::Arc::new(RealClock::new()),
        std::sync::Arc::new(DefaultRedactor::default()),
    );
    ToolContext {
        run_id: run_id.to_string().into(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: worker_actor("worker-1"),
        category: Some("deep".to_string()),
        tool_call_id: tool_call_id.into(),
        current_model_ref: None,
        current_model_settings: None,
        tool_state: ToolRunState::default(),
        coordinator,
    }
}

pub fn test_context_with_tool_state(
    workspace_root: &Path,
    run_id: &str,
    tool_call_id: &str,
    tool_state: ToolRunState,
) -> ToolContext {
    ToolContext {
        tool_state,
        ..test_context(workspace_root, run_id, tool_call_id)
    }
}
