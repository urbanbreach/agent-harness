use std::path::Path;

use harness_core::edit::hashline::LineAnchor;
use harness_core::tool::{ToolContext, ToolError};

pub(crate) fn record_file_read(ctx: &ToolContext, resolved_path: &Path) -> Result<(), ToolError> {
    ctx.tool_state.record_file_read(&ctx.run_id, resolved_path)
}

pub(crate) fn record_file_hashline_read(
    ctx: &ToolContext,
    resolved_path: &Path,
    anchors: Vec<LineAnchor>,
) -> Result<(), ToolError> {
    ctx.tool_state
        .record_file_hashline_read(&ctx.run_id, resolved_path, anchors)
}

pub(crate) fn recent_hashline_anchors(
    ctx: &ToolContext,
    resolved_path: &Path,
) -> Option<Vec<LineAnchor>> {
    ctx.tool_state
        .recent_hashline_anchors(&ctx.run_id, resolved_path)
}
