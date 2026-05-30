use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use globset::{Glob, GlobSet, GlobSetBuilder};
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::TextDiff;
use tokio::process::Command;
use tokio::time::timeout;

use crate::fs_walk::{
    collect_workspace_files, normalize_workspace_relative_entry, workspace_file_from_path,
    WorkspaceFile,
};
use crate::hashline_apply::write_atomic;
use crate::limit_summary::summarize_limit;
use crate::workspace_paths::{
    canonical_workspace_root, ensure_within_workspace_path, normalize_workspace_target_path,
};
use crate::{parse_tool_args, text_json_artifacts_tool_result, text_json_tool_result};

const DEFAULT_AST_GREP_LIMIT: usize = 100;
const MAX_AST_GREP_LIMIT: usize = 200;
const MAX_AST_GREP_CONTEXT: usize = 5;
const MAX_MATCH_TEXT_CHARS: usize = 4_000;
const MAX_SNIPPET_CHARS: usize = 8_000;
const MAX_INLINE_JSON_CHARS: usize = 24_000;
const AST_GREP_TIMEOUT_SECS: u64 = 30;

pub(crate) struct AstGrepSearchTool {
    command: String,
}

pub(crate) struct AstGrepReplaceTool {
    command: String,
}

impl AstGrepSearchTool {
    pub(crate) fn new() -> Self {
        Self::with_command("ast-grep")
    }

    pub(crate) fn with_command(command: impl Into<String>) -> Self {
        Self {
            command: ast_grep_command(command.into()),
        }
    }
}

impl AstGrepReplaceTool {
    pub(crate) fn new() -> Self {
        Self::with_command("ast-grep")
    }

