//! Built-in tool registry and tool implementations for filesystem, shell, and
//! hashline edit operations.
//!
//! Runtime policy lives in `harness-core`; this crate should focus on tool
//! argument validation, execution, and stable schema exposure.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::config::{McpConfig, ShellAllowlist};
use harness_core::edit::hashline::{compute_line_hash, LineAnchor};
use harness_core::event::ActorKind;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

mod hashline_apply;
use hashline_apply::HashlineApplyTool;

mod hashline_edit;
use hashline_edit::HashlineEditTool;

mod hashline_scan;
use hashline_scan::HashlineScanTool;

mod fs_glob;
use fs_glob::FsGlobTool;

mod fs_ls;
use fs_ls::FsLsTool;

mod fs_grep;
use fs_grep::FsGrepTool;

mod workspace_edit;
use workspace_edit::{
    record_file_hashline_read, record_file_read, FsWriteTool, WorkspaceEditExecutor,
};

mod network;
use network::NetworkExecutor;

mod mcp;

mod control_plane;
use control_plane::ControlPlaneExecutor;

mod agent_ops;
use agent_ops::AgentOpsExecutor;

mod code_lsp;
use code_lsp::CodeLspExecutor;

mod github;
use github::{GitHubExecutor, GitHubIssueTool, GitHubPullRequestTool};

mod code_lsp_rename;
use code_lsp_rename::{CodeLspRenameExecutor, CodeLspRenameTool};

mod lsp_support;

mod native_tools;
use native_tools::{
    blocked_shell_command_message, BashTool, BatchTool, CodeSearchTool, GlobTool, GrepTool,
    InvalidTool, ListTool, LspTool, QuestionTool, ReadTool, SkillTool, TaskTool, TodoReadTool,
    TodoWriteTool, WebFetchTool, WebSearchTool, WriteTool,
};

pub use harness_core::tool::canonical_tool_id_for;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditingToolSurfaceConfig {
    pub hashline_edit: bool,
}

impl Default for EditingToolSurfaceConfig {
    fn default() -> Self {
        Self {
            hashline_edit: true,
        }
    }
}

pub fn coordinator_registry(shell_allowlist: ShellAllowlist) -> ToolRegistry {
    coordinator_registry_with_mcp_and_editing(
        shell_allowlist,
        McpConfig::default(),
        EditingToolSurfaceConfig::default(),
    )
}

pub fn coordinator_registry_with_mcp(
    shell_allowlist: ShellAllowlist,
    mcp_config: McpConfig,
) -> ToolRegistry {
    coordinator_registry_with_mcp_and_editing(
        shell_allowlist,
        mcp_config,
        EditingToolSurfaceConfig::default(),
    )
}

