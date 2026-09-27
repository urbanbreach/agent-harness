mod stage;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum AstTool {
    Search,
    Replace,
}
#[derive(Default, Clone, Copy, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    DryRun,
    Apply,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    pattern: String,
    rewrite: Option<String>,
    language: Option<String>,
    path: Option<PathBuf>,
    #[serde(default)]
    paths: Vec<PathBuf>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    mode: Mode,
    context: Option<usize>,
    limit: Option<usize>,
}
impl Args {
    fn roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<_> = self.path.iter().chain(&self.paths).cloned().collect();
        if roots.is_empty() {
            roots.push(".".into());
        }
        roots
    }
}
fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidArguments(message.into())
}
fn failure(message: impl Into<String>) -> ToolError {
    ToolError::Execution(message.into())
}
impl AstTool {
    fn parse(self, value: Value) -> Result<Args, ToolError> {
        let args: Args = serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
        if args.pattern.trim().is_empty()
            || args.pattern.len() > 8192
            || args.rewrite.as_ref().is_some_and(|s| s.len() > 8192)
            || args.paths.len() > 256
            || (self == Self::Replace) != args.rewrite.is_some()
            || (self == Self::Search && args.mode == Mode::Apply)
        {
            return Err(invalid("provide a pattern (at most 8192 bytes), at most 256 paths, and rewrite only for ast_grep_replace"));
        }
        Ok(args)
    }
}
#[async_trait::async_trait]
impl Tool for AstTool {
    fn id(&self) -> &str {
        match self {
            Self::Search => "ast_grep_search",
            Self::Replace => "ast_grep_replace",
        }
    }
    fn description(&self) -> &str {
        match self {
            Self::Search => "Search code structurally using ast-grep patterns such as console.log($X). Select language or one inferable language, path/paths, include/exclude globs, context (0–5), and limit (1–200). Requires ast-grep on PATH.",
            Self::Replace => "Preview structural edits with pattern and rewrite; mode defaults to dry_run. Apply mode checks the complete plan and each target's edit permission before changing files through normal edit receipts. Partial, unreadable, or over-limit plans are not applied.",
        }
    }
    fn capability(&self) -> ToolCapability {
        if *self == Self::Replace {
            ToolCapability::EditFs
        } else {
            ToolCapability::ReadFs
        }
    }
    fn parameters_json_schema(&self) -> Value {
        schemars::schema_for!(Args).to_value()
    }
    fn filesystem_paths(&self, value: &Value) -> Result<Vec<PathBuf>, ToolError> {
        Ok(self.parse(value.clone())?.roots())
    }
    fn permission_requests(&self, value: &Value) -> Vec<(String, String)> {
        self.parse(value.clone())
            .map(|args| {
                args.roots()
                    .into_iter()
                    .flat_map(|path| {
                        let selector = path.to_string_lossy().into_owned();
                        let mut requests = vec![("read".into(), selector.clone())];
                        if args.mode == Mode::Apply {
                            requests.push(("edit".into(), selector));
                        }
                        requests
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    async fn call(&self, ctx: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let args = self.parse(value)?;
        tokio::task::spawn_blocking(move || execute(&ctx, args))
            .await
            .map_err(|_| failure("structural search worker stopped"))?
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    text: String,
    file: PathBuf,
    range: Range,
    lines: Option<String>,
    replacement: Option<String>,
    replacement_offsets: Option<Span>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Range {
    byte_offset: Span,
    start: Position,
    end: Position,
}
#[derive(Deserialize)]
struct Position {
    line: u32,
    column: u32,
}
#[derive(Deserialize, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}
impl Span {
    fn slice(self, text: &str) -> Result<&str, ToolError> {
        text.get(self.start..self.end)
            .ok_or_else(|| failure("ast-grep returned an invalid UTF-8 byte range"))
    }
}
fn execute(ctx: &ToolContext, args: Args) -> Result<ToolResult, ToolError> {
    let started = Instant::now();
    let executor = tokio::runtime::Handle::current();
    let (stage, sources, language, skipped) = stage::prepare(ctx, &args, &executor)?;
    let limit = args.limit.unwrap_or(100).clamp(1, 200);
    let mut hits: Vec<Hit> = if sources.is_empty() {
        Vec::new()
    } else {
        let mut command = tokio::process::Command::new("ast-grep");
        command
            .current_dir(stage.path())
            .args([
                "run",
                "--pattern",
                &args.pattern,
                "--lang",
                language,
                "--json=compact",
                "--threads",
                "1",
                "--color",
                "never",
                "--heading",
                "never",
                "--context",
            ])
            .arg(args.context.unwrap_or(0).min(5).to_string());
        if let Some(rewrite) = &args.rewrite {
            command.args(["--rewrite", rewrite]);
        }
        command
            .arg("--")
            .args(sources.iter().map(|source| &source.staged));
        let timeout = Duration::from_secs(30)
            .checked_sub(started.elapsed())
            .ok_or_else(|| failure("structural search timed out"))?;
        let output = executor
            .block_on(crate::process::run(command, timeout, &ctx.cancellation))
            .map_err(|e| match e {
                ToolError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    failure("ast-grep is required on PATH; install it or use grep")
                }
                e => e,
            })?;
        if output.truncated {
            return Err(failure(
                "ast-grep output exceeds 512 KiB; narrow the search",
            ));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Pattern contains an ERROR node")
            || !(output.status.success() || output.status.code() == Some(1))
        {
            return Err(failure(format!(
                "ast-grep failed: {}",
                &stderr[..stderr.floor_char_boundary(stderr.len().min(4096))]
            )));
        }
        let hits: Vec<Hit> = serde_json::from_slice(&output.stdout)
            .map_err(|e| failure(format!("invalid ast-grep response: {e}")))?;
        if !output.status.success() && !hits.is_empty() {
            return Err(failure("ast-grep failed with partial results"));
        }
        hits
    };
    let count = hits.len();
    if args.mode == Mode::Apply && (count > limit || skipped > 0) {
        return Err(failure(
            "rewrite limit exceeded or files were skipped; narrow the selection before applying",
        ));
    }
    hits.truncate(limit);
    let mut grouped = BTreeMap::<usize, Vec<&Hit>>::new();
    let mut matches = Vec::new();
    for hit in &hits {
        let index = sources
            .iter()
            .position(|s| s.staged == hit.file)
            .ok_or_else(|| failure("ast-grep returned an unrequested path"))?;
        let source = &sources[index];
        if hit.range.byte_offset.slice(&source.before)? != hit.text {
            return Err(failure(
                "ast-grep returned a match that differs from the source",
            ));
        }
        grouped.entry(index).or_default().push(hit);
        matches.push(json!({"file_path":source.path.strip_prefix(&ctx.workspace_root).unwrap_or(&source.path),
            "range":{"start":{"line":hit.range.start.line.saturating_add(1),"column":hit.range.start.column.saturating_add(1)},"end":{"line":hit.range.end.line.saturating_add(1),"column":hit.range.end.column.saturating_add(1)}},
            "byte_range":{"start":hit.range.byte_offset.start,"end":hit.range.byte_offset.end},
            "matched_text":hit.text,"snippet":hit.lines,"replacement":hit.replacement}));
    }
    let mut plans = Vec::new();
    let mut diff = String::new();
    if args.rewrite.is_some() {
        for (index, mut hits) in grouped {
            hits.sort_by_key(|hit| {
                hit.replacement_offsets
                    .unwrap_or(hit.range.byte_offset)
                    .start
            });
            let source = &sources[index];
            let (mut end, mut size) = (0, source.before.len());
            for hit in &hits {
                let span = hit.replacement_offsets.unwrap_or(hit.range.byte_offset);
                let _ = span.slice(&source.before)?;
                if span.start < end {
                    return Err(failure("overlapping AST rewrites require separate calls"));
                }
                end = span.end;
                size = size.saturating_sub(span.end - span.start).saturating_add(
                    hit.replacement
                        .as_ref()
                        .ok_or_else(|| failure("ast-grep omitted a replacement"))?
                        .len(),
                );
                if size as u64 > crate::files::MAX_FILE {
                    return Err(failure("rewritten file exceeds 8 MiB"));
                }
            }
            let mut after = source.before.clone();
            for hit in hits.into_iter().rev() {
                let span = hit.replacement_offsets.unwrap_or(hit.range.byte_offset);
                after.replace_range(
                    span.start..span.end,
                    hit.replacement
                        .as_deref()
                        .ok_or_else(|| failure("missing replacement"))?,
                );
            }
            if after == source.before {
                continue;
            }
            if args.mode == Mode::DryRun {
                let display = source
                    .path
                    .strip_prefix(&ctx.workspace_root)
                    .unwrap_or(&source.path)
                    .to_string_lossy();
                diff.push_str(
                    &similar::TextDiff::configure()
                        .timeout(Duration::from_millis(200))
                        .diff_lines(&source.before, &after)
                        .unified_diff()
                        .header(&format!("a/{display}"), &format!("b/{display}"))
                        .to_string(),
                );
                if diff.len() > 1024 * 1024 {
                    return Err(failure(
                        "rewrite preview exceeds 1 MiB; narrow the selection",
                    ));
                }
            }
            plans.push((index, after));
        }
    }
    let mut artifacts = Vec::new();
    if !diff.is_empty() {
        artifacts.push(
            executor
                .block_on(
                    ctx.coordinator
                        .retain_tool_diff(ctx.tool_call_id.to_string(), diff),
                )
                .map_err(|e| failure(e.to_string()))?,
        );
    }
    let mut result = json!({"language":language,"total_count":count,"returned_count":hits.len(),"truncated":count>limit,
        "skipped_files":skipped,"matches":matches,"applied":false,"diff_artifact":artifacts.first()});
    let mut completed = Vec::new();
    let mut details = Vec::new();
    let mut warning = None;
    if args.mode == Mode::Apply {
        executor
            .block_on(
                ctx.coordinator.authorize_tool_edit_paths(
                    ctx.tool_call_id.to_string(),
                    plans
                        .iter()
                        .map(|(i, _)| sources[*i].path.clone())
                        .collect(),
                ),
            )
            .map_err(|e| failure(e.to_string()))?;
        for (index, _) in &plans {
            let source = &sources[*index];
            if ctx.resolve_workspace_path(&source.path)? != source.path
                || crate::files::contents(&source.path)? != source.before
            {
                return Err(failure("a rewrite target changed; no files were edited"));
            }
        }
        for (index, after) in plans {
            let source = &sources[index];
            let display = source
                .path
                .strip_prefix(&ctx.workspace_root)
                .unwrap_or(&source.path);
            let written = ctx.tool_state.edit(&source.path, |_| {
                crate::files::edit::commit(
                    ctx,
                    &source.path,
                    display,
                    Some(&source.before),
                    Some(after),
                )
            });
            match written {
                Ok(output) => {
                    details.push(output.display_text);
                    artifacts.extend(output.artifacts);
                    completed.push(display.to_string_lossy().into_owned());
                }
                Err(error) => {
                    warning = Some(error.to_string());
                    break;
                }
            }
        }
        result["applied"] = warning.is_none().into();
    }
    result["completed_files"] = json!(completed);
    result["is_error"] = warning.is_some().into();
    result["error"] = json!(warning);
    Ok(ToolResult::structured_with_artifacts(
        format!(
            "{} structural matches; {} files changed.\n{}{}",
            hits.len(),
            completed.len(),
            details.join("\n"),
            warning.map_or_else(String::new, |w| format!("\n{w}"))
        ),
        result,
        artifacts,
    ))
}