    pub(crate) fn with_command(command: impl Into<String>) -> Self {
        Self {
            command: ast_grep_command(command.into()),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AstGrepSearchArgs {
    pattern: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    context: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AstGrepReplaceArgs {
    pattern: String,
    rewrite: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    mode: AstGrepReplaceMode,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AstGrepReplaceMode {
    #[default]
    DryRun,
    Apply,
}

impl AstGrepReplaceMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DryRun => "dry_run",
            Self::Apply => "apply",
        }
    }
}

#[derive(Debug, Clone)]
struct SearchRoot {
    canonical: PathBuf,
    relative: String,
}

#[derive(Debug, Clone)]
struct AstGrepMatch {
    file_path: String,
    language: String,
    start_line: u64,
    start_column: u64,
    end_line: u64,
    end_column: u64,
    matched_text: String,
    matched_text_truncated: bool,
    byte_start: Option<u64>,
    byte_end: Option<u64>,
    snippet: String,
    snippet_truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AstGrepCliMatch {
    text: String,
    range: AstGrepCliRange,
    file: String,
    #[serde(default)]
    lines: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    replacement: Option<String>,
    #[serde(default)]
    replacement_offsets: Option<AstGrepCliByteRange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AstGrepCliRange {
    #[serde(default)]
    byte_offset: Option<AstGrepCliByteRange>,
    start: AstGrepCliPosition,
    end: AstGrepCliPosition,
}

#[derive(Debug, Clone, Deserialize)]
struct AstGrepCliByteRange {
    start: u64,
    end: u64,
}

#[derive(Debug, Deserialize)]
struct AstGrepCliPosition {
    line: u64,
    column: u64,
}

#[derive(Debug, Clone)]
struct AstGrepReplacement {
    file_path: String,
    resolved_path: PathBuf,
    language: String,
    start_line: u64,
    start_column: u64,
    end_line: u64,
    end_column: u64,
    match_byte_start: usize,
    match_byte_end: usize,
    byte_start: usize,
    byte_end: usize,
    matched_text: String,
    matched_text_truncated: bool,
    replacement: String,
    replacement_truncated: bool,
}

#[derive(Debug)]
struct ReplacementFilePlan {
    file_path: String,
    resolved_path: PathBuf,
    before: String,
    after: String,
    diff: String,
    replacement_count: usize,
}

#[async_trait]
impl Tool for AstGrepSearchTool {
    fn id(&self) -> &str {
        "ast_grep_search"
    }

    fn description(&self) -> &str {
        "Performs read-only structural code search through the ast-grep CLI with workspace path safety, explicit or safely inferred language, hard caps, and artifact spill for large output."
    }

    fn parameters_json_schema(&self) -> Value {
        crate::json_schema_for::<AstGrepSearchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: AstGrepSearchArgs = parse_tool_args(args_json)?;
        let pattern = validate_pattern(&args.pattern)?;
        let workspace = canonical_workspace_root(&ctx)?;
        let include = compile_glob_set(&args.include, "include")?;
        let exclude = compile_glob_set(&args.exclude, "exclude")?;
        let search_roots = search_roots(&workspace, &args.paths)?;
        let limit = clamp_limit(args.limit, DEFAULT_AST_GREP_LIMIT, MAX_AST_GREP_LIMIT, 1);
        let context = clamp_limit(args.context, 0, MAX_AST_GREP_CONTEXT, 0);
        let explicit_language = args
            .language
            .as_deref()
            .map(normalize_language)
            .transpose()?;
        let language = match explicit_language.clone() {
            Some(language) => language,
            None => infer_single_language(
                &workspace,
                &search_roots,
                include.as_ref(),
                exclude.as_ref(),
            )?,
        };

        let adapter = run_ast_grep(AstGrepRunRequest {
            workspace: &workspace,
            roots: &search_roots,
            pattern: &pattern,
            rewrite: None,
            language: &language,
            include: &args.include,
            exclude: &args.exclude,
            context: context.effective,
            command_name: &self.command,
            tool_id: "ast_grep_search",
        })
        .await?;

        let all_matches = adapter
            .matches
            .into_iter()
            .map(|matched| cli_match_to_match(matched, &language))
            .collect::<Vec<_>>();
        let total_count = all_matches.len();
        let limit_summary = summarize_limit(total_count, limit.effective);
        let returned_matches = all_matches
            .into_iter()
            .take(limit_summary.returned_count)
            .map(|matched| {
                json!({
                    "file_path": matched.file_path,
                    "range": {
                        "start": { "line": matched.start_line, "column": matched.start_column },
                        "end": { "line": matched.end_line, "column": matched.end_column },
                    },
                    "language": matched.language,
                    "byte_range": {
                        "start": matched.byte_start,
                        "end": matched.byte_end,
                    },
                    "matched_text": matched.matched_text,
                    "matched_text_truncated": matched.matched_text_truncated,
                    "snippet": matched.snippet,
                    "snippet_truncated": matched.snippet_truncated,
                })
            })
            .collect::<Vec<_>>();
        let returned_count = returned_matches.len();

        let payload = json!({
            "source": "ast_grep_cli_adapter",
            "adapter": {
                "name": "ast-grep",
                "status": "available",
                "command": format!("{} run", self.command),
                "warnings": adapter.warnings,
                "exit_status": adapter.exit_status,
            },
            "pattern": args.pattern,
            "language": language,
            "language_inference": if explicit_language.is_some() { "explicit" } else { "single_language_from_paths" },
            "paths": args.paths,
            "include": args.include,
            "exclude": args.exclude,
            "context": context.effective,
            "requested_context": context.requested,
            "effective_context": context.effective,
            "max_context": MAX_AST_GREP_CONTEXT,
            "context_clamped": context.clamped,
            "limit": limit.effective,
            "requested_limit": limit.requested,
            "effective_limit": limit.effective,
            "max_limit": MAX_AST_GREP_LIMIT,
            "limit_clamped": limit.clamped,
            "per_match_caps": {
                "matched_text_chars": MAX_MATCH_TEXT_CHARS,
                "snippet_chars": MAX_SNIPPET_CHARS,
            },
            "total_count": total_count,
            "returned_count": returned_count,
            "truncated_count": limit_summary.truncated_count,
            "truncated": limit_summary.is_truncated,
            "matches": returned_matches,
        });

        if returned_count == 0 {
            return Ok(text_json_tool_result("No structural matches.", payload));
        }
        maybe_spill_json(
            &ctx,
            "ast-grep-search.json",
            format!("{returned_count} structural match(es)"),
            payload,
        )
    }
}

#[async_trait]
impl Tool for AstGrepReplaceTool {
    fn id(&self) -> &str {
        "ast_grep_replace"
    }

    fn description(&self) -> &str {
        "Plans or applies structural code rewrites through ast-grep JSON rewrite output. The adapter never mutates the workspace directly; apply mode writes files only through the Harness edit-permission path with workspace path checks, atomic writes, and diff artifacts. Defaults to dry_run."
    }

    fn parameters_json_schema(&self) -> Value {
        crate::json_schema_for::<AstGrepReplaceArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: AstGrepReplaceArgs = parse_tool_args(args_json)?;
        let pattern = validate_pattern_for_tool("ast_grep_replace", &args.pattern)?;
        let workspace = canonical_workspace_root(&ctx)?;
        let include = compile_glob_set(&args.include, "include")?;
        let exclude = compile_glob_set(&args.exclude, "exclude")?;
        let search_roots = search_roots(&workspace, &args.paths)?;
        let limit = clamp_limit(args.limit, DEFAULT_AST_GREP_LIMIT, MAX_AST_GREP_LIMIT, 1);
        let explicit_language = args
            .language
            .as_deref()
            .map(normalize_language)
            .transpose()?;
        let language = match explicit_language.clone() {
            Some(language) => language,
            None => infer_single_language(
                &workspace,
                &search_roots,
                include.as_ref(),
                exclude.as_ref(),
            )?,
        };

        let adapter = run_ast_grep(AstGrepRunRequest {
            workspace: &workspace,
            roots: &search_roots,
            pattern: &pattern,
            rewrite: Some(&args.rewrite),
            language: &language,
            include: &args.include,
            exclude: &args.exclude,
            context: 0,
            command_name: &self.command,
            tool_id: "ast_grep_replace",
        })
        .await?;

        let all_replacements = adapter
            .matches
            .into_iter()
            .map(|matched| cli_match_to_replacement(&workspace, &search_roots, matched, &language))
            .collect::<Result<Vec<_>, _>>()?;
        let total_count = all_replacements.len();
        let limit_summary = summarize_limit(total_count, limit.effective);
        if args.mode == AstGrepReplaceMode::Apply && limit_summary.is_truncated {
            return Err(ToolError::Execution(format!(
                "ast_grep_replace refused to apply {total_count} replacement(s) because the effective limit is {}; narrow paths/include globs or run dry_run first",
                limit.effective
            )));
        }
        let replacements = all_replacements
            .into_iter()
            .take(limit_summary.returned_count)
            .collect::<Vec<_>>();
        let returned_count = replacements.len();
        let plans = plan_replacements(&replacements)?;
        let diff_artifact = if plans.is_empty() {
            None
        } else {
            Some(write_replace_diff_artifact(
                &ctx,
                args.mode,
                &combined_diff(&plans),
            )?)
        };

        if args.mode == AstGrepReplaceMode::Apply {
            for plan in &plans {
                write_atomic(&plan.resolved_path, &plan.after)?;
            }
        }

        let edits = replacements
            .iter()
            .map(replacement_json)
            .collect::<Vec<_>>();
        let files = plans
            .iter()
            .map(|plan| {
                json!({
                    "file_path": plan.file_path,
                    "resolved_path": plan.resolved_path.display().to_string(),
                    "replacement_count": plan.replacement_count,
                    "changed": plan.before != plan.after,
                })
            })
            .collect::<Vec<_>>();
        let mut artifacts = Vec::new();
        let diff_artifact_json = diff_artifact.as_ref().map(|artifact| {
            artifacts.push(artifact.clone());
            json!({
                "path": artifact.path,
                "digest": artifact.digest,
            })
        });
        let payload = json!({
            "source": "ast_grep_cli_adapter",
            "adapter": {
                "name": "ast-grep",
                "status": "available",
                "command": format!("{} run", self.command),
                "mutation": "json_rewrite_dry_run_only",
                "warnings": adapter.warnings,
                "exit_status": adapter.exit_status,
            },
            "pattern": args.pattern,
            "rewrite": args.rewrite,
            "language": language,
            "language_inference": if explicit_language.is_some() { "explicit" } else { "single_language_from_paths" },
            "mode": args.mode.as_str(),
            "applied": args.mode == AstGrepReplaceMode::Apply,
            "paths": args.paths,
            "include": args.include,
            "exclude": args.exclude,
            "limit": limit.effective,
            "requested_limit": limit.requested,
            "effective_limit": limit.effective,
            "max_limit": MAX_AST_GREP_LIMIT,
            "limit_clamped": limit.clamped,
            "per_match_caps": {
                "matched_text_chars": MAX_MATCH_TEXT_CHARS,
                "replacement_chars": MAX_MATCH_TEXT_CHARS,
            },
            "total_count": total_count,
            "returned_count": returned_count,
            "truncated_count": limit_summary.truncated_count,
            "truncated": limit_summary.is_truncated,
            "files": files,
            "diff_artifact": diff_artifact_json,
            "edits": edits,
        });

        if returned_count == 0 {
            return Ok(text_json_tool_result(
                "No structural replacements.",
                payload,
            ));
        }
        maybe_spill_json_with_artifacts(
            &ctx,
            "ast-grep-replace.json",
            format!(
                "{} structural replacement(s) {}; diff artifact {}",
                returned_count,
                if args.mode == AstGrepReplaceMode::Apply {
                    "applied"
                } else {
                    "planned (dry run, no files written)"
                },
                diff_artifact
                    .as_ref()
                    .map(|artifact| artifact.path.as_str())
                    .unwrap_or("<none>")
            ),
            payload,
            artifacts,
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct ClampedLimit {
    requested: Option<usize>,
    effective: usize,
    clamped: bool,
}

fn clamp_limit(
    requested: Option<u32>,
    default_value: usize,
    max_value: usize,
    min_value: usize,
) -> ClampedLimit {
    let requested = requested.map(|value| value as usize);
    let raw = requested.unwrap_or(default_value);
    let effective = raw.clamp(min_value.min(max_value), max_value);
    ClampedLimit {
        requested,
        effective,
        clamped: raw != effective,
    }
}

fn validate_pattern(pattern: &str) -> Result<String, ToolError> {
    validate_pattern_for_tool("ast_grep_search", pattern)
}

fn validate_pattern_for_tool(tool_id: &str, pattern: &str) -> Result<String, ToolError> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Err(ToolError::InvalidArguments(format!(
            "{tool_id} pattern cannot be empty"
        )));
    }
    Ok(trimmed.to_string())
}

fn ast_grep_command(command: String) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        "ast-grep".to_string()
    } else {
        trimmed.to_string()
    }
}

fn search_roots(workspace: &Path, paths: &[String]) -> Result<Vec<SearchRoot>, ToolError> {
    if paths.is_empty() {
        return Ok(vec![SearchRoot {
            canonical: workspace.to_path_buf(),
            relative: ".".to_string(),
        }]);
    }
    paths
        .iter()
        .map(|path| {
            if path.contains("..") {
                return Err(ToolError::InvalidArguments(
                    "ast_grep_search paths cannot contain parent traversal".to_string(),
                ));
            }
            let candidate = normalize_workspace_target_path(workspace, Path::new(path))?;
            let canonical = candidate.canonicalize().map_err(|err| {
                ToolError::Execution(format!("failed to resolve path {path}: {err}"))
            })?;
            ensure_within_workspace_path(workspace, &canonical)?;
            if !(canonical.is_file() || canonical.is_dir()) {
                return Err(ToolError::InvalidArguments(format!(
                    "path `{path}` must resolve to a file or directory"
                )));
            }
            Ok(SearchRoot {
                relative: normalize_workspace_relative_entry(workspace, &canonical)?,
                canonical,
            })
        })
        .collect()
}

fn infer_single_language(
    workspace: &Path,
    roots: &[SearchRoot],
    include: Option<&GlobSet>,
    exclude: Option<&GlobSet>,
) -> Result<String, ToolError> {
    let mut languages = BTreeSet::new();
    for root in roots {
        for file in collect_candidate_files(workspace, root, include, exclude)? {
            if let Some(language) = infer_language_from_path(Path::new(&file.relative_path)) {
                languages.insert(language);
            }
            if languages.len() > 1 {
                return Err(ToolError::InvalidArguments(format!(
                    "ast_grep_search language is required when paths resolve to multiple supported languages: {}; pass `language` explicitly",
                    languages.into_iter().collect::<Vec<_>>().join(", ")
                )));
            }
        }
    }
    languages.into_iter().next().ok_or_else(|| {
        ToolError::InvalidArguments(
            "ast_grep_search could not infer a supported language from the selected paths; pass `language` explicitly".to_string(),
        )
    })
}

fn collect_candidate_files(
    workspace: &Path,
    root: &SearchRoot,
    include: Option<&GlobSet>,
    exclude: Option<&GlobSet>,
) -> Result<Vec<WorkspaceFile>, ToolError> {
    let mut files = if root.canonical.is_file() {
        vec![workspace_file_from_path(workspace, &root.canonical)?]
    } else {
        collect_workspace_files(workspace, &root.canonical)?
    };
    files.retain(|file| {
        include.is_none_or(|set| set.is_match(&file.relative_path))
            && exclude.is_none_or(|set| !set.is_match(&file.relative_path))
            && infer_language_from_path(Path::new(&file.relative_path)).is_some()
    });
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

#[derive(Debug)]
struct AstGrepAdapterOutput {
    matches: Vec<AstGrepCliMatch>,
    warnings: Vec<String>,
    exit_status: Option<i32>,
}

struct AstGrepRunRequest<'a> {
    workspace: &'a Path,
    roots: &'a [SearchRoot],
    pattern: &'a str,
    rewrite: Option<&'a str>,
    language: &'a str,
    include: &'a [String],
    exclude: &'a [String],
    context: usize,
    command_name: &'a str,
    tool_id: &'a str,
}

async fn run_ast_grep(request: AstGrepRunRequest<'_>) -> Result<AstGrepAdapterOutput, ToolError> {
    let mut command = Command::new(request.command_name);
    command
        .kill_on_drop(true)
        .current_dir(request.workspace)
        .arg("run")
        .arg("--pattern")
        .arg(request.pattern)
        .arg("--lang")
        .arg(request.language)
        .arg("--json=compact")
        .arg("--color")
        .arg("never")
        .arg("--heading")
        .arg("never")
        .arg("--context")
        .arg(request.context.to_string());
    if let Some(rewrite) = request.rewrite {
        command.arg("--rewrite").arg(rewrite);
    }
    for pattern in request.include {
        command.arg("--globs").arg(pattern);
    }
    for pattern in request.exclude {
        command.arg("--globs").arg(format!("!{pattern}"));
    }
    for root in request.roots {
        command.arg(&root.relative);
    }

    let output = timeout(Duration::from_secs(AST_GREP_TIMEOUT_SECS), command.output())
        .await
        .map_err(|_| {
            ToolError::Execution(format!(
                "ast-grep adapter timed out after {AST_GREP_TIMEOUT_SECS}s; narrow paths, include globs, or pattern"
            ))
        })?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                ToolError::Execution(format!(
                    "{} requires the `ast-grep` binary on PATH; install ast-grep or remove this tool from the active toolset",
                    request.tool_id
                ))
            } else {
                ToolError::Execution(format!("failed to run ast-grep adapter: {err}"))
            }
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if stderr.contains("Pattern contains an ERROR node") {
        return Err(ToolError::InvalidArguments(format!(
            "ast-grep could not parse pattern `{}` cleanly for language `{}`: {}",
            request.pattern,
            request.language,
            compact_message(&stderr)
        )));
    }

    let warnings = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(cap_warning)
        .collect::<Vec<_>>();
    let parsed = serde_json::from_str::<Vec<AstGrepCliMatch>>(stdout.trim()).map_err(|err| {
        let status = output.status.code();
        let stderr = compact_message(&stderr);
        ToolError::Execution(format!(
            "failed to parse ast-grep JSON output for language `{}` (status {status:?}): {err}; stderr: {stderr}",
            request.language
        ))
    })?;

    if !(output.status.success() || output.status.code() == Some(1) && parsed.is_empty()) {
        return Err(ToolError::Execution(format!(
            "ast-grep search failed for language `{}` with status {:?}: {}",
            request.language,
            output.status.code(),
            compact_message(&stderr)
        )));
    }

    Ok(AstGrepAdapterOutput {
        matches: parsed,
        warnings,
        exit_status: output.status.code(),
    })
}

fn cli_match_to_match(matched: AstGrepCliMatch, requested_language: &str) -> AstGrepMatch {
    let (matched_text, matched_text_truncated) = cap_text(&matched.text, MAX_MATCH_TEXT_CHARS);
    let snippet_source = matched.lines.as_deref().unwrap_or(&matched.text);
    let (snippet, snippet_truncated) = cap_text(snippet_source, MAX_SNIPPET_CHARS);
    let byte_start = matched.range.byte_offset.as_ref().map(|range| range.start);
    let byte_end = matched.range.byte_offset.as_ref().map(|range| range.end);
    AstGrepMatch {
        file_path: normalize_adapter_file_path(&matched.file),
        language: matched
            .language
            .as_deref()
            .map(normalize_output_language)
            .unwrap_or_else(|| requested_language.to_string()),
        start_line: matched.range.start.line + 1,
        start_column: matched.range.start.column + 1,
        end_line: matched.range.end.line + 1,
        end_column: matched.range.end.column + 1,
        matched_text,
        matched_text_truncated,
        byte_start,
        byte_end,
        snippet,
        snippet_truncated,
    }
}

fn cli_match_to_replacement(
    workspace: &Path,
    roots: &[SearchRoot],
    matched: AstGrepCliMatch,
    requested_language: &str,
) -> Result<AstGrepReplacement, ToolError> {
    let (file_path, resolved_path) =
        resolve_adapter_replacement_path(workspace, roots, &matched.file)?;
    let replacement = matched.replacement.ok_or_else(|| {
        ToolError::Execution(
            "ast_grep_replace adapter output did not include `replacement`; ensure ast-grep supports JSON rewrite output".to_string(),
        )
    })?;
    let match_byte_range = matched.range.byte_offset.ok_or_else(|| {
        ToolError::Execution(
            "ast_grep_replace adapter output did not include match byte offsets; cannot safely validate rewrite".to_string(),
        )
    })?;
    let replacement_byte_range = matched
        .replacement_offsets
        .unwrap_or_else(|| match_byte_range.clone());
    let match_byte_start = usize::try_from(match_byte_range.start).map_err(|_| {
        ToolError::Execution(
            "ast_grep_replace match byte range start exceeds platform size".to_string(),
        )
    })?;
    let match_byte_end = usize::try_from(match_byte_range.end).map_err(|_| {
        ToolError::Execution(
            "ast_grep_replace match byte range end exceeds platform size".to_string(),
        )
    })?;
    if match_byte_start > match_byte_end {
        return Err(ToolError::Execution(format!(
            "ast_grep_replace adapter returned an invalid match byte range {match_byte_start}..{match_byte_end} for {file_path}"
        )));
    }
    let byte_start = usize::try_from(replacement_byte_range.start).map_err(|_| {
        ToolError::Execution("ast_grep_replace byte range start exceeds platform size".to_string())
    })?;
    let byte_end = usize::try_from(replacement_byte_range.end).map_err(|_| {
        ToolError::Execution("ast_grep_replace byte range end exceeds platform size".to_string())
    })?;
    if byte_start > byte_end {
        return Err(ToolError::Execution(format!(
            "ast_grep_replace adapter returned an invalid byte range {byte_start}..{byte_end} for {file_path}"
        )));
    }
    if byte_start < match_byte_start || byte_end > match_byte_end {
        return Err(ToolError::Execution(format!(
            "ast_grep_replace adapter replacement range {byte_start}..{byte_end} escapes match range {match_byte_start}..{match_byte_end} for {file_path}"
        )));
    }

    Ok(AstGrepReplacement {
        file_path,
        resolved_path,
        language: matched
            .language
            .as_deref()
            .map(normalize_output_language)
            .unwrap_or_else(|| requested_language.to_string()),
        start_line: matched.range.start.line + 1,
        start_column: matched.range.start.column + 1,
        end_line: matched.range.end.line + 1,
        end_column: matched.range.end.column + 1,
        match_byte_start,
        match_byte_end,
        byte_start,
        byte_end,
        matched_text_truncated: matched.text.chars().count() > MAX_MATCH_TEXT_CHARS,
        matched_text: matched.text,
        replacement_truncated: replacement.chars().count() > MAX_MATCH_TEXT_CHARS,
        replacement,
    })
}

fn resolve_adapter_replacement_path(
    workspace: &Path,
    roots: &[SearchRoot],
    adapter_path: &str,
) -> Result<(String, PathBuf), ToolError> {
    let normalized = normalize_adapter_file_path(adapter_path);
    let candidate = normalize_workspace_target_path(workspace, Path::new(&normalized))?;
    let canonical = candidate.canonicalize().map_err(|err| {
        ToolError::Execution(format!(
            "ast_grep_replace failed to resolve adapter path `{adapter_path}`: {err}"
        ))
    })?;
    ensure_within_workspace_path(workspace, &canonical)?;
    if !roots.iter().any(|root| {
        if root.canonical.is_file() {
            canonical == root.canonical
        } else {
            canonical.starts_with(&root.canonical)
        }
    }) {
        return Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: adapter_path.to_string(),
        });
    }
    Ok((
        normalize_workspace_relative_entry(workspace, &canonical)?,
        canonical,
    ))
}