pub fn coordinator_registry_with_mcp_and_editing(
    shell_allowlist: ShellAllowlist,
    mcp_config: McpConfig,
    editing: EditingToolSurfaceConfig,
) -> ToolRegistry {
    let agent_ops_executor = Arc::new(AgentOpsExecutor::new());
    let control_plane_executor = Arc::new(ControlPlaneExecutor::new());
    let network_executor = Arc::new(NetworkExecutor::new());
    let code_lsp_executor = Arc::new(CodeLspExecutor::new());
    let github_executor = Arc::new(GitHubExecutor::new());
    let code_lsp_rename_executor = Arc::new(CodeLspRenameExecutor::new());
    let workspace_edit_executor = Arc::new(WorkspaceEditExecutor::new());

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool::new(editing.hashline_edit)));
    registry.register(Arc::new(ListTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(TaskTool::new(agent_ops_executor.clone())));
    registry.register(Arc::new(HashlineScanTool));
    registry.register(Arc::new(HashlineApplyTool));
    registry.register(Arc::new(FsWriteTool::new(workspace_edit_executor.clone())));
    registry.register(Arc::new(HashlineEditTool));
    registry.register(Arc::new(WriteTool::new(workspace_edit_executor.clone())));
    registry.register(Arc::new(BashTool::new(shell_allowlist)));
    registry.register(Arc::new(WebFetchTool::new(network_executor.clone())));
    registry.register(Arc::new(WebSearchTool::new(network_executor.clone())));
    registry.register(Arc::new(CodeSearchTool::new(network_executor.clone())));
    registry.register(Arc::new(GitHubIssueTool::new(github_executor.clone())));
    registry.register(Arc::new(GitHubPullRequestTool::new(github_executor)));
    registry.register(Arc::new(TodoWriteTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(TodoReadTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(SkillTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(BatchTool::new(agent_ops_executor.clone())));
    registry.register(Arc::new(QuestionTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(CodeLspRenameTool::new(code_lsp_rename_executor)));
    registry.register(Arc::new(LspTool::new(code_lsp_executor)));
    registry.register(Arc::new(InvalidTool::new(control_plane_executor)));
    mcp::register_mcp_tools(&mut registry, mcp_config);
    registry
}

pub fn worker_registry(shell_allowlist: ShellAllowlist) -> ToolRegistry {
    worker_registry_with_mcp_and_editing(
        shell_allowlist,
        McpConfig::default(),
        EditingToolSurfaceConfig::default(),
    )
}

pub fn worker_registry_with_mcp(
    shell_allowlist: ShellAllowlist,
    mcp_config: McpConfig,
) -> ToolRegistry {
    worker_registry_with_mcp_and_editing(
        shell_allowlist,
        mcp_config,
        EditingToolSurfaceConfig::default(),
    )
}

pub fn worker_registry_with_mcp_and_editing(
    shell_allowlist: ShellAllowlist,
    mcp_config: McpConfig,
    editing: EditingToolSurfaceConfig,
) -> ToolRegistry {
    let coordinator =
        coordinator_registry_with_mcp_and_editing(shell_allowlist, mcp_config, editing);
    let mut worker = ToolRegistry::new();
    for tool in coordinator.filter_for_actor(ActorKind::Worker) {
        worker.register(tool);
    }
    worker
}

pub(crate) struct FsReadTool {
    default_hashline_anchors: bool,
}

impl FsReadTool {
    pub(crate) fn new(default_hashline_anchors: bool) -> Self {
        Self {
            default_hashline_anchors,
        }
    }
}

const FS_READ_DEFAULT_OFFSET: u32 = 1;
const FS_READ_DEFAULT_LIMIT: u32 = 2000;
const TOOL_OUTPUT_INLINE_LINE_LIMIT: usize = 2_000;
const TOOL_OUTPUT_INLINE_BYTE_LIMIT: usize = 51_200;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FsReadArgs {
    path: String,
    #[serde(default = "default_fs_read_offset")]
    offset: u32,
    #[serde(default = "default_fs_read_limit")]
    limit: u32,
    #[serde(default = "default_fs_read_line_numbers")]
    line_numbers: bool,
    #[serde(default)]
    hashline_anchors: Option<bool>,
}

fn default_fs_read_offset() -> u32 {
    FS_READ_DEFAULT_OFFSET
}

fn default_fs_read_limit() -> u32 {
    FS_READ_DEFAULT_LIMIT
}

fn default_fs_read_line_numbers() -> bool {
    true
}

fn fs_read_parameters_json_schema(default_hashline_anchors: bool) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string"
            },
            "offset": {
                "type": "integer",
                "minimum": 1,
                "default": FS_READ_DEFAULT_OFFSET
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "default": FS_READ_DEFAULT_LIMIT
            },
            "line_numbers": {
                "type": "boolean",
                "default": true
            },
            "hashline_anchors": {
                "type": "boolean",
                "default": default_hashline_anchors,
                "description": "When true, render lines as LINE#HASH|text for anchor-driven edit workflows"
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn normalize_fs_read_offset(offset: u32) -> u32 {
    offset.max(FS_READ_DEFAULT_OFFSET)
}

fn normalize_fs_read_limit(limit: u32) -> u32 {
    if limit == 0 {
        FS_READ_DEFAULT_LIMIT
    } else {
        limit
    }
}

fn format_fs_read_lines(lines: &[&str], start_line_index: usize, line_numbers: bool) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if line_numbers {
                format!("{}: {}", start_line_index + index + 1, line)
            } else {
                (*line).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_fs_read_hashline_lines(lines: &[&str], start_line_index: usize) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            format!(
                "{}#{}|{}",
                start_line_index + index + 1,
                compute_line_hash(line),
                line.strip_suffix('\r').unwrap_or(line)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_fs_read_anchors(lines: &[&str], start_line_index: usize) -> Vec<LineAnchor> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| LineAnchor {
            line: (start_line_index + index + 1) as u32,
            hash: compute_line_hash(line),
        })
        .collect()
}

fn build_fs_read_anchor_payload(lines: &[&str], start_line_index: usize) -> serde_json::Value {
    json!(lines
        .iter()
        .enumerate()
        .map(|(index, line)| json!({
            "line": start_line_index + index + 1,
            "hash": compute_line_hash(line),
            "text": line.strip_suffix('\r').unwrap_or(line),
        }))
        .collect::<Vec<_>>())
}

#[derive(Debug, Clone)]
struct OutputPreview {
    text: String,
    total_bytes: usize,
    total_lines: usize,
    inline_bytes: usize,
    inline_lines: usize,
    truncated: bool,
}

fn preview_tool_output(text: &str, max_lines: usize, max_bytes: usize) -> OutputPreview {
    let total_bytes = text.len();
    let total_lines = rendered_line_count(text);

    let mut preview_end = 0usize;

    for (idx, ch) in text.char_indices() {
        let next_end = idx + ch.len_utf8();
        if next_end > max_bytes {
            break;
        }
        preview_end = next_end;
    }

    let byte_limited_end = preview_end;
    let line_limited_end = preview_end_for_line_limit(text, max_lines);
    let preview_end = byte_limited_end.min(line_limited_end);
    let truncated = preview_end < text.len();

    let preview = text[..preview_end].to_string();
    let inline_lines = rendered_line_count(&preview);

    OutputPreview {
        inline_bytes: preview.len(),
        inline_lines,
        text: preview,
        total_bytes,
        total_lines,
        truncated,
    }
}

fn rendered_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.split_inclusive('\n').count()
    }
}

fn preview_end_for_line_limit(text: &str, max_lines: usize) -> usize {
    if text.is_empty() || max_lines == 0 {
        return 0;
    }

    let mut line_end = 0usize;
    for segment in text.split_inclusive('\n').take(max_lines) {
        line_end += segment.len();
    }
    line_end
}

fn join_command_output(stdout: &str, stderr: &str) -> String {
    if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    }
}

fn resolve_bash_executable() -> String {
    for candidate in ["/bin/bash", "/usr/bin/bash"] {
        if is_usable_bash_path(Path::new(candidate)) {
            return candidate.to_string();
        }
    }

    if let Some(shell) = shell_env_bash_candidate(std::env::var("SHELL").ok().as_deref()) {
        return shell;
    }

    "bash".to_string()
}

fn shell_env_bash_candidate(shell: Option<&str>) -> Option<String> {
    let shell = shell?.trim();
    if shell.is_empty() {
        return None;
    }

    let path = Path::new(shell);
    if !path.is_absolute() || !is_usable_bash_path(path) {
        return None;
    }

    Some(shell.to_string())
}

fn is_usable_bash_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("bash"))
        && std::fs::metadata(path)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
}

