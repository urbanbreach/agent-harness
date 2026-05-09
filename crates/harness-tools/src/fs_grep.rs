use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::Path;

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::fs_walk::{
    collect_workspace_files, resolve_search_base, workspace_file_from_path, WorkspaceFile,
    SKIPPED_WORKSPACE_DIRS,
};
use crate::limit_summary::summarize_limit;

pub(crate) const DEFAULT_GREP_LIMIT: usize = 100;
pub(crate) const DEFAULT_GREP_CONTEXT: usize = 0;

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
    literal: bool,
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

struct FileMatchSelection {
    selected_line_indexes: Vec<usize>,
    total_count: usize,
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
        let args: FsGrepArgs = crate::parse_tool_args(args_json)?;

        let (workspace_root, resolved_base, display_path) =
            resolve_search_base(&ctx, args.path.as_deref())?;
        if !resolved_base.is_dir() && !resolved_base.is_file() {
            return Err(ToolError::InvalidArguments(
                "path must resolve to a file or directory".to_string(),
            ));
        }

        let limit = args
            .limit
            .map_or(DEFAULT_GREP_LIMIT, |value| value as usize);
        let context = args
            .context
            .map_or(DEFAULT_GREP_CONTEXT, |value| value as usize);

        let matches = collect_grep_matches(
            &workspace_root,
            &resolved_base,
            &args.pattern,
            args.literal,
            args.include.as_deref(),
            limit,
            context,
        )?;

        Ok(crate::text_json_tool_result(
            matches.lines.join("\n"),
            json!({
                "pattern": args.pattern,
                "path": display_path,
                "resolved_path": resolved_base.display().to_string(),
                "include": args.include,
                "literal": args.literal,
                "limit": limit,
                "context": context,
                "matches": matches.lines,
                "total_count": matches.total_count,
                "returned_count": matches.returned_count,
                "truncated_count": matches.truncated_count,
                "truncated": matches.is_truncated,
                "skipped_dirs": SKIPPED_WORKSPACE_DIRS,
            }),
        ))
    }
}

fn collect_grep_matches(
    workspace_root: &Path,
    search_path: &Path,
    pattern: &str,
    literal: bool,
    include: Option<&str>,
    limit: usize,
    context: usize,
) -> Result<GrepMatches, ToolError> {
    let regex = compile_grep_regex(pattern, literal)?;
    let include_matcher = compile_include_matcher(include)?;
    let files = collect_sorted_grep_files(workspace_root, search_path, include_matcher.as_ref())?;

    let mut rendered_lines = Vec::new();
    let mut total_count = 0usize;
    let mut selected_count = 0usize;

    for file in files {
        let Some(lines) = read_utf8_lines(&file.path)? else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }

        let file_matches =
            select_file_matches(&regex, &lines, limit.saturating_sub(selected_count));
        total_count += file_matches.total_count;
        selected_count += file_matches.selected_line_indexes.len();

        if file_matches.selected_line_indexes.is_empty() {
            continue;
        }

        append_rendered_lines(
            &mut rendered_lines,
            &file.relative_path,
            &lines,
            &file_matches.selected_line_indexes,
            context,
        );
    }

    let limit_summary = summarize_limit(total_count, limit);
    Ok(GrepMatches {
        lines: rendered_lines,
        total_count,
        returned_count: limit_summary.returned_count,
        truncated_count: limit_summary.truncated_count,
        is_truncated: limit_summary.is_truncated,
    })
}