fn plan_replacements(
    replacements: &[AstGrepReplacement],
) -> Result<Vec<ReplacementFilePlan>, ToolError> {
    let mut by_file: BTreeMap<String, Vec<&AstGrepReplacement>> = BTreeMap::new();
    for replacement in replacements {
        by_file
            .entry(replacement.file_path.clone())
            .or_default()
            .push(replacement);
    }

    by_file
        .into_iter()
        .map(|(file_path, mut edits)| {
            edits.sort_by_key(|edit| (edit.byte_start, edit.byte_end));
            let resolved_path = edits
                .first()
                .map(|edit| edit.resolved_path.clone())
                .expect("group has at least one edit");
            let before = std::fs::read_to_string(&resolved_path).map_err(|err| {
                ToolError::Execution(format!(
                    "ast_grep_replace failed to read {file_path} before applying rewrite: {err}"
                ))
            })?;
            let mut previous_end = 0usize;
            for edit in &edits {
                if edit.match_byte_end > before.len()
                    || edit.byte_end > before.len()
                    || !before.is_char_boundary(edit.match_byte_start)
                    || !before.is_char_boundary(edit.match_byte_end)
                    || !before.is_char_boundary(edit.byte_start)
                    || !before.is_char_boundary(edit.byte_end)
                {
                    return Err(ToolError::Execution(format!(
                        "ast_grep_replace adapter byte range {}..{} is invalid for {file_path}",
                        edit.byte_start, edit.byte_end
                    )));
                }
                if edit.byte_start < edit.match_byte_start || edit.byte_end > edit.match_byte_end {
                    return Err(ToolError::Execution(format!(
                        "ast_grep_replace adapter replacement range {}..{} escapes match range {}..{} for {file_path}",
                        edit.byte_start, edit.byte_end, edit.match_byte_start, edit.match_byte_end
                    )));
                }
                if edit.byte_start < previous_end {
                    return Err(ToolError::Execution(format!(
                        "ast_grep_replace refused overlapping rewrites in {file_path}; narrow the pattern or run separate edits"
                    )));
                }
                let current = &before[edit.match_byte_start..edit.match_byte_end];
                if current != edit.matched_text {
                    return Err(ToolError::Execution(format!(
                        "ast_grep_replace refused stale adapter output for {file_path}; expected match byte range did not match current file contents"
                    )));
                }
                previous_end = edit.byte_end;
            }

            let mut after = before.clone();
            for edit in edits.iter().rev() {
                after.replace_range(edit.byte_start..edit.byte_end, &edit.replacement);
            }
            let diff = TextDiff::from_lines(&before, &after)
                .unified_diff()
                .to_string();
            Ok(ReplacementFilePlan {
                file_path,
                resolved_path,
                before,
                after,
                diff,
                replacement_count: edits.len(),
            })
        })
        .collect()
}