fn build_shell_run_result(
    ctx: &ToolContext,
    mut structured_json: serde_json::Map<String, serde_json::Value>,
    stdout: String,
    stderr: String,
) -> Result<ToolResult, ToolError> {
    structured_json.insert("stdout_bytes".to_string(), json!(stdout.len()));
    structured_json.insert("stderr_bytes".to_string(), json!(stderr.len()));

    let full_output = join_command_output(&stdout, &stderr);
    let preview = preview_tool_output(
        &full_output,
        TOOL_OUTPUT_INLINE_LINE_LIMIT,
        TOOL_OUTPUT_INLINE_BYTE_LIMIT,
    );

    if !preview.truncated {
        structured_json.insert("stdout".to_string(), json!(stdout));
        structured_json.insert("stderr".to_string(), json!(stderr));
        structured_json.insert("truncated".to_string(), json!(false));
        return Ok(ToolResult {
            display_text: full_output,
            structured_json: Some(serde_json::Value::Object(structured_json)),
            artifacts: Vec::new(),
        });
    }

    let artifact = ctx
        .artifact_store()
        .map_err(|err| ToolError::Execution(format!("failed to access artifact store: {err}")))?
        .write_text(
            &format!("toolcalls/{}/shell.output.txt", ctx.tool_call_id),
            &full_output,
        )
        .map_err(|err| {
            ToolError::Execution(format!("failed to write shell output artifact: {err}"))
        })?;

    structured_json.insert("truncated".to_string(), json!(true));
    structured_json.insert(
        "inline_output_bytes".to_string(),
        json!(preview.inline_bytes),
    );
    structured_json.insert(
        "inline_output_lines".to_string(),
        json!(preview.inline_lines),
    );
    structured_json.insert("total_output_bytes".to_string(), json!(preview.total_bytes));
    structured_json.insert("total_output_lines".to_string(), json!(preview.total_lines));
    structured_json.insert(
        "output_artifact".to_string(),
        json!({
            "path": artifact.path,
            "digest": artifact.digest,
        }),
    );

    let marker = format!(
        "... [truncated: showing {} of {} lines and {} of {} bytes; full output: {}]",
        preview.inline_lines,
        preview.total_lines,
        preview.inline_bytes,
        preview.total_bytes,
        artifact.path
    );
    let mut display_text = preview.text;
    if display_text.is_empty() {
        display_text = marker;
    } else if display_text.ends_with('\n') {
        display_text.push_str(&marker);
    } else {
        display_text.push('\n');
        display_text.push_str(&marker);
    }

    Ok(ToolResult {
        display_text,
        structured_json: Some(serde_json::Value::Object(structured_json)),
        artifacts: vec![artifact],
    })
}

#[async_trait]
impl Tool for FsReadTool {
    fn id(&self) -> &str {
        "fs.read"
    }