fn collect_sorted_grep_files(
    workspace_root: &Path,
    search_path: &Path,
    include_matcher: Option<&GlobMatcher>,
) -> Result<Vec<WorkspaceFile>, ToolError> {
    let mut files = collect_candidate_files(workspace_root, search_path)?;

    if let Some(matcher) = include_matcher {
        files.retain(|file| {
            matcher.is_match(&file.file_name) || matcher.is_match(&file.relative_path)
        });
    }

    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn select_file_matches(
    regex: &Regex,
    lines: &[String],
    remaining_limit: usize,
) -> FileMatchSelection {
    let mut selected_line_indexes = Vec::new();
    let mut total_count = 0usize;

    for (line_idx, line) in lines.iter().enumerate() {
        if regex.is_match(line) {
            total_count += 1;
            if selected_line_indexes.len() < remaining_limit {
                selected_line_indexes.push(line_idx);
            }
        }
    }

    FileMatchSelection {
        selected_line_indexes,
        total_count,
    }
}

fn compile_grep_regex(pattern: &str, literal: bool) -> Result<Regex, ToolError> {
    let compiled_pattern = if literal {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };

    Regex::new(&compiled_pattern).map_err(|err| {
        ToolError::InvalidArguments(format_invalid_regex_pattern_error(pattern, err))
    })
}

fn format_invalid_regex_pattern_error(pattern: &str, err: regex::Error) -> String {
    format!(
        "invalid regex pattern: {err}\nHint: grep patterns are regular expressions. Escape regex metacharacters (for example, `{}`) or set `literal: true` to search for this text exactly.",
        regex::escape(pattern)
    )
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
) -> Result<Vec<WorkspaceFile>, ToolError> {
    if search_path.is_file() {
        return Ok(vec![workspace_file_from_path(workspace_root, search_path)?]);
    }

    if !search_path.is_dir() {
        return Err(ToolError::InvalidArguments(
            "path must resolve to a file or directory".to_string(),
        ));
    }

    collect_workspace_files(workspace_root, search_path)
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
            output.push(render_grep_line(relative_path, lines, line_idx));
        }
        return;
    }

    for line_idx in context_line_indexes(lines.len(), match_line_indexes, context) {
        output.push(render_grep_line(relative_path, lines, line_idx));
    }
}

fn context_line_indexes(
    line_count: usize,
    match_line_indexes: &[usize],
    context: usize,
) -> BTreeSet<usize> {
    let mut line_indexes = BTreeSet::<usize>::new();
    for &match_idx in match_line_indexes {
        let start = match_idx.saturating_sub(context);
        let end = (match_idx + context).min(line_count.saturating_sub(1));

        for line_idx in start..=end {
            line_indexes.insert(line_idx);
        }
    }
    line_indexes
}

fn render_grep_line(relative_path: &str, lines: &[String], line_idx: usize) -> String {
    format!("{relative_path}:{}: {}", line_idx + 1, lines[line_idx])
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
        let Some(line) = decode_utf8_line(raw_line.as_slice()) else {
            return Ok(None);
        };
        lines.push(line);
    }

    Ok(Some(lines))
}

fn decode_utf8_line(raw_line: &[u8]) -> Option<String> {
    let mut line = raw_line;
    if let Some(stripped) = line.strip_suffix(b"\n") {
        line = stripped;
        if let Some(stripped) = line.strip_suffix(b"\r") {
            line = stripped;
        }
    }

    std::str::from_utf8(line).ok().map(ToOwned::to_owned)
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
            current_model_ref: None,
            current_model_settings: None,
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
            collect_grep_matches(root, root, "TODO", false, None, 100, 1).expect("collect matches");

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
            collect_grep_matches(root, root, "TODO", false, Some("*.txt"), 1, 0).expect("collect");

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

    #[test]
    fn collect_grep_matches_invalid_regex_suggests_literal_search() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();
        fs::write(root.join("notes.txt"), "task(run_in_background\n").expect("write notes");

        let err = collect_grep_matches(root, root, "task(run_in_background", false, None, 100, 0)
            .expect_err("unescaped regex group should fail");

        let message = err.to_string();
        assert!(message.contains("invalid regex pattern"));
        assert!(message.contains("task\\(run_in_background"));
        assert!(message.contains("literal: true"));
    }

    #[test]
    fn collect_grep_matches_literal_search_escapes_regex_metacharacters() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();
        fs::write(root.join("notes.txt"), "task(run_in_background\ntaskXrun\n")
            .expect("write notes");

        let result = collect_grep_matches(root, root, "task(run_in_background", true, None, 100, 0)
            .expect("literal search should escape pattern");

        assert_eq!(result.total_count, 1);
        assert_eq!(result.lines, vec!["notes.txt:1: task(run_in_background"]);
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