fn replacement_json(replacement: &AstGrepReplacement) -> Value {
    let (matched_text, matched_text_truncated) =
        cap_text(&replacement.matched_text, MAX_MATCH_TEXT_CHARS);
    let (replacement_text, replacement_text_truncated) =
        cap_text(&replacement.replacement, MAX_MATCH_TEXT_CHARS);
    json!({
        "file_path": replacement.file_path,
        "range": {
            "start": { "line": replacement.start_line, "column": replacement.start_column },
            "end": { "line": replacement.end_line, "column": replacement.end_column },
        },
        "language": replacement.language,
        "byte_range": {
            "start": replacement.byte_start,
            "end": replacement.byte_end,
        },
        "match_byte_range": {
            "start": replacement.match_byte_start,
            "end": replacement.match_byte_end,
        },
        "matched_text": matched_text,
        "matched_text_truncated": matched_text_truncated || replacement.matched_text_truncated,
        "replacement": replacement_text,
        "replacement_truncated": replacement_text_truncated || replacement.replacement_truncated,
    })
}

fn combined_diff(plans: &[ReplacementFilePlan]) -> String {
    let mut output = String::new();
    for plan in plans {
        output.push_str(&format!("diff --ast-grep-replace {}\n", plan.file_path));
        output.push_str(&plan.diff);
        if !output.ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

fn write_replace_diff_artifact(
    ctx: &ToolContext,
    mode: AstGrepReplaceMode,
    diff: &str,
) -> Result<ArtifactRef, ToolError> {
    ctx.artifact_store()
        .map_err(|err| ToolError::Execution(err.to_string()))?
        .write_text(&format!("ast-grep-replace-{}.diff", mode.as_str()), diff)
        .map_err(|err| {
            ToolError::Execution(format!(
                "failed to write ast_grep_replace diff artifact: {err}"
            ))
        })
}

fn normalize_adapter_file_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn cap_text(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let mut capped = text.chars().take(max_chars).collect::<String>();
    capped.push_str("…[truncated]");
    (capped, true)
}

fn cap_warning(line: &str) -> String {
    cap_text(line, 500).0
}

fn compact_message(message: &str) -> String {
    let compact = message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.is_empty() {
        "<no stderr>".to_string()
    } else {
        cap_text(&compact, 1_000).0
    }
}

fn compile_glob_set(patterns: &[String], label: &str) -> Result<Option<GlobSet>, ToolError> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        validate_glob_pattern(pattern, label)?;
        builder.add(Glob::new(pattern).map_err(|err| {
            ToolError::InvalidArguments(format!("invalid {label} glob `{pattern}`: {err}"))
        })?);
    }
    builder
        .build()
        .map(Some)
        .map_err(|err| ToolError::InvalidArguments(format!("invalid {label} globs: {err}")))
}