    fn description(&self) -> &str {
        "Reads UTF-8 text from a workspace file with optional offset/limit and line numbers."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        fs_read_parameters_json_schema(self.default_hashline_anchors)
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: FsReadArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        let offset = normalize_fs_read_offset(args.offset);
        let limit = normalize_fs_read_limit(args.limit);
        let hashline_anchors = args
            .hashline_anchors
            .unwrap_or(self.default_hashline_anchors);

        let path = Path::new(&args.path);
        let resolved = ctx.resolve_workspace_path(path)?;
        let bytes = std::fs::read(&resolved)
            .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| ToolError::Execution("binary file not supported".to_string()))?;

        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();
        let start_line_index = (offset - 1) as usize;

        let available_lines: &[&str] = if start_line_index < total_lines {
            &lines[start_line_index..]
        } else {
            &[][..]
        };

        let line_limit = limit as usize;
        let shown_count = available_lines.len().min(line_limit);
        let truncated = available_lines.len() > line_limit;
        let shown_lines = &available_lines[..shown_count];
        let read_anchors =
            hashline_anchors.then(|| build_fs_read_anchors(shown_lines, start_line_index));

        let mut display_text = if hashline_anchors {
            format_fs_read_hashline_lines(shown_lines, start_line_index)
        } else {
            format_fs_read_lines(shown_lines, start_line_index, args.line_numbers)
        };
        let mut artifacts = Vec::new();

        if truncated {
            let full_output = if hashline_anchors {
                format_fs_read_hashline_lines(available_lines, start_line_index)
            } else {
                format_fs_read_lines(available_lines, start_line_index, args.line_numbers)
            };
            let artifact = ctx
                .artifact_store()
                .map_err(|err| {
                    ToolError::Execution(format!("failed to access artifact store: {err}"))
                })?
                .write_text(
                    &format!("toolcalls/{}/fs.read.redacted.txt", ctx.tool_call_id),
                    &full_output,
                )
                .map_err(|err| {
                    ToolError::Execution(format!("failed to write fs.read artifact: {err}"))
                })?;

            let marker = format!(
                "... [truncated: showing {} of {} lines from line {}; full output: {}]",
                shown_count,
                available_lines.len(),
                start_line_index + 1,
                artifact.path
            );

            if display_text.is_empty() {
                display_text = marker;
            } else {
                display_text.push('\n');
                display_text.push_str(&marker);
            }

            artifacts.push(artifact);
        }

        if let Some(anchors) = read_anchors {
            record_file_hashline_read(&ctx, &resolved, anchors)?;
        } else {
            record_file_read(&ctx, &resolved)?;
        }

        Ok(ToolResult {
            display_text,
            structured_json: Some(json!({
                "path": args.path,
                "resolved_path": resolved.display().to_string(),
                "offset": offset,
                "limit": limit,
                "total_lines": total_lines,
                "line_numbers": args.line_numbers,
                "hashline_anchors": hashline_anchors,
                "anchors": if hashline_anchors {
                    build_fs_read_anchor_payload(shown_lines, start_line_index)
                } else {
                    serde_json::Value::Null
                },
                "truncated": truncated,
            })),
            artifacts,
        })
    }
}

pub(crate) struct ShellRunTool {
    allowlist: ShellAllowlist,
}

impl ShellRunTool {
    pub(crate) fn new(allowlist: ShellAllowlist) -> Self {
        Self { allowlist }
    }

    fn is_executable_allowed(&self, executable: &str) -> bool {
        self.allowlist
            .executables
            .iter()
            .any(|allowed| allowed == executable)
    }

    fn resolve_cwd(&self, ctx: &ToolContext, cwd: Option<&str>) -> Result<PathBuf, ToolError> {
        let cwd = match cwd {
            Some(cwd) => ctx.resolve_workspace_path(Path::new(cwd))?,
            None => ctx.workspace_root.clone(),
        };

        if self.allowlist.cwd_roots.is_empty() {
            return Ok(cwd);
        }

        let canonical = cwd
            .canonicalize()
            .map_err(|err| ToolError::Execution(format!("failed to resolve cwd: {err}")))?;

        let allowed = self.allowlist.cwd_roots.iter().any(|root| {
            ctx.resolve_workspace_path(Path::new(root))
                .map(|allowed_root| canonical.starts_with(allowed_root))
                .unwrap_or(false)
        });

        if !allowed {
            return Err(ToolError::CommandBlocked(format!(
                "cwd {} is not in allowlist",
                canonical.display()
            )));
        }

        Ok(canonical)
    }
}

#[derive(Deserialize, JsonSchema)]
struct ShellRunArgs {
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    description: Option<String>,
}

