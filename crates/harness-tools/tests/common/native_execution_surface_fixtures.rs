use std::fs;

use harness_core::agent::{build_provider_tool_defs, build_provider_tool_defs_for_model, AgentProfile};
use harness_core::config::ShellAllowlist;
use harness_core::edit::hashline::compute_line_hash;
use harness_core::tool::ToolRunState;
use harness_tools::{coordinator_registry, coordinator_registry_with_internal_hashline_tools};
use serde_json::json;

#[path = "mod.rs"]
mod common;

use common::{
    setup_workspace_fixture, test_context as common_test_context,
    test_context_with_tool_state as common_test_context_with_tool_state,
};

fn test_context(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-native-surface-tests", tool_call_id)
}

fn test_context_with_tool_state(
    workspace_root: &std::path::Path,
    tool_call_id: &str,
    tool_state: ToolRunState,
) -> harness_core::tool::ToolContext {
    common_test_context_with_tool_state(
        workspace_root,
        "run-native-surface-tests",
        tool_call_id,
        tool_state,
    )
}