fn validate_glob_pattern(pattern: &str, label: &str) -> Result<(), ToolError> {
    if pattern.trim().is_empty() {
        return Err(ToolError::InvalidArguments(format!(
            "ast_grep_search {label} globs cannot be empty"
        )));
    }
    if pattern.contains("..") || pattern.starts_with('/') || pattern.starts_with('\\') {
        return Err(ToolError::InvalidArguments(format!(
            "ast_grep_search {label} glob `{pattern}` must stay within the workspace"
        )));
    }
    Ok(())
}

fn normalize_language(language: &str) -> Result<String, ToolError> {
    let normalized = match language.trim().to_ascii_lowercase().as_str() {
        "rs" | "rust" => "rust",
        "js" | "javascript" => "javascript",
        "ts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "py" | "python" => "python",
        "md" | "markdown" => "markdown",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        other => {
            return Err(ToolError::InvalidArguments(format!(
                "unsupported language `{other}`; expected rust, javascript, typescript, tsx, jsx, python, markdown, json, toml, or yaml"
            )));
        }
    };
    Ok(normalized.to_string())
}

fn normalize_output_language(language: &str) -> String {
    normalize_language(language).unwrap_or_else(|_| language.trim().to_ascii_lowercase())
}

fn infer_language_from_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    normalize_language(&ext).ok()
}