#[async_trait]
impl Tool for ShellRunTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn description(&self) -> &str {
        "Runs an allowlisted shell command inside the workspace and returns stdout/stderr. Use bash for real shell work like git, cargo, or npm—not for file discovery, file search, reading, or editing. Commands such as find, grep/rg, cat, head, tail, sed, and awk are blocked; use glob, grep, list, read, edit, or write instead."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        json_schema_for::<ShellRunArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: ShellRunArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        let direct_exec_requested = args.cmd.is_some();
        let shell_command_requested = args.command.is_some();
        if direct_exec_requested == shell_command_requested {
            return Err(ToolError::InvalidArguments(
                "provide exactly one of cmd or command".to_string(),
            ));
        }

        let timeout_ms = args.timeout.unwrap_or(120_000);
        if let Some(cmd) = args.cmd {
            if !self.is_executable_allowed(&cmd) {
                return Err(ToolError::CommandBlocked(blocked_shell_command_message(
                    &cmd,
                )));
            }

            let cwd = self.resolve_cwd(&ctx, args.cwd.as_deref())?;
            let output = tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                tokio::process::Command::new(&cmd)
                    .args(&args.args)
                    .current_dir(cwd)
                    .output(),
            )
            .await
            .map_err(|_| ToolError::Execution(format!("command timed out after {timeout_ms} ms")))?
            .map_err(|err| ToolError::Execution(format!("failed to execute command: {err}")))?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let status = output.status.code().unwrap_or(-1);
            let structured_json = json!({
                "cmd": cmd,
                "args": args.args,
                "cwd": args.cwd,
                "status": status,
                "success": output.status.success(),
            })
            .as_object()
            .cloned()
            .expect("shell json object");

            build_shell_run_result(&ctx, structured_json, stdout, stderr)
        } else {
            let command = args.command.expect("validated shell command presence");
            let workdir = args.workdir.or(args.cwd);
            let cwd = self.resolve_cwd(&ctx, workdir.as_deref())?;
            crate::native_tools::validate_bash_command(
                &command,
                &cwd,
                &ctx.workspace_root,
                &self.allowlist,
            )?;
            let shell = resolve_bash_executable();
            let output = tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                tokio::process::Command::new(&shell)
                    .arg("-lc")
                    .arg(&command)
                    .current_dir(cwd)
                    .output(),
            )
            .await
            .map_err(|_| ToolError::Execution(format!("command timed out after {timeout_ms} ms")))?
            .map_err(|err| ToolError::Execution(format!("failed to execute command: {err}")))?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let status = output.status.code().unwrap_or(-1);
            let structured_json = json!({
                "description": args.description,
                "command": command,
                "workdir": workdir,
                "status": status,
                "success": output.status.success(),
            })
            .as_object()
            .cloned()
            .expect("shell json object");

            build_shell_run_result(&ctx, structured_json, stdout, stderr)
        }
    }
}

fn json_schema_for<T: JsonSchema>() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(T).schema)
        .unwrap_or_else(|_| empty_object_json_schema());
    normalize_top_level_object_schema(&mut schema);
    schema
}

fn empty_object_json_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {},
        "required": [],
        "additionalProperties": false
    })
}

fn normalize_top_level_object_schema(schema: &mut serde_json::Value) {
    if schema.get("type").and_then(serde_json::Value::as_str) != Some("object") {
        return;
    }

    let Some(object) = schema.as_object_mut() else {
        return;
    };

    object
        .entry("properties".to_string())
        .or_insert_with(|| json!({}));
    object
        .entry("additionalProperties".to_string())
        .or_insert_with(|| json!(false));

    if object
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|properties| properties.is_empty())
    {
        object
            .entry("required".to_string())
            .or_insert_with(|| json!([]));
    }
}

