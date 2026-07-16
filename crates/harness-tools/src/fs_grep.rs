// allow: SIZE_OK — filesystem tool (read + glob + grep)
use crate::UnwrapOrAbort;
use std::collections::BTreeSet;
use std::path::Path;

use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use harness_core::tool_metadata;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};

use crate::fs_walk::{
    collect_workspace_files, resolve_search_base, workspace_file_from_path, WorkspaceFile,
    SKIPPED_WORKSPACE_DIRS,
};
use crate::limit_summary::summarize_limit;

pub(crate) const DEFAULT_GREP_LIMIT: usize = 100;
pub(crate) const DEFAULT_GREP_CONTEXT: usize = 0;
pub(crate) const MAX_GREP_RENDER_BYTES: usize = 50 * 1024;

pub(crate) struct FsGrepTool;

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GrepOutputMode {
    #[default]
    Content,
    FilesWithMatches,
    Count,
}

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
    #[serde(default)]
    output_mode: GrepOutputMode,
    #[serde(default)]
    head_limit: Option<u32>,
}

impl FsGrepArgs {
    fn search(&self) -> GrepSearch<'_> {
        GrepSearch {
            pattern: &self.pattern,
            literal: self.literal,
            include: self.include.as_deref(),
            limit: self
                .limit
                .map_or(DEFAULT_GREP_LIMIT, |value| value as usize),
            context: self
                .context
                .map_or(DEFAULT_GREP_CONTEXT, |value| value as usize),
            output_mode: self.output_mode,
            head_limit: self.head_limit.map(|value| value as usize),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrepMatches {
    lines: Vec<String>,
    display_text: String,
    total_count: usize,
    returned_count: usize,
    truncated_count: usize,
    is_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrepOutputEntry {
    path: String,
    line_number: usize,
    text: String,
}

struct FileMatchSelection {
    selected_line_indexes: Vec<usize>,
    total_count: usize,
}

struct GrepSearch<'a> {
    pattern: &'a str,
    literal: bool,
    include: Option<&'a str>,
    limit: usize,
    context: usize,
    output_mode: GrepOutputMode,
    head_limit: Option<usize>,
}

enum Utf8FileLines {
    Lines(Vec<String>),
    NonUtf8,
}

#[async_trait]
impl Tool for FsGrepTool {
    tool_metadata!(
        "fs.grep",
        "Searches UTF-8 workspace files or directories for regex matches with optional include glob and context lines.",
        ToolCapability::ReadFs,
        super::json_schema_for::<FsGrepArgs>()
    );

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

        let search = args.search();
        let output_mode = args.output_mode;
        let head_limit = args.head_limit;

        match output_mode {
            GrepOutputMode::Content => {
                let full_output_search = GrepSearch {
                    pattern: search.pattern,
                    literal: search.literal,
                    include: search.include,
                    limit: usize::MAX,
                    context: search.context,
                    output_mode,
                    head_limit: None,
                };
                let limit = search.limit;
                let context = search.context;
                let matches = collect_grep_matches(
                    &workspace_root,
                    &resolved_base,
                    search,
                    MAX_GREP_RENDER_BYTES,
                )?;
                let output_artifact = if matches.is_truncated {
                    let full_output = collect_grep_matches(
                        &workspace_root,
                        &resolved_base,
                        full_output_search,
                        usize::MAX,
                    )?
                    .lines
                    .join("\n");

                    Some(
                        ctx.artifact_store()
                            .map_err(|err| {
                                ToolError::Execution(format!(
                                    "failed to access artifact store: {err}"
                                ))
                            })?
                            .write_text(
                                &format!("toolcalls/{}/fs.grep.full.txt", ctx.tool_call_id),
                                &full_output,
                            )
                            .map_err(|err| {
                                ToolError::Execution(format!(
                                    "failed to write fs.grep artifact: {err}"
                                ))
                            })?,
                    )
                } else {
                    None
                };
                let artifacts = output_artifact.iter().cloned().collect::<Vec<_>>();
                let guidance = output_artifact
                    .as_ref()
                    .map(|artifact| grep_truncation_guidance(&matches, artifact));
                let mut display_text = matches.display_text.clone();
                if let Some(guidance) = guidance.as_ref() {
                    if !display_text.is_empty() {
                        display_text.push('\n');
                    }
                    display_text.push_str(guidance);
                }
                let mut structured_json = json!({
                    "pattern": args.pattern,
                    "path": display_path,
                    "resolved_path": resolved_base.display().to_string(),
                    "include": args.include,
                    "literal": args.literal,
                    "limit": limit,
                    "context": context,
                    "output_mode": "content",
                    "head_limit": head_limit,
                    "matches": matches.lines,
                    "total_count": matches.total_count,
                    "returned_count": matches.returned_count,
                    "truncated_count": matches.truncated_count,
                    "truncated": matches.is_truncated,
                    "skipped_dirs": SKIPPED_WORKSPACE_DIRS,
                });

                if let Some(artifact) = output_artifact.as_ref() {
                    structured_json["output_artifact"] = json!({
                        "path": artifact.path,
                        "digest": artifact.digest,
                    });
                    structured_json["guidance"] = json!(guidance);
                }

                Ok(crate::text_json_artifacts_tool_result(
                    display_text,
                    structured_json,
                    artifacts,
                ))
            }
            GrepOutputMode::FilesWithMatches => {
                let (files, total_matching_files) =
                    collect_grep_file_paths(&workspace_root, &resolved_base, &search)?;

                let returned_count = files.len();
                let is_truncated = total_matching_files > returned_count;
                let mut display_lines = vec![format!(
                    "Found {total_matching_files} files with matches{}",
                    if is_truncated {
                        " (more files available)"
                    } else {
                        ""
                    }
                )];
                for file_path in &files {
                    display_lines.push(file_path.clone());
                }
                if is_truncated {
                    display_lines.push(String::new());
                    display_lines.push(
                        "(Results truncated. Consider using a more specific path or pattern.)"
                            .to_string(),
                    );
                }
                let display_text = display_lines.join("\n");
                let structured_json = json!({
                    "pattern": args.pattern,
                    "path": display_path,
                    "resolved_path": resolved_base.display().to_string(),
                    "include": args.include,
                    "literal": args.literal,
                    "output_mode": "files_with_matches",
                    "head_limit": head_limit,
                    "files": files,
                    "total_count": total_matching_files,
                    "returned_count": returned_count,
                    "truncated": is_truncated,
                    "skipped_dirs": SKIPPED_WORKSPACE_DIRS,
                });

                Ok(crate::text_json_tool_result(display_text, structured_json))
            }
            GrepOutputMode::Count => {
                let (counts, total_match_count) =
                    collect_grep_counts(&workspace_root, &resolved_base, &search)?;

                let returned_files = counts.len();
                let is_truncated = total_match_count > 0
                    && counts.iter().map(|(_, c)| c).sum::<usize>() < total_match_count;
                let mut display_lines = vec![format!(
                    "Found {total_match_count} matches across files{}",
                    if is_truncated {
                        " (more files available)"
                    } else {
                        ""
                    }
                )];
                for (file_path, count) in &counts {
                    display_lines.push(format!("{file_path}: {count}"));
                }
                if is_truncated {
                    display_lines.push(String::new());
                    display_lines.push(
                        "(Results truncated. Consider using a more specific path or pattern.)"
                            .to_string(),
                    );
                }
                let display_text = display_lines.join("\n");
                let counts_json: Vec<Value> = counts
                    .iter()
                    .map(|(file, count)| json!({"file": file, "count": count}))
                    .collect();
                let structured_json = json!({
                    "pattern": args.pattern,
                    "path": display_path,
                    "resolved_path": resolved_base.display().to_string(),
                    "include": args.include,
                    "literal": args.literal,
                    "output_mode": "count",
                    "head_limit": head_limit,
                    "counts": counts_json,
                    "total_count": total_match_count,
                    "returned_count": returned_files,
                    "truncated": is_truncated,
                    "skipped_dirs": SKIPPED_WORKSPACE_DIRS,
                });

                Ok(crate::text_json_tool_result(display_text, structured_json))
            }
        }
    }
}

fn grep_truncation_guidance(matches: &GrepMatches, artifact: &ArtifactRef) -> String {
    format!(
        "... [truncated: showing {} of {} matches ({} hidden); full output artifact: {}; narrow by path/include/pattern or rerun grep with a more specific query]",
        matches.returned_count, matches.total_count, matches.truncated_count, artifact.path
    )
}

fn collect_grep_matches(
    workspace_root: &Path,
    search_path: &Path,
    search: GrepSearch<'_>,
    inline_max_bytes: usize,
) -> Result<GrepMatches, ToolError> {
    let regex = compile_grep_regex(search.pattern, search.literal)?;
    let include_matcher = compile_include_matcher(search.include)?;
    let files = collect_sorted_grep_files(workspace_root, search_path, include_matcher.as_ref())?;

    let mut entries = Vec::new();
    let mut total_count = 0usize;
    let mut selected_count = 0usize;
    let mut files_returned = 0usize;
    let head_limit = search.head_limit.filter(|&n| n > 0);

    for file in files {
        let lines = match read_utf8_lines(&file.path)? {
            Utf8FileLines::Lines(lines) => lines,
            Utf8FileLines::NonUtf8 => continue,
        };
        if lines.is_empty() {
            continue;
        }

        let remaining_limit = if head_limit.is_some_and(|limit| files_returned >= limit) {
            0
        } else {
            search.limit.saturating_sub(selected_count)
        };
        let file_matches = select_file_matches(&regex, &lines, remaining_limit);
        total_count += file_matches.total_count;

        if file_matches.selected_line_indexes.is_empty() {
            continue;
        }

        selected_count += file_matches.selected_line_indexes.len();
        files_returned += 1;

        append_grep_entries(
            &mut entries,
            workspace_root,
            &file.relative_path,
            &lines,
            &file_matches.selected_line_indexes,
            search.context,
        );
    }

    let limit_summary = summarize_limit(total_count, search.limit);
    let untruncated_display =
        render_grep_display(&entries, total_count, limit_summary.is_truncated);
    let (display_text, byte_truncated) =
        truncate_display_text_by_bytes(&untruncated_display, inline_max_bytes);
    let returned_count = limit_summary.returned_count;
    let is_truncated = limit_summary.is_truncated || byte_truncated;
    let truncated_count = total_count.saturating_sub(returned_count);
    let lines = entries
        .iter()
        .map(render_grep_match_line)
        .collect::<Vec<_>>();

    Ok(GrepMatches {
        lines,
        display_text,
        total_count,
        returned_count,
        truncated_count,
        is_truncated,
    })
}

fn collect_grep_file_paths(
    workspace_root: &Path,
    search_path: &Path,
    search: &GrepSearch<'_>,
) -> Result<(Vec<String>, usize), ToolError> {
    let regex = compile_grep_regex(search.pattern, search.literal)?;
    let include_matcher = compile_include_matcher(search.include)?;
    let files = collect_sorted_grep_files(workspace_root, search_path, include_matcher.as_ref())?;

    let mut file_paths = Vec::new();
    let mut total_matching_files = 0usize;
    let head_limit = search.head_limit.filter(|&n| n > 0);

    for file in files {
        let lines = match read_utf8_lines(&file.path)? {
            Utf8FileLines::Lines(lines) => lines,
            Utf8FileLines::NonUtf8 => continue,
        };
        if lines.is_empty() {
            continue;
        }

        let has_match = lines.iter().any(|line| regex.is_match(line));
        if !has_match {
            continue;
        }

        total_matching_files += 1;

        if head_limit.is_some_and(|limit| file_paths.len() >= limit) {
            continue;
        }

        let display_path = workspace_root
            .join(&file.relative_path)
            .display()
            .to_string();
        file_paths.push(display_path);
    }

    Ok((file_paths, total_matching_files))
}

fn collect_grep_counts(
    workspace_root: &Path,
    search_path: &Path,
    search: &GrepSearch<'_>,
) -> Result<(Vec<(String, usize)>, usize), ToolError> {
    let regex = compile_grep_regex(search.pattern, search.literal)?;
    let include_matcher = compile_include_matcher(search.include)?;
    let files = collect_sorted_grep_files(workspace_root, search_path, include_matcher.as_ref())?;

    let mut counts = Vec::new();
    let mut total_count = 0usize;
    let head_limit = search.head_limit.filter(|&n| n > 0);

    for file in files {
        let lines = match read_utf8_lines(&file.path)? {
            Utf8FileLines::Lines(lines) => lines,
            Utf8FileLines::NonUtf8 => continue,
        };
        if lines.is_empty() {
            continue;
        }

        let file_count = lines.iter().filter(|line| regex.is_match(line)).count();
        if file_count == 0 {
            continue;
        }

        total_count += file_count;

        if head_limit.is_some_and(|limit| counts.len() >= limit) {
            continue;
        }

        let display_path = workspace_root
            .join(&file.relative_path)
            .display()
            .to_string();
        counts.push((display_path, file_count));
    }

    Ok((counts, total_count))
}

fn truncate_display_text_by_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }

    (
        truncate_str_to_byte_boundary(text, max_bytes).to_string(),
        true,
    )
}

