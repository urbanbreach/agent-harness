//! `history://current/full`: the caller's whole active branch as markdown, so a rolled
//! over agent can read or search what its live window no longer holds. The read and grep
//! tools resolve the route; selectors are their ordinary arguments.
use crate::search::{limit, matching_lines, render_matches, GrepArgs, Mode};
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use serde_json::json;
use std::path::Path;

mod render;
pub(crate) use render::render_full_history;

const SCHEME: &str = "history://";
const FULL: &str = "history://current/full";
const INVALID: &str =
    "Invalid history://current route; use exactly history://current/full (selectors may follow it)";
const MAX_DISPLAY: usize = 64 * 1024;

/// How a `history://` path parsed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Full,
    Invalid,
}

/// The route a path names, or `None` when it is an ordinary path.
pub(crate) fn parse(path: &Path) -> Option<Route> {
    let path = path.to_str()?;
    path.starts_with(SCHEME).then_some(if path == FULL {
        Route::Full
    } else {
        Route::Invalid
    })
}

/// The rendered branch. The route is gated on the setting and a bound live branch first,
/// then checked for its exact shape.
fn resolve(ctx: &ToolContext, route: Route) -> Result<String, ToolError> {
    let events = tokio::runtime::Handle::current()
        .block_on(
            ctx.coordinator
                .current_branch_events(ctx.tool_call_id.to_string()),
        )
        .map_err(ToolError::from)?;
    if route == Route::Invalid {
        return Err(ToolError::InvalidArguments(INVALID.into()));
    }
    Ok(render_full_history(&events))
}

/// Reads a window of the rendered history.
pub(crate) fn read(
    ctx: &ToolContext,
    route: Route,
    offset: usize,
    max_lines: usize,
) -> Result<ToolResult, ToolError> {
    if offset == 0 || max_lines == 0 {
        return Err(ToolError::InvalidArguments(
            "offset and limit must be positive".into(),
        ));
    }
    let text = resolve(ctx, route)?;
    let mut output = String::new();
    let mut displayed = 0;
    let mut truncated = false;
    for line in text.lines().skip(offset - 1) {
        if displayed >= max_lines.min(2000) || output.len() >= MAX_DISPLAY {
            truncated = true;
            break;
        }
        let end = line.floor_char_boundary((MAX_DISPLAY - output.len()).min(line.len()));
        output.push_str(&line[..end]);
        output.push('\n');
        displayed += 1;
        if end < line.len() {
            truncated = true;
            break;
        }
    }
    if truncated {
        output.push_str("\n[Output truncated; request a narrower range.]\n");
    }
    let mut result = ToolResult::structured(
        output,
        json!({
            "path": FULL,
            "offset": offset,
            "lines": displayed,
            "totalLines": text.lines().count(),
            "read_complete": offset == 1 && !truncated,
            "truncated": truncated,
            "mime": "text/markdown",
            "bytes": text.len(),
            "notes": ["Source: caller-bound live session branch (full, uncompacted)"],
            "meta": {"source": {"type": "internal", "value": FULL}},
        }),
    );
    result.source_artifact = true;
    Ok(result)
}

/// Searches the rendered history with the grep tool's arguments.
pub(crate) fn grep(
    ctx: &ToolContext,
    route: Route,
    args: GrepArgs,
) -> Result<ToolResult, ToolError> {
    let cap = limit(args.limit)?;
    limit(args.head_limit)?;
    let context = args.context.unwrap_or(0);
    if args.pattern.len() > 8192 || context > 64 {
        return Err(ToolError::InvalidArguments(
            "pattern exceeds 8192 bytes or context exceeds 64 lines".into(),
        ));
    }
    let pattern = if args.literal {
        regex::escape(&args.pattern)
    } else {
        args.pattern.clone()
    };
    let pattern = regex::RegexBuilder::new(&pattern)
        .size_limit(1024 * 1024)
        .build()
        .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
    let text = resolve(ctx, route)?;
    let content = args.output_mode == Mode::Content;
    let (count, hits) = matching_lines(&text, &pattern, if content { cap } else { 0 });
    let mut display = String::new();
    let mut truncated = false;
    if count > 0 {
        match args.output_mode {
            Mode::FilesWithMatches => display.push_str(&format!("{FULL}\n")),
            Mode::Count => display.push_str(&format!("{FULL}:{count}\n")),
            Mode::Content => {
                truncated |= render_matches(&mut display, FULL, &text, &hits, context);
                truncated |= count > hits.len();
            }
        }
    }
    if truncated {
        display.push_str("[Search incomplete: output truncated: true.]\n");
    }
    let found = if count > 0 {
        vec![json!({"path": FULL, "count": count})]
    } else {
        Vec::new()
    };
    let mut result = ToolResult::structured(
        display,
        json!({
            "files": found,
            "scanned_matches": count,
            "total_count": if truncated { None } else { Some(count) },
            "skipped_files": 0,
            "truncated": truncated,
        }),
    );
    result.useless = count == 0;
    result.source_artifact = count > 0;
    Ok(result)
}
