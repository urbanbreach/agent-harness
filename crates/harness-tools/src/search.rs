use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy)]
pub(crate) enum SearchTool {
    Glob,
    Grep,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GlobArgs {
    pattern: String,
    path: Option<PathBuf>,
    limit: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GrepArgs {
    pattern: String,
    path: Option<PathBuf>,
    include: Option<String>,
    #[serde(default)]
    literal: bool,
    limit: Option<usize>,
    context: Option<usize>,
    head_limit: Option<usize>,
    #[serde(default)]
    output_mode: Mode,
}
#[derive(Clone, Copy, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    Content,
    FilesWithMatches,
    Count,
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(|e| ToolError::InvalidArguments(e.to_string()))
}
fn limit(value: Option<usize>) -> Result<usize, ToolError> {
    let value = value.unwrap_or(100);
    if !(1..=1000).contains(&value) {
        return Err(ToolError::InvalidArguments(
            "limit must be between 1 and 1000".into(),
        ));
    }
    Ok(value)
}
fn glob(pattern: &str) -> Result<globset::GlobMatcher, ToolError> {
    if pattern.len() > 8192 {
        return Err(ToolError::InvalidArguments(
            "pattern exceeds 8192 bytes".into(),
        ));
    }
    globset::Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| ToolError::InvalidArguments(e.to_string()))
}
#[async_trait::async_trait]
impl Tool for SearchTool {
    fn id(&self) -> &str {
        match self {
            Self::Glob => "glob",
            Self::Grep => "grep",
        }
    }
    fn description(&self) -> &str {
        match self {
        Self::Glob => "Find files by glob, newest first. Ignore files, hidden files, symlinks, and build directories are respected.",
        Self::Grep => "Search UTF-8 files by regex or literal text. Supports context, content, file names, and counts. Files requiring separate read approval are skipped.",
    }
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        match self {
            Self::Glob => schemars::schema_for!(GlobArgs).to_value(),
            Self::Grep => schemars::schema_for!(GrepArgs).to_value(),
        }
    }
    fn filesystem_paths(&self, value: &Value) -> Result<Vec<PathBuf>, ToolError> {
        let path = match self {
            Self::Glob => parse::<GlobArgs>(value.clone())?.path,
            Self::Grep => parse::<GrepArgs>(value.clone())?.path,
        };
        Ok(vec![path.unwrap_or_else(|| ".".into())])
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        vec![(
            "read".into(),
            args.get("path")
                .and_then(Value::as_str)
                .unwrap_or(".")
                .into(),
        )]
    }
    async fn call(&self, ctx: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let tool = *self;
        let executor = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || match tool {
            Self::Glob => find_files(&ctx, parse(value)?),
            Self::Grep => search_text(&ctx, &executor, parse(value)?),
        })
        .await
        .map_err(|_| ToolError::Execution("file search stopped unexpectedly".into()))?
    }
}
pub(crate) fn files(
    root: &std::path::Path,
) -> impl Iterator<Item = Result<PathBuf, ToolError>> + use<> {
    ignore::WalkBuilder::new(root)
        .follow_links(false)
        .require_git(false)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            entry.depth() == 0
                || !matches!(
                    entry.file_name().to_str(),
                    Some("target" | "node_modules" | "__pycache__")
                )
        })
        .build()
        .take(100_001)
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            _ if index == 100_000 => Some(Err(ToolError::Execution(
                "file scan exceeds 100000 entries; narrow the path".into(),
            ))),
            Ok(entry) if entry.file_type().is_some_and(|t| t.is_file()) => {
                Some(Ok(entry.into_path()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(ToolError::Execution(error.to_string()))),
        })
}
fn match_path<'a>(root: &std::path::Path, path: &'a std::path::Path) -> &'a std::path::Path {
    path.strip_prefix(root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| path.file_name().map(std::path::Path::new).unwrap_or(path))
}
fn find_files(ctx: &ToolContext, args: GlobArgs) -> Result<ToolResult, ToolError> {
    let root =
        ctx.resolve_workspace_path(args.path.as_deref().unwrap_or(std::path::Path::new(".")))?;
    let pattern = glob(&args.pattern)?;
    let cap = limit(args.limit)?;
    let mut newest = BTreeSet::new();
    let mut total = 0;
    for path in files(&root) {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let path = path?;
        if !pattern.is_match(match_path(&root, &path)) {
            continue;
        }
        total += 1;
        let modified = path.metadata()?.modified()?;
        newest.insert((std::cmp::Reverse(modified), path));
        if newest.len() > cap {
            newest.pop_last();
        }
    }
    let paths: Vec<_> = newest
        .into_iter()
        .map(|(_, p)| {
            p.strip_prefix(&ctx.workspace_root)
                .unwrap_or(&p)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    Ok(ToolResult::structured(
        paths.join("\n"),
        json!({"paths":paths, "total_count":total, "truncated":total > cap}),
    ))
}
fn search_text(
    ctx: &ToolContext,
    executor: &tokio::runtime::Handle,
    args: GrepArgs,
) -> Result<ToolResult, ToolError> {
    let root =
        ctx.resolve_workspace_path(args.path.as_deref().unwrap_or(std::path::Path::new(".")))?;
    let cap = limit(args.limit)?;
    let head = limit(args.head_limit)?;
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
    let include = args.include.as_deref().map(glob).transpose()?;
    let mut candidates = files(&root).filter(|entry| {
        entry.as_ref().map_or(true, |path| {
            include
                .as_ref()
                .is_none_or(|g| g.is_match(match_path(&root, path)))
        })
    });
    let mut found = Vec::new();
    let mut display = String::new();
    let mut total = 0;
    let mut kept = 0;
    let mut skipped = 0;
    let mut truncated = false;
    'search: loop {
        let mut batch = candidates
            .by_ref()
            .take(128)
            .collect::<Result<Vec<_>, _>>()?;
        if batch.is_empty() {
            break;
        }
        if root.is_dir() {
            let count = batch.len();
            batch = executor
                .block_on(
                    ctx.coordinator
                        .allowed_read_paths(ctx.tool_call_id.to_string(), batch),
                )
                .map_err(|e| ToolError::Execution(e.to_string()))?;
            skipped += count - batch.len();
        }
        for path in batch {
            if ctx.cancellation.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            // Recheck symlinks after discovery, before opening the file.
            let path = ctx.resolve_workspace_path(&path)?;
            let text = match crate::files::contents(&path) {
                Ok(text) if !text.contains('\0') => text,
                _ => {
                    skipped += 1;
                    continue;
                }
            };
            let (count, hits) = matching_lines(
                &text,
                &pattern,
                if args.output_mode == Mode::Content {
                    cap - kept
                } else {
                    0
                },
            );
            if count == 0 {
                continue;
            }
            total += count;
            if found.len() == head || (args.output_mode == Mode::Content && kept == cap) {
                truncated = true;
                break 'search;
            }
            let name = path
                .strip_prefix(&ctx.workspace_root)
                .unwrap_or(&path)
                .to_string_lossy();
            match args.output_mode {
                Mode::FilesWithMatches => display.push_str(&format!("{name}\n")),
                Mode::Count => display.push_str(&format!("{name}:{count}\n")),
                Mode::Content => {
                    truncated |= render_matches(&mut display, &name, &text, &hits, context);
                    truncated |= count > hits.len();
                }
            }
            kept += hits.len();
            found.push(json!({"path":name, "count":count}));
            if display.len() > 1024 * 1024 {
                break 'search;
            }
        }
    }
    if skipped > 0 || truncated {
        display.push_str(&format!("[Search incomplete: {skipped} files skipped for policy, format, size, or read errors; output truncated: {truncated}.]\n"));
    }
    Ok(ToolResult::structured(
        display,
        json!({"files":found,"scanned_matches":total,"total_count":if skipped > 0 || truncated { None } else { Some(total) },"skipped_files":skipped,"truncated":truncated}),
    ))
}

fn matching_lines(text: &str, pattern: &regex::Regex, cap: usize) -> (usize, Vec<usize>) {
    let mut count = 0;
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if !pattern.is_match(line) {
            continue;
        }
        count += 1;
        if hits.len() < cap {
            hits.push(index);
        }
    }
    (count, hits)
}
fn render_matches(
    display: &mut String,
    name: &str,
    text: &str,
    hits: &[usize],
    context: usize,
) -> bool {
    let mut hit = 0;
    let mut truncated = false;
    for (index, line) in text.lines().enumerate() {
        while hits
            .get(hit)
            .is_some_and(|pos| index > pos.saturating_add(context))
        {
            hit += 1;
        }
        if hits
            .get(hit)
            .is_none_or(|pos| index < pos.saturating_sub(context))
        {
            continue;
        }
        let end = line.floor_char_boundary(line.len().min(8192));
        display.push_str(&format!("{name}:{}:{}\n", index + 1, &line[..end]));
        truncated |= end < line.len();
        if display.len() > 1024 * 1024 {
            return true;
        }
    }
    truncated
}
