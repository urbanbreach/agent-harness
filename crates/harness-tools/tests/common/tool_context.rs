use std::path::Path;

use harness_core::clock::RealClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolContext;

use super::worker_actor;

pub fn test_context(workspace_root: &Path, run_id: &str, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        std::sync::Arc::new(RealClock::new()),
        std::sync::Arc::new(DefaultRedactor::default()),
    );
    ToolContext {
        run_id: run_id.to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: worker_actor("worker-1"),
        category: Some("deep".to_string()),
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}