#[cfg(test)]
mod tests {
    use super::{coordinator_registry, FsReadTool};
    use harness_core::clock::RealClock;
    use harness_core::config::ShellAllowlist;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolContext, ToolError};
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Arc;

    fn fs_read_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-fs-read-tests".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: workspace_root.join(".artifacts"),
            actor: EventActor::new(ActorKind::Supervisor, None),
            category: Some("deep".to_string()),
            tool_call_id: tool_call_id.to_string(),
            coordinator,
        }
    }

    #[test]
    fn tool_parameter_schemas_are_json_objects() {
        let registry = coordinator_registry(ShellAllowlist::default());

        for tool_id in [
            "bash",
            "batch",
            "codesearch",
            "edit",
            "lsp.rename",
            "github.issue",
            "github.pull_request",
            "glob",
            "grep",
            "invalid",
            "list",
            "lsp",
            "question",
            "read",
            "skill",
            "todoread",
            "todowrite",
            "task",
            "webfetch",
            "websearch",
            "write",
            "edit.hashline_apply",
            "edit.hashline_scan",
        ] {
            let tool = registry
                .get(tool_id)
                .unwrap_or_else(|| panic!("missing tool {tool_id}"));
            let schema = tool.parameters_json_schema();
            assert!(schema.is_object(), "schema for {tool_id} must be an object");
            assert_eq!(
                schema.get("type").and_then(serde_json::Value::as_str),
                Some("object"),
                "schema for {tool_id} must declare top-level type=object"
            );
            assert!(
                schema
                    .get("properties")
                    .and_then(serde_json::Value::as_object)
                    .is_some(),
                "schema for {tool_id} must declare top-level properties"
            );
            for forbidden in ["oneOf", "anyOf", "allOf", "enum", "not"] {
                assert!(
                    schema.get(forbidden).is_none(),
                    "schema for {tool_id} must not use top-level {forbidden}"
                );
            }
        }
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct EmptyObjectArgs {}

    #[test]
    fn json_schema_for_empty_object_includes_explicit_properties() {
        let schema = super::json_schema_for::<EmptyObjectArgs>();
        assert_eq!(schema.get("type"), Some(&json!("object")));
        assert_eq!(schema.get("properties"), Some(&json!({})));
        assert_eq!(schema.get("required"), Some(&json!([])));
        assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
    }

    #[test]
    fn coordinator_registry_function_names_are_unique_and_deterministic() {
        let registry = coordinator_registry(ShellAllowlist::default());
        let mapping_a = registry.function_name_mapping();
        let mapping_b = registry.function_name_mapping();

        assert_eq!(mapping_a, mapping_b);

        let function_names = mapping_a
            .function_to_tool_id()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(function_names.len(), mapping_a.function_to_tool_id().len());

        for (function_name, tool_id) in mapping_a.function_to_tool_id() {
            assert_eq!(
                mapping_a.tool_id_for_function_name(function_name),
                Some(tool_id.as_str())
            );
            assert_eq!(
                mapping_a.function_name_for_tool_id(tool_id),
                Some(function_name.as_str())
            );
        }
    }

    #[test]
    fn read_tool_schema_advertises_one_indexed_positive_paging() {
        let registry = coordinator_registry(ShellAllowlist::default());
        let schema = registry
            .get("read")
            .expect("read tool")
            .parameters_json_schema();
        let properties = schema["properties"].as_object().expect("read properties");

        assert_eq!(properties["offset"]["minimum"], json!(1));
        assert_eq!(properties["limit"]["minimum"], json!(1));
        assert_eq!(properties["hashlineAnchors"]["default"], json!(true));
        assert_eq!(schema["required"], json!(["filePath"]));
    }

    #[test]
    fn preview_tool_output_enforces_line_limit_without_trailing_newline() {
        let output = (1..=2_001)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let preview =
            super::preview_tool_output(&output, super::TOOL_OUTPUT_INLINE_LINE_LIMIT, usize::MAX);

        assert!(preview.truncated);
        assert_eq!(preview.inline_lines, super::TOOL_OUTPUT_INLINE_LINE_LIMIT);
        assert_eq!(preview.total_lines, 2_001);
        assert!(preview.text.trim_end_matches('\n').ends_with("line 2000"));
        assert!(!preview.text.contains("line 2001"));
    }

    #[test]
    fn preview_tool_output_allows_exact_line_limit_without_trailing_newline() {
        let output = (1..=super::TOOL_OUTPUT_INLINE_LINE_LIMIT)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let preview =
            super::preview_tool_output(&output, super::TOOL_OUTPUT_INLINE_LINE_LIMIT, usize::MAX);

        assert!(!preview.truncated);
        assert_eq!(preview.inline_lines, super::TOOL_OUTPUT_INLINE_LINE_LIMIT);
        assert_eq!(preview.total_lines, super::TOOL_OUTPUT_INLINE_LINE_LIMIT);
        assert_eq!(preview.text, output);
    }

    #[test]
    fn shell_env_bash_candidate_rejects_relative_and_missing_paths() {
        assert_eq!(super::shell_env_bash_candidate(Some("bash")), None);
        assert_eq!(
            super::shell_env_bash_candidate(Some("/tmp/definitely-not-a-real-bash")),
            None
        );
    }

    #[test]
    fn shell_env_bash_candidate_accepts_existing_absolute_bash_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bash_path = temp.path().join("bash");
        std::fs::write(&bash_path, "#!/usr/bin/env bash\n").expect("write bash stub");

        let candidate = super::shell_env_bash_candidate(bash_path.to_str());
        assert_eq!(candidate.as_deref(), bash_path.to_str());
    }

    #[tokio::test]
    async fn fs_read_supports_offset_and_limit_with_line_numbers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "line one\nline two\nline three\n").expect("write fixture");

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-offset-limit"),
                json!({
                    "path": "fixture.txt",
                    "offset": 2,
                    "limit": 2
                }),
            )
            .await
            .expect("fs.read should succeed");

        assert_eq!(result.display_text, "2: line two\n3: line three");
        assert!(result.artifacts.is_empty());

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["offset"], json!(2));
        assert_eq!(metadata["limit"], json!(2));
        assert_eq!(metadata["total_lines"], json!(3));
        assert_eq!(metadata["truncated"], json!(false));
    }

    #[tokio::test]
    async fn fs_read_can_render_hashline_anchors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "alpha\nbeta\n").expect("write fixture");

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-hashline-read"),
                json!({
                    "path": "fixture.txt",
                    "hashline_anchors": true,
                }),
            )
            .await
            .expect("fs.read hashline mode should succeed");

        assert_eq!(
            result.display_text,
            format!(
                "1#{}|alpha\n2#{}|beta",
                super::compute_line_hash("alpha"),
                super::compute_line_hash("beta")
            )
        );

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["hashline_anchors"], json!(true));
        assert_eq!(metadata["anchors"][0]["line"], json!(1));
        assert_eq!(
            metadata["anchors"][0]["hash"],
            json!(super::compute_line_hash("alpha"))
        );
        assert_eq!(metadata["anchors"][0]["text"], json!("alpha"));
        assert_eq!(metadata["anchors"][1]["line"], json!(2));
        assert_eq!(
            metadata["anchors"][1]["hash"],
            json!(super::compute_line_hash("beta"))
        );
        assert_eq!(metadata["anchors"][1]["text"], json!("beta"));
    }

    #[tokio::test]
    async fn fs_read_normalizes_zero_offset_and_limit_to_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "line one\nline two\nline three\n").expect("write fixture");

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-zero-paging"),
                json!({
                    "path": "fixture.txt",
                    "offset": 0,
                    "limit": 0
                }),
            )
            .await
            .expect("fs.read should normalize zero paging values");

        assert_eq!(
            result.display_text,
            "1: line one\n2: line two\n3: line three"
        );

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["offset"], json!(1));
        assert_eq!(metadata["limit"], json!(super::FS_READ_DEFAULT_LIMIT));
        assert_eq!(metadata["hashline_anchors"], json!(false));
    }

    #[tokio::test]
    async fn read_tool_normalizes_zero_offset_and_limit_for_model_compatibility() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "line one\nline two\nline three\n").expect("write fixture");

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let result = read
            .call(
                fs_read_context(temp.path(), "toolcall-read-zero-paging"),
                json!({
                    "filePath": "fixture.txt",
                    "offset": 0,
                    "limit": 0
                }),
            )
            .await
            .expect("read should normalize zero paging values");

        assert_eq!(
            result.display_text,
            format!(
                "1#{}|line one\n2#{}|line two\n3#{}|line three",
                super::compute_line_hash("line one"),
                super::compute_line_hash("line two"),
                super::compute_line_hash("line three")
            )
        );

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["offset"], json!(1));
        assert_eq!(metadata["limit"], json!(super::FS_READ_DEFAULT_LIMIT));
        assert_eq!(metadata["hashline_anchors"], json!(true));
    }

    #[tokio::test]
    async fn read_tool_exposes_hashline_anchor_mode_for_model_workflows() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "alpha\nbeta\n").expect("write fixture");

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let result = read
            .call(
                fs_read_context(temp.path(), "toolcall-read-hashline"),
                json!({
                    "filePath": "fixture.txt",
                    "hashlineAnchors": true,
                }),
            )
            .await
            .expect("read hashline mode should succeed");

        assert!(result.display_text.contains("1#"));
        assert!(result.display_text.contains("|alpha"));
        assert!(result.display_text.contains("2#"));
        assert!(result.display_text.contains("|beta"));

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["hashline_anchors"], json!(true));
        assert_eq!(metadata["anchors"][0]["line"], json!(1));
        assert_eq!(metadata["anchors"][1]["line"], json!(2));
    }

    #[tokio::test]
    async fn read_tool_accepts_absolute_workspace_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "alpha\nbeta\n").expect("write fixture");

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let result = read
            .call(
                fs_read_context(temp.path(), "toolcall-read-absolute"),
                json!({
                    "filePath": source,
                }),
            )
            .await
            .expect("absolute workspace path should read successfully");

        assert!(result.display_text.contains("|alpha"));
        assert!(result.display_text.contains("|beta"));
        let metadata = result.structured_json.expect("structured json");
        assert!(metadata["resolved_path"]
            .as_str()
            .expect("resolved path string")
            .ends_with("fixture.txt"));
    }

    #[tokio::test]
    async fn read_tool_rejects_absolute_paths_outside_workspace() {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let source = outside.path().join("escape.txt");
        std::fs::write(&source, "blocked\n").expect("write outside fixture");

        let registry = coordinator_registry(ShellAllowlist::default());
        let read = registry.get("read").expect("read tool");
        let error = read
            .call(
                fs_read_context(workspace.path(), "toolcall-read-absolute-escape"),
                json!({
                    "filePath": source,
                }),
            )
            .await
            .expect_err("absolute path outside workspace should be rejected");

        assert!(matches!(error, ToolError::PathEscapesWorkspace { .. }));
    }

    #[tokio::test]
    async fn fs_read_adds_truncation_marker_and_spills_full_output_artifact() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "alpha\nbeta\ngamma\ndelta\n").expect("write fixture");

        let context = fs_read_context(temp.path(), "toolcall-truncated");
        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                context.clone(),
                json!({
                    "path": "fixture.txt",
                    "limit": 2
                }),
            )
            .await
            .expect("fs.read should succeed");

        assert!(result.display_text.contains("1: alpha\n2: beta"));
        assert!(result.display_text.contains("[truncated:"));
        assert_eq!(result.artifacts.len(), 1);

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["truncated"], json!(true));
        assert_eq!(metadata["total_lines"], json!(4));

        let artifact_path = &result.artifacts[0].path;
        let relative = artifact_path
            .strip_prefix("artifacts/")
            .expect("artifact path prefix");
        let spilled = std::fs::read_to_string(context.artifacts_dir.join(relative))
            .expect("read spilled artifact");
        assert!(spilled.contains("1: alpha"));
        assert!(spilled.contains("4: delta"));
    }

    #[tokio::test]
    async fn fs_read_rejects_binary_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.bin");
        std::fs::write(&source, [0xff_u8, 0xfe, 0x00]).expect("write binary fixture");

        let tool = FsReadTool::new(false);
        let error = tool
            .call(
                fs_read_context(temp.path(), "toolcall-binary"),
                json!({
                    "path": "fixture.bin"
                }),
            )
            .await
            .expect_err("fs.read should fail for binary");

        match error {
            ToolError::Execution(message) => assert_eq!(message, "binary file not supported"),
            other => panic!("unexpected error variant: {other}"),
        }
    }

    #[tokio::test]
    async fn bash_spills_large_output_to_artifact_for_event_stability() {
        let temp = tempfile::tempdir().expect("tempdir");
        let registry = coordinator_registry(ShellAllowlist::default());
        let bash = registry.get("bash").expect("bash tool");

        let context = fs_read_context(temp.path(), "toolcall-bash-truncated");
        let result = bash
            .call(
                context.clone(),
                json!({
                    "command": "printf 'alpha%.0s' {1..11000}",
                    "description": "emit many lines"
                }),
            )
            .await
            .expect("bash should succeed");

        assert!(result.display_text.contains("[truncated:"));
        assert_eq!(result.artifacts.len(), 1);

        let metadata = result.structured_json.expect("structured json");
        assert_eq!(metadata["truncated"], json!(true));
        assert!(metadata.get("stdout").is_none());
        assert!(metadata.get("stderr").is_none());
        assert_eq!(metadata["total_output_bytes"], json!(55_000));

        let artifact_path = result.artifacts[0]
            .path
            .strip_prefix("artifacts/")
            .expect("artifact path prefix");
        let spilled = std::fs::read_to_string(context.artifacts_dir.join(artifact_path))
            .expect("read spilled shell output artifact");
        assert_eq!(spilled.len(), 55_000);
        assert!(spilled.starts_with("alphaalphaalpha"));
    }

    #[test]
    fn bash_description_warns_against_find_style_repo_exploration() {
        let registry = coordinator_registry(ShellAllowlist::default());
        let bash = registry.get("bash").expect("bash tool");
        let description = bash.description();

        assert!(description.contains("find"));
        assert!(description.contains("glob"));
        assert!(description.contains("grep"));
        assert!(description.contains("read"));
    }

    #[tokio::test]
    async fn bash_direct_exec_returns_recovery_hint_for_blocked_find() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bash = super::ShellRunTool::new(ShellAllowlist::default());

        let error = bash
            .call(
                fs_read_context(temp.path(), "toolcall-bash-find-blocked"),
                json!({
                    "cmd": "find",
                    "args": ["docs", "-maxdepth", "1", "-type", "f"],
                }),
            )
            .await
            .expect_err("find should be blocked with recovery guidance");

        match error {
            ToolError::CommandBlocked(message) => {
                assert!(message.contains("find is blocked"));
                assert!(message.contains("glob"));
                assert!(message.contains("git status"));
            }
            other => panic!("unexpected error variant: {other}"),
        }
    }
}
