use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use walkdir::{DirEntry, WalkDir};

use crate::hashline_apply::resolve_workspace_target_path;

const DEFAULT_LIMIT: usize = 100;
const DEFAULT_CONTEXT: usize = 0;
const SKIPPED_DIR_NAMES: &[&str] = &[".git", "target"];
const SKIPPED_RELATIVE_DIRS: &[&str] = &[".agent-harness/sessions"];

pub(crate) struct FsGrepTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FsGrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    context: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrepMatches {
    lines: Vec<String>,
    total_count: usize,
    returned_count: usize,
    truncated_count: usize,
    is_truncated: bool,
}

#[async_trait]
impl Tool for FsGrepTool {
    fn id(&self) -> &str {
        "fs.grep"
    }

    fn description(&self) -> &str {
        "Searches UTF-8 workspace files or directories for regex matches with optional include glob and context lines."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        super::json_schema_for::<FsGrepArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: FsGrepArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        let base_path = args.path.as_deref().unwrap_or(".");
        let workspace_root = ctx.resolve_workspace_path(Path::new("."))?;
        let resolved_base = resolve_workspace_target_path(&ctx, base_path)?;
        if !resolved_base.is_dir() && !resolved_base.is_file() {
            return Err(ToolError::InvalidArguments(
                "path must resolve to a file or directory".to_string(),
            ));
        }
        let display_path = workspace_relative_display(&workspace_root, &resolved_base)?;

        let limit = args.limit.map_or(DEFAULT_LIMIT, |value| value as usize);
        let context = args.context.map_or(DEFAULT_CONTEXT, |value| value as usize);

        let matches = collect_grep_matches(
            &workspace_root,
            &resolved_base,
            &args.pattern,
            args.include.as_deref(),
            limit,
            context,
        )?;

        Ok(ToolResult {
            display_text: matches.lines.join("\n"),
            structured_json: Some(json!({
                "pattern": args.pattern,
                "path": display_path,
                "resolved_path": resolved_base.display().to_string(),
                "include": args.include,
                "limit": limit,
                "context": context,
                "matches": matches.lines,
                "total_count": matches.total_count,
                "returned_count": matches.returned_count,
                "truncated_count": matches.truncated_count,
                "truncated": matches.is_truncated,
                "skipped_dirs": [".git", "target", ".agent-harness/sessions"],
            })),
            artifacts: Vec::new(),
        })
    }
}

fn collect_grep_matches(
    workspace_root: &Path,
    search_path: &Path,
    pattern: &str,
    include: Option<&str>,
    limit: usize,
    context: usize,
) -> Result<GrepMatches, ToolError> {
    let regex = Regex::new(pattern)
        .map_err(|err| ToolError::InvalidArguments(format!("invalid regex pattern: {err}")))?;
    let include_matcher = compile_include_matcher(include)?;

    let mut files = collect_candidate_files(workspace_root, search_path)?;

    if let Some(matcher) = include_matcher.as_ref() {
        files.retain(|(relative, file_name, _)| {
            matcher.is_match(file_name) || matcher.is_match(relative)
        });
    }

    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut rendered_lines = Vec::new();
    let mut total_count = 0usize;
    let mut returned_count = 0usize;

    for (relative, _file_name, path) in files {
        let Some(lines) = read_utf8_lines(&path)? else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }

        let mut selected_matches = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            if regex.is_match(line) {
                total_count += 1;
                if returned_count < limit {
                    selected_matches.push(line_idx);
                    returned_count += 1;
                }
            }
        }

        if selected_matches.is_empty() {
            continue;
        }

        append_rendered_lines(
            &mut rendered_lines,
            &relative,
            &lines,
            &selected_matches,
            context,
        );
    }

    let truncated_count = total_count.saturating_sub(returned_count);
    Ok(GrepMatches {
        lines: rendered_lines,
        total_count,
        returned_count,
        truncated_count,
        is_truncated: truncated_count > 0,
    })
}

fn compile_include_matcher(include: Option<&str>) -> Result<Option<GlobMatcher>, ToolError> {
    include
        .map(|pattern| {
            Glob::new(pattern)
                .map_err(|err| {
                    ToolError::InvalidArguments(format!("invalid include glob pattern: {err}"))
                })
                .map(|glob| glob.compile_matcher())
        })
        .transpose()
}