fn maybe_spill_json(
    ctx: &ToolContext,
    artifact_name: &str,
    display_text: String,
    payload: Value,
) -> Result<ToolResult, ToolError> {
    maybe_spill_json_with_artifacts(ctx, artifact_name, display_text, payload, Vec::new())
}

fn maybe_spill_json_with_artifacts(
    ctx: &ToolContext,
    artifact_name: &str,
    display_text: String,
    payload: Value,
    mut artifacts: Vec<ArtifactRef>,
) -> Result<ToolResult, ToolError> {
    let body = serde_json::to_string_pretty(&payload)
        .map_err(|err| ToolError::Execution(format!("failed to serialize output: {err}")))?;
    if body.len() <= MAX_INLINE_JSON_CHARS {
        if artifacts.is_empty() {
            return Ok(text_json_tool_result(display_text, payload));
        }
        return Ok(text_json_artifacts_tool_result(
            display_text,
            payload,
            artifacts,
        ));
    }
    let artifact = ctx
        .artifact_store()
        .map_err(|err| ToolError::Execution(err.to_string()))?
        .write_text(artifact_name, &body)
        .map_err(|err| ToolError::Execution(err.to_string()))?;
    let artifact_ref = ArtifactRef {
        path: artifact.path,
        digest: artifact.digest,
    };
    artifacts.push(artifact_ref.clone());
    Ok(text_json_artifacts_tool_result(
        format!(
            "{display_text}; full output spilled to {}",
            artifact_ref.path
        ),
        json!({
            "source": "ast_grep_cli_adapter",
            "spilled": true,
            "artifact": artifact_ref,
        }),
        artifacts,
    ))
}