fn truncate_str_to_byte_boundary(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut cut = max_bytes;
    while cut > 0 && !value.is_char_boundary(cut) {
        cut -= 1;
    }
    &value[..cut]
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
        "invalid regex pattern: {err}\nHint: grep patterns are regular expressions. Escape regex metacharacters before retrying (for example, `{}`).",
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

fn append_grep_entries(
    output: &mut Vec<GrepOutputEntry>,
    workspace_root: &Path,
    relative_path: &str,
    lines: &[String],
    match_line_indexes: &[usize],
    context: usize,
) {
    let display_path = workspace_root.join(relative_path).display().to_string();
    if context == 0 {
        for &line_idx in match_line_indexes {
            output.push(grep_output_entry(&display_path, lines, line_idx));
        }
        return;
    }

    for line_idx in context_line_indexes(lines.len(), match_line_indexes, context) {
        output.push(grep_output_entry(&display_path, lines, line_idx));
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

fn grep_output_entry(path: &str, lines: &[String], line_idx: usize) -> GrepOutputEntry {
    GrepOutputEntry {
        path: path.to_string(),
        line_number: line_idx + 1,
        text: lines[line_idx].clone(),
    }
}

fn render_grep_match_line(entry: &GrepOutputEntry) -> String {
    format!("{}:{}: {}", entry.path, entry.line_number, entry.text)
}

fn render_grep_display(
    entries: &[GrepOutputEntry],
    total_count: usize,
    is_truncated: bool,
) -> String {
    if entries.is_empty() {
        return "No files found".to_string();
    }

    let mut output = vec![format!(
        "Found {total_count} matches{}",
        if is_truncated {
            " (more matches available)"
        } else {
            ""
        }
    )];
    let mut current = "";
    for entry in entries {
        if current != entry.path {
            if !current.is_empty() {
                output.push(String::new());
            }
            current = &entry.path;
            output.push(format!("{}:", entry.path));
        }
        output.push(format!("  Line {}: {}", entry.line_number, entry.text));
    }

    if is_truncated {
        output.push(String::new());
        output.push(
            "(Results truncated. Consider using a more specific path or pattern.)".to_string(),
        );
    }

    output.join("\n")
}

fn read_utf8_lines(path: &Path) -> Result<Utf8FileLines, ToolError> {
    let bytes = std::fs::read(path).map_err(|err| {
        ToolError::Execution(format!("failed to read file {}: {err}", path.display()))
    })?;

    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Utf8FileLines::NonUtf8);
    };

    Ok(Utf8FileLines::Lines(
        text.lines().map(str::to_owned).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolContext, ToolRunState};
    use serde_json::{json, Value};

    use super::{
        collect_grep_matches, FsGrepTool, GrepOutputMode, GrepSearch, MAX_GREP_RENDER_BYTES,
    };

    fn grep_search(pattern: &str) -> GrepSearch<'_> {
        GrepSearch {
            pattern,
            literal: false,
            include: None,
            limit: 100,
            context: 0,
            output_mode: GrepOutputMode::Content,
            head_limit: None,
        }
    }

    fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-fs-grep-tests".into(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: workspace_root.join(".artifacts"),
            actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
            category: Some("deep".to_string()),
            tool_call_id: tool_call_id.into(),
            current_model_ref: None,
            current_model_settings: None,
            tool_state: ToolRunState::default(),
            external_directory_allow_prefixes: Vec::new(),
            coordinator,
        }
    }

    fn create_dir(root: &Path, path: &str) {
        fs::create_dir_all(root.join(path)).unwrap_or_abort();
    }

    fn write_file(root: &Path, path: &str, contents: impl AsRef<[u8]>) {
        fs::write(root.join(path), contents).unwrap_or_abort();
    }

    #[test]
    fn collect_grep_matches_finds_matches_with_context_and_skips_ignored_directories() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();

        create_dir(root, "docs");
        create_dir(root, "src");
        create_dir(root, "target/build");
        create_dir(root, ".git/objects");
        create_dir(root, ".agent-harness/sessions/run-1");

        write_file(root, "docs/todo.md", "TODO docs\nnote\n");
        write_file(
            root,
            "src/main.txt",
            "alpha\nTODO first\nbeta\nTODO second\ngamma\n",
        );
        write_file(root, "target/build/generated.txt", "TODO hidden");
        write_file(root, ".git/objects/cache.txt", "TODO hidden");
        write_file(root, ".agent-harness/sessions/run-1/log.txt", "TODO hidden");
        write_file(
            root,
            "src/binary.bin",
            [0xff_u8, 0xfe, 0x00, 0x54, 0x4f, 0x44, 0x4f],
        );

        let result = collect_grep_matches(
            root,
            root,
            GrepSearch {
                context: 1,
                ..grep_search("TODO")
            },
            MAX_GREP_RENDER_BYTES,
        )
        .unwrap_or_abort();

        assert_eq!(result.total_count, 3);
        assert_eq!(result.returned_count, 3);
        assert_eq!(result.truncated_count, 0);
        assert!(!result.is_truncated);
        assert_eq!(result.lines.len(), 7);
        assert!(result.lines[0].ends_with("docs/todo.md:1: TODO docs"));
        assert!(result.lines[1].ends_with("docs/todo.md:2: note"));
        assert!(result.lines[2].ends_with("src/main.txt:1: alpha"));
        assert!(result.lines[3].ends_with("src/main.txt:2: TODO first"));
        assert!(result.lines[4].ends_with("src/main.txt:3: beta"));
        assert!(result.lines[5].ends_with("src/main.txt:4: TODO second"));
        assert!(result.lines[6].ends_with("src/main.txt:5: gamma"));
        assert!(result.display_text.contains("Found 3 matches"));
        assert!(result.display_text.contains("docs/todo.md:"));
        assert!(result.display_text.contains("  Line 1: TODO docs"));
    }

    #[test]
    fn collect_grep_matches_applies_include_filter_and_limit() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();

        write_file(root, "a.txt", "TODO a\n");
        write_file(root, "b.log", "TODO b\n");
        write_file(root, "c.txt", "TODO c\n");

        let result = collect_grep_matches(
            root,
            root,
            GrepSearch {
                include: Some("*.txt"),
                limit: 1,
                ..grep_search("TODO")
            },
            MAX_GREP_RENDER_BYTES,
        )
        .unwrap_or_abort();

        assert_eq!(result.total_count, 2);
        assert_eq!(result.returned_count, 1);
        assert_eq!(result.truncated_count, 1);
        assert!(result.is_truncated);
        assert_eq!(result.lines.len(), 1);
        assert!(result.lines[0].ends_with("a.txt:1: TODO a"));
        assert!(result
            .display_text
            .contains("Found 2 matches (more matches available)"));
        assert!(result.display_text.contains("  Line 1: TODO a"));
    }

    #[tokio::test]
    async fn fs_grep_accepts_exact_file_path_without_searching_siblings() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        create_dir(root, "src");
        write_file(root, "src/session_lineage.rs", "fn fork_point() {}\n");
        write_file(root, "src/other.rs", "fn fork_point() {}\n");

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
            .unwrap_or_abort();

        assert!(result.display_text.contains("Found 1 matches"));
        assert!(result.display_text.contains("src/session_lineage.rs:"));
        assert!(result.display_text.contains("  Line 1: fn fork_point() {}"));
        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(
            structured.get("path"),
            Some(&json!("src/session_lineage.rs"))
        );
        assert_eq!(structured.get("total_count"), Some(&json!(1)));
    }

    #[test]
    fn collect_grep_matches_invalid_regex_suggests_escaping_regex_metacharacters() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "notes.txt", "task(run_in_background\n");

        // act
        let err = collect_grep_matches(
            root,
            root,
            grep_search("task(run_in_background"),
            MAX_GREP_RENDER_BYTES,
        )
        .expect_err("unescaped regex group should fail");

        // assert
        let message = err.to_string();
        assert!(message.contains("invalid regex pattern"));
        assert!(message.contains("task\\(run_in_background"));
        assert!(!message.contains("literal"));
        assert!(!message.contains("context"));
    }

    #[test]
    fn collect_grep_matches_literal_search_escapes_regex_metacharacters() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "notes.txt", "task(run_in_background\ntaskXrun\n");

        let result = collect_grep_matches(
            root,
            root,
            GrepSearch {
                literal: true,
                ..grep_search("task(run_in_background")
            },
            MAX_GREP_RENDER_BYTES,
        )
        .unwrap_or_abort();

        assert_eq!(result.total_count, 1);
        assert_eq!(result.lines.len(), 1);
        assert!(result.lines[0].ends_with("notes.txt:1: task(run_in_background"));
    }

    #[tokio::test]
    async fn fs_grep_accepts_absolute_workspace_path() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "main.rs", "#[tokio::main]\nfn main() {}\n");

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
            .unwrap_or_abort();

        assert!(result.display_text.contains("main.rs:"));
        assert!(result.display_text.contains("  Line 1: #[tokio::main]"));
        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(structured.get("path"), Some(&json!(".")));
    }

    #[tokio::test]
    async fn fs_grep_files_with_matches_returns_file_paths() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "a.txt", "TODO a\n");
        write_file(root, "b.log", "TODO b\n");
        write_file(root, "c.txt", "no match\n");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-files-with-matches"),
                json!({
                    "pattern": "TODO",
                    "output_mode": "files_with_matches"
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(
            structured.get("output_mode"),
            Some(&json!("files_with_matches"))
        );
        let files = structured
            .get("files")
            .and_then(|f| f.as_array())
            .unwrap_or_abort();
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|f| f.as_str().is_some_and(|s| s.ends_with("a.txt"))));
        assert!(files
            .iter()
            .any(|f| f.as_str().is_some_and(|s| s.ends_with("b.log"))));
        assert_eq!(structured.get("total_count"), Some(&json!(2)));
        assert_eq!(structured.get("returned_count"), Some(&json!(2)));
        assert_eq!(structured.get("truncated"), Some(&json!(false)));
        assert!(result.display_text.contains("Found 2 files with matches"));
        assert!(result.display_text.contains("a.txt"));
        assert!(result.display_text.contains("b.log"));
        assert!(!result.display_text.contains("c.txt"));
    }

    #[tokio::test]
    async fn fs_grep_count_returns_per_file_match_counts() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "a.txt", "TODO a\nTODO b\n");
        write_file(root, "b.log", "TODO c\n");
        write_file(root, "c.txt", "no match\n");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-count"),
                json!({
                    "pattern": "TODO",
                    "output_mode": "count"
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(structured.get("output_mode"), Some(&json!("count")));
        let counts = structured
            .get("counts")
            .and_then(|c| c.as_array())
            .unwrap_or_abort();
        assert_eq!(counts.len(), 2);
        let a_entry = counts
            .iter()
            .find(|e| {
                e.get("file")
                    .and_then(|f| f.as_str())
                    .is_some_and(|s| s.ends_with("a.txt"))
            })
            .unwrap_or_abort();
        assert_eq!(a_entry.get("count"), Some(&json!(2)));
        let b_entry = counts
            .iter()
            .find(|e| {
                e.get("file")
                    .and_then(|f| f.as_str())
                    .is_some_and(|s| s.ends_with("b.log"))
            })
            .unwrap_or_abort();
        assert_eq!(b_entry.get("count"), Some(&json!(1)));
        assert_eq!(structured.get("total_count"), Some(&json!(3)));
        assert_eq!(structured.get("returned_count"), Some(&json!(2)));
        assert!(result.display_text.contains("Found 3 matches across files"));
        assert!(result.display_text.contains("a.txt: 2"));
        assert!(result.display_text.contains("b.log: 1"));
    }

    #[tokio::test]
    async fn fs_grep_head_limit_restricts_files_in_files_with_matches_mode() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "a.txt", "TODO a\n");
        write_file(root, "b.txt", "TODO b\n");
        write_file(root, "c.txt", "TODO c\n");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-head-limit-files"),
                json!({
                    "pattern": "TODO",
                    "output_mode": "files_with_matches",
                    "head_limit": 2
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        let files = structured
            .get("files")
            .and_then(|f| f.as_array())
            .unwrap_or_abort();
        assert_eq!(files.len(), 2);
        assert_eq!(structured.get("total_count"), Some(&json!(3)));
        assert_eq!(structured.get("returned_count"), Some(&json!(2)));
        assert_eq!(structured.get("truncated"), Some(&json!(true)));
        assert!(result.display_text.contains("Found 3 files with matches"));
        assert!(result.display_text.contains("more files available"));
    }

    #[tokio::test]
    async fn fs_grep_head_limit_restricts_files_in_count_mode() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "a.txt", "TODO a\n");
        write_file(root, "b.txt", "TODO b\nTODO c\n");
        write_file(root, "c.txt", "TODO d\n");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-head-limit-count"),
                json!({
                    "pattern": "TODO",
                    "output_mode": "count",
                    "head_limit": 2
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        let counts = structured
            .get("counts")
            .and_then(|c| c.as_array())
            .unwrap_or_abort();
        assert_eq!(counts.len(), 2);
        assert_eq!(structured.get("total_count"), Some(&json!(4)));
        assert_eq!(structured.get("truncated"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn fs_grep_content_default_behavior_unchanged_without_output_mode() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "a.txt", "TODO a\n");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-content-default"),
                json!({
                    "pattern": "TODO"
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(structured.get("output_mode"), Some(&json!("content")));
        assert!(structured.get("matches").is_some());
        assert_eq!(structured.get("total_count"), Some(&json!(1)));
        assert!(result.display_text.contains("Found 1 matches"));
        assert!(result.display_text.contains("  Line 1: TODO a"));
    }

    #[tokio::test]
    async fn fs_grep_head_limit_in_content_mode_limits_files_returned() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "a.txt", "TODO a\n");
        write_file(root, "b.txt", "TODO b\n");
        write_file(root, "c.txt", "TODO c\n");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-head-limit-content"),
                json!({
                    "pattern": "TODO",
                    "head_limit": 1
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(structured.get("output_mode"), Some(&json!("content")));
        assert_eq!(structured.get("total_count"), Some(&json!(3)));
        let matches = structured
            .get("matches")
            .and_then(|m| m.as_array())
            .unwrap_or_abort();
        assert_eq!(matches.len(), 1);
        assert!(matches[0]
            .as_str()
            .is_some_and(|s| s.ends_with("a.txt:1: TODO a")));
    }

    #[tokio::test]
    async fn fs_grep_head_limit_zero_treated_as_no_limit() {
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let root = tempdir.path();
        write_file(root, "a.txt", "TODO a\n");
        write_file(root, "b.txt", "TODO b\n");

        let result = FsGrepTool
            .call(
                test_context(root, "grep-head-limit-zero"),
                json!({
                    "pattern": "TODO",
                    "output_mode": "files_with_matches",
                    "head_limit": 0
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        let files = structured
            .get("files")
            .and_then(|f| f.as_array())
            .unwrap_or_abort();
        assert_eq!(files.len(), 2);
        assert_eq!(structured.get("truncated"), Some(&json!(false)));
    }
}