fn collect_candidate_files(
    workspace_root: &Path,
    search_path: &Path,
) -> Result<Vec<(String, String, PathBuf)>, ToolError> {
    if search_path.is_file() {
        let relative = search_path.strip_prefix(workspace_root).map_err(|_| {
            ToolError::Execution(format!(
                "failed to compute workspace-relative path for {}",
                search_path.display()
            ))
        })?;
        let file_name = search_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        return Ok(vec![(
            normalize_relative_path(relative),
            file_name,
            search_path.to_path_buf(),
        )]);
    }

    if !search_path.is_dir() {
        return Err(ToolError::InvalidArguments(
            "path must resolve to a file or directory".to_string(),
        ));
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(search_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(workspace_root, entry))
    {
        let entry = entry.map_err(|err| {
            ToolError::Execution(format!("failed to traverse directory tree: {err}"))
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(workspace_root).map_err(|_| {
            ToolError::Execution(format!(
                "failed to compute workspace-relative path for {}",
                entry.path().display()
            ))
        })?;

        let relative = normalize_relative_path(relative);
        let file_name = entry.file_name().to_string_lossy().to_string();
        let path = entry.into_path();
        files.push((relative, file_name, path));
    }
    Ok(files)
}

fn append_rendered_lines(
    output: &mut Vec<String>,
    relative_path: &str,
    lines: &[String],
    match_line_indexes: &[usize],
    context: usize,
) {
    if context == 0 {
        for &line_idx in match_line_indexes {
            output.push(format!(
                "{relative_path}:{}: {}",
                line_idx + 1,
                lines[line_idx]
            ));
        }
        return;
    }

    let mut line_indexes = BTreeMap::<usize, bool>::new();
    for &match_idx in match_line_indexes {
        let start = match_idx.saturating_sub(context);
        let end = (match_idx + context).min(lines.len().saturating_sub(1));

        for line_idx in start..=end {
            line_indexes
                .entry(line_idx)
                .and_modify(|is_match| *is_match = *is_match || line_idx == match_idx)
                .or_insert(line_idx == match_idx);
        }
    }

    for (line_idx, _is_match) in line_indexes {
        output.push(format!(
            "{relative_path}:{}: {}",
            line_idx + 1,
            lines[line_idx]
        ));
    }
}

fn read_utf8_lines(path: &Path) -> Result<Option<Vec<String>>, ToolError> {
    let file = std::fs::File::open(path).map_err(|err| {
        ToolError::Execution(format!("failed to read file {}: {err}", path.display()))
    })?;
    let mut reader = std::io::BufReader::new(file);
    let mut raw_line = Vec::new();
    let mut lines = Vec::new();

    loop {
        raw_line.clear();
        let bytes_read = reader.read_until(b'\n', &mut raw_line).map_err(|err| {
            ToolError::Execution(format!("failed to read file {}: {err}", path.display()))
        })?;
        if bytes_read == 0 {
            break;
        }
        let mut line = raw_line.as_slice();
        if let Some(stripped) = line.strip_suffix(b"\n") {
            line = stripped;
            if let Some(stripped) = line.strip_suffix(b"\r") {
                line = stripped;
            }
        }
        let Ok(line) = String::from_utf8(line.to_vec()) else {
            return Ok(None);
        };
        lines.push(line);
    }

    Ok(Some(lines))
}

fn should_skip_entry(workspace_root: &Path, entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }

    let path = entry.path();
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if entry.file_type().is_dir() && SKIPPED_DIR_NAMES.contains(&name) {
        return true;
    }

    let Ok(relative) = path.strip_prefix(workspace_root) else {
        return false;
    };
    let relative = normalize_relative_path(relative);

    SKIPPED_RELATIVE_DIRS
        .iter()
        .any(|prefix| relative == *prefix || relative.starts_with(&format!("{prefix}/")))
}

fn normalize_relative_path(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return ".".to_string();
    }

    path.iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn workspace_relative_display(
    workspace_root: &Path,
    resolved_path: &Path,
) -> Result<String, ToolError> {
    let relative = resolved_path.strip_prefix(workspace_root).map_err(|_| {
        ToolError::PathEscapesWorkspace {
            workspace_root: workspace_root.display().to_string(),
            path: resolved_path.display().to_string(),
        }
    })?;
    Ok(normalize_relative_path(relative))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolContext};
    use serde_json::json;

    use super::{collect_grep_matches, FsGrepTool};

    fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-fs-grep-tests".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: workspace_root.join(".artifacts"),
            actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
            category: Some("deep".to_string()),
            tool_call_id: tool_call_id.to_string(),
            coordinator,
        }
    }

    #[test]
    fn collect_grep_matches_finds_matches_with_context_and_skips_ignored_directories() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();

        fs::create_dir_all(root.join("docs")).expect("create docs");
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::create_dir_all(root.join("target/build")).expect("create target dir");
        fs::create_dir_all(root.join(".git/objects")).expect("create .git dir");
        fs::create_dir_all(root.join(".agent-harness/sessions/run-1")).expect("create sessions");

        fs::write(root.join("docs/todo.md"), "TODO docs\nnote\n").expect("write docs/todo.md");
        fs::write(
            root.join("src/main.txt"),
            "alpha\nTODO first\nbeta\nTODO second\ngamma\n",
        )
        .expect("write src/main.txt");
        fs::write(root.join("target/build/generated.txt"), "TODO hidden")
            .expect("write target file");
        fs::write(root.join(".git/objects/cache.txt"), "TODO hidden").expect("write git file");
        fs::write(
            root.join(".agent-harness/sessions/run-1/log.txt"),
            "TODO hidden",
        )
        .expect("write sessions file");
        fs::write(
            root.join("src/binary.bin"),
            [0xff_u8, 0xfe, 0x00, 0x54, 0x4f, 0x44, 0x4f],
        )
        .expect("write binary");

        let result =
            collect_grep_matches(root, root, "TODO", None, 100, 1).expect("collect matches");

        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 3);
        assert_eq!(result.truncated_count, 0);
        assert!(!result.is_truncated);
        assert_eq!(
            result.lines,
            vec![
                "docs/todo.md:1: TODO docs",
                "docs/todo.md:2: note",
                "src/main.txt:1: alpha",
                "src/main.txt:2: TODO first",
                "src/main.txt:3: beta",
                "src/main.txt:4: TODO second",
                "src/main.txt:5: gamma",
            ]
        );
    }

    #[test]
    fn collect_grep_matches_applies_include_filter_and_limit() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();

        fs::write(root.join("a.txt"), "TODO a\n").expect("write a.txt");
        fs::write(root.join("b.log"), "TODO b\n").expect("write b.log");
        fs::write(root.join("c.txt"), "TODO c\n").expect("write c.txt");

        let result =
            collect_grep_matches(root, root, "TODO", Some("*.txt"), 1, 0).expect("collect");

        assert_eq!(result.total_count, 2);
        assert_eq!(result.returned_count, 1);
        assert_eq!(result.truncated_count, 1);
        assert!(result.is_truncated);
        assert_eq!(result.lines, vec!["a.txt:1: TODO a"]);
    }

    #[tokio::test]
    async fn fs_grep_accepts_exact_file_path_without_searching_siblings() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("src/session_lineage.rs"), "fn fork_point() {}\n")
            .expect("write target file");
        fs::write(root.join("src/other.rs"), "fn fork_point() {}\n").expect("write sibling file");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-exact-file-path"),
                json!({
                    "pattern": "fork_point",
                    "path": "src/session_lineage.rs",
                    "include": "*.rs"
                }),
            )
            .await
            .expect("grep should accept exact file paths");

        assert_eq!(
            result.display_text,
            "src/session_lineage.rs:1: fn fork_point() {}"
        );
        let structured = result.structured_json.expect("structured result");
        assert_eq!(
            structured.get("path"),
            Some(&json!("src/session_lineage.rs"))
        );
        assert_eq!(structured.get("total_count"), Some(&json!(1)));
    }

    #[tokio::test]
    async fn fs_grep_accepts_absolute_workspace_path() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();
        fs::write(root.join("main.rs"), "#[tokio::main]\nfn main() {}\n").expect("write main.rs");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-abs-path"),
                json!({
                    "pattern": "#\\[tokio::main\\]",
                    "path": root.display().to_string(),
                    "include": "*.rs"
                }),
            )
            .await
            .expect("grep with absolute workspace path should succeed");

        assert!(result.display_text.contains("main.rs:1: #[tokio::main]"));
        let structured = result.structured_json.expect("structured result");
        assert_eq!(structured.get("path"), Some(&json!(".")));
    }
}
