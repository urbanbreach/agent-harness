//! Built-in tool registry and tool implementations for filesystem, shell, and
//! hashline edit operations.
//!
//! Runtime policy lives in `harness-core`; this crate should focus on tool
//! argument validation, execution, and stable schema exposure.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::config::{McpConfig, ShellAllowlist};
use harness_core::edit::hashline::{compute_line_hash, LineAnchor};
use harness_core::event::ActorKind;
use harness_core::tool::{
    ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;

mod hashline_apply;
use hashline_apply::HashlineApplyTool;

mod hashline_edit;
use hashline_edit::HashlineEditTool;

mod hashline_scan;
use hashline_scan::HashlineScanTool;

mod fs_walk;

mod http_client;

mod fs_glob;
use fs_glob::FsGlobTool;

mod fs_ls;
use fs_ls::FsLsTool;

mod fs_grep;
use fs_grep::FsGrepTool;

mod workspace_edit;
use workspace_edit::{record_file_hashline_read, record_file_read};

mod workspace_paths;

mod network;
use network::NetworkExecutor;

mod mcp;

mod control_plane;
use control_plane::ControlPlaneExecutor;

mod question_env;

mod env_vars;

mod read_window;
use read_window::{
    normalize_read_limit, normalize_read_offset, READ_DEFAULT_LIMIT, READ_DEFAULT_OFFSET,
};

mod limit_summary;

mod text;
use text::trimmed_non_empty;

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
    blocked_shell_command_message, BackgroundOutputTool, BashTool, BatchTool, CodeSearchTool,
    GlobTool, GrepTool, InvalidTool, ListTool, LspTool, QuestionTool, ReadTool, SkillTool,
    TaskTool, TodoReadTool, TodoWriteTool, WebFetchTool, WebSearchTool,
};

mod plan;
use plan::PlanExitTool;

pub use harness_core::tool::canonical_tool_id_for;

pub(crate) fn parse_tool_args<T: DeserializeOwned>(
    args_json: serde_json::Value,
) -> Result<T, ToolError> {
    serde_json::from_value(args_json).map_err(|err| ToolError::InvalidArguments(err.to_string()))
}

pub(crate) fn text_json_tool_result(
    display_text: impl Into<String>,
    structured_json: serde_json::Value,
) -> ToolResult {
    ToolResult::structured(display_text, structured_json)
}

pub(crate) fn text_json_artifacts_tool_result(
    display_text: impl Into<String>,
    structured_json: serde_json::Value,
    artifacts: Vec<ArtifactRef>,
) -> ToolResult {
    ToolResult::structured_with_artifacts(display_text, structured_json, artifacts)
}

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
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadTool::new(editing.hashline_edit)));
    registry.register(Arc::new(ListTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(TaskTool::new(agent_ops_executor.clone())));
    registry.register(Arc::new(BackgroundOutputTool::new(
        agent_ops_executor.clone(),
    )));
    registry.register(Arc::new(PlanExitTool));
    registry.register(Arc::new(HashlineEditTool));
    registry.register(Arc::new(ShellRunTool::new(shell_allowlist.clone())));
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

pub fn coordinator_registry_with_internal_hashline_tools(
    shell_allowlist: ShellAllowlist,
) -> ToolRegistry {
    let mut registry = coordinator_registry(shell_allowlist);
    register_internal_hashline_tools(&mut registry);
    registry
}

fn register_internal_hashline_tools(registry: &mut ToolRegistry) {
    registry.register(Arc::new(HashlineScanTool));
    registry.register(Arc::new(HashlineApplyTool));
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

const TOOL_OUTPUT_INLINE_LINE_LIMIT: usize = 2_000;
const TOOL_OUTPUT_INLINE_BYTE_LIMIT: usize = 51_200;

#[derive(Debug, Clone, Copy)]
struct FsReadRenderOptions {
    line_numbers: bool,
    hashline_anchors: bool,
}

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

#[derive(Debug)]
struct FsReadRequest {
    path: String,
    offset: u32,
    limit: u32,
    render: FsReadRenderOptions,
}

impl FsReadArgs {
    fn into_request(self, default_hashline_anchors: bool) -> FsReadRequest {
        FsReadRequest {
            path: self.path,
            offset: normalize_read_offset(self.offset),
            limit: normalize_read_limit(self.limit),
            render: FsReadRenderOptions {
                line_numbers: self.line_numbers,
                hashline_anchors: self.hashline_anchors.unwrap_or(default_hashline_anchors),
            },
        }
    }
}

impl FsReadRequest {
    fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    fn start_line_index(&self) -> usize {
        (self.offset - 1) as usize
    }

    fn line_limit(&self) -> usize {
        self.limit as usize
    }
}

fn default_fs_read_offset() -> u32 {
    READ_DEFAULT_OFFSET
}

fn default_fs_read_limit() -> u32 {
    READ_DEFAULT_LIMIT
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
                "default": READ_DEFAULT_OFFSET
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "default": READ_DEFAULT_LIMIT
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

fn format_fs_read_line(line: &str, line_number: usize, line_numbers: bool) -> String {
    if line_numbers {
        format!("{line_number}: {line}")
    } else {
        line.to_string()
    }
}

fn format_fs_read_hashline_line(anchor: &LineAnchor, line: &str) -> String {
    format!(
        "{}#{}|{}",
        anchor.line,
        anchor.hash,
        fs_read_hashline_text(line)
    )
}

fn build_fs_read_line_anchor(line_number: usize, line: &str) -> LineAnchor {
    LineAnchor {
        line: line_number as u32,
        hash: compute_line_hash(line),
    }
}

fn fs_read_hashline_text(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

fn format_fs_read_output_line(
    line: &str,
    line_number: usize,
    render: FsReadRenderOptions,
) -> String {
    if render.hashline_anchors {
        let anchor = build_fs_read_line_anchor(line_number, line);
        format_fs_read_hashline_line(&anchor, line)
    } else {
        format_fs_read_line(line, line_number, render.line_numbers)
    }
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

    let byte_limited_end = preview_end_for_byte_limit(text, max_bytes);
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

fn preview_end_for_byte_limit(text: &str, max_bytes: usize) -> usize {
    let mut preview_end = 0usize;

    for (idx, ch) in text.char_indices() {
        let next_end = idx + ch.len_utf8();
        if next_end > max_bytes {
            break;
        }
        preview_end = next_end;
    }

    preview_end
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

fn append_truncation_marker(display_text: &mut String, marker: &str) {
    if display_text.is_empty() {
        display_text.push_str(marker);
        return;
    }

    if !display_text.ends_with('\n') {
        display_text.push('\n');
    }
    display_text.push_str(marker);
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

#[derive(Debug)]
struct ShellProcessOutput {
    stdout: String,
    stderr: String,
    status: i32,
    success: bool,
}

impl From<std::process::Output> for ShellProcessOutput {
    fn from(output: std::process::Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            status: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        }
    }
}

impl ShellProcessOutput {
    fn combined_output(&self) -> String {
        join_command_output(&self.stdout, &self.stderr)
    }

    fn insert_stream_byte_metadata(
        &self,
        metadata: &mut serde_json::Map<String, serde_json::Value>,
    ) {
        metadata.insert("stdout_bytes".to_string(), json!(self.stdout.len()));
        metadata.insert("stderr_bytes".to_string(), json!(self.stderr.len()));
    }

    fn insert_full_streams(self, metadata: &mut serde_json::Map<String, serde_json::Value>) {
        metadata.insert("stdout".to_string(), json!(self.stdout));
        metadata.insert("stderr".to_string(), json!(self.stderr));
    }
}

async fn run_shell_process(
    mut command: tokio::process::Command,
    timeout_ms: u64,
) -> Result<ShellProcessOutput, ToolError> {
    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), command.output())
        .await
        .map_err(|_| ToolError::Execution(format!("command timed out after {timeout_ms} ms")))?
        .map_err(|err| ToolError::Execution(format!("failed to execute command: {err}")))?;

    Ok(output.into())
}

fn build_shell_run_result(
    ctx: &ToolContext,
    execution: ShellRunExecution,
) -> Result<ToolResult, ToolError> {
    let ShellRunExecution {
        output,
        mut structured_json,
    } = execution;

    output.insert_stream_byte_metadata(&mut structured_json);
    let full_output = output.combined_output();
    let preview = preview_tool_output(
        &full_output,
        TOOL_OUTPUT_INLINE_LINE_LIMIT,
        TOOL_OUTPUT_INLINE_BYTE_LIMIT,
    );

    if !preview.truncated {
        output.insert_full_streams(&mut structured_json);
        structured_json.insert("truncated".to_string(), json!(false));
        return Ok(text_json_tool_result(
            full_output,
            serde_json::Value::Object(structured_json),
        ));
    }

    build_truncated_shell_run_result(ctx, structured_json, &full_output, preview)
}

fn build_truncated_shell_run_result(
    ctx: &ToolContext,
    mut structured_json: serde_json::Map<String, serde_json::Value>,
    full_output: &str,
    preview: OutputPreview,
) -> Result<ToolResult, ToolError> {
    let artifact = ctx
        .artifact_store()
        .map_err(|err| ToolError::Execution(format!("failed to access artifact store: {err}")))?
        .write_text(
            &format!("toolcalls/{}/shell.output.txt", ctx.tool_call_id),
            full_output,
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
    append_truncation_marker(&mut display_text, &marker);

    Ok(text_json_artifacts_tool_result(
        display_text,
        serde_json::Value::Object(structured_json),
        vec![artifact],
    ))
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
        let request =
            parse_tool_args::<FsReadArgs>(args_json)?.into_request(self.default_hashline_anchors);
        let resolved = ctx.resolve_workspace_path(request.path())?;
        let start_line_index = request.start_line_index();
        let line_limit = request.line_limit();
        let render = request.render;
        let read = read_fs_window(&resolved, start_line_index, line_limit, render)?;
        let display = build_fs_read_display(&ctx, &resolved, &read, start_line_index, render)?;
        let structured_json = build_fs_read_structured_json(&request, &resolved, &read);

        record_fs_read_access(&ctx, &resolved, read.anchors)?;

        Ok(text_json_artifacts_tool_result(
            display.text,
            structured_json,
            display.artifacts,
        ))
    }
}

fn record_fs_read_access(
    ctx: &ToolContext,
    resolved: &Path,
    anchors: Option<Vec<LineAnchor>>,
) -> Result<(), ToolError> {
    if let Some(anchors) = anchors {
        record_file_hashline_read(ctx, resolved, anchors)
    } else {
        record_file_read(ctx, resolved)
    }
}

struct FsReadDisplay {
    text: String,
    artifacts: Vec<ArtifactRef>,
}

fn build_fs_read_display(
    ctx: &ToolContext,
    resolved: &Path,
    read: &FsReadWindow,
    start_line_index: usize,
    render: FsReadRenderOptions,
) -> Result<FsReadDisplay, ToolError> {
    let mut text = read.display_text.clone();
    let mut artifacts = Vec::new();

    if read.truncated {
        let artifact = write_fs_read_artifact_streaming(ctx, resolved, start_line_index, render)?;
        append_fs_read_truncation_marker(&mut text, read, start_line_index, &artifact);
        artifacts.push(artifact);
    }

    Ok(FsReadDisplay { text, artifacts })
}

fn append_fs_read_truncation_marker(
    display_text: &mut String,
    read: &FsReadWindow,
    start_line_index: usize,
    artifact: &ArtifactRef,
) {
    let marker = format!(
        "... [truncated: showing {} of {} lines from line {}; full output: {}]",
        read.shown_lines.len(),
        read.available_lines,
        start_line_index + 1,
        artifact.path
    );
    append_truncation_marker(display_text, &marker);
}

fn build_fs_read_structured_json(
    request: &FsReadRequest,
    resolved: &Path,
    read: &FsReadWindow,
) -> serde_json::Value {
    let anchors = read
        .anchors
        .as_ref()
        .map(|anchors| build_fs_read_anchor_payload(anchors, &read.shown_lines))
        .unwrap_or(serde_json::Value::Null);

    json!({
        "path": request.path.as_str(),
        "resolved_path": resolved.display().to_string(),
        "offset": request.offset,
        "limit": request.limit,
        "total_lines": read.total_lines,
        "line_numbers": request.render.line_numbers,
        "hashline_anchors": request.render.hashline_anchors,
        "anchors": anchors,
        "truncated": read.truncated,
    })
}

#[derive(Debug)]
struct FsReadWindow {
    shown_lines: Vec<String>,
    display_text: String,
    anchors: Option<Vec<LineAnchor>>,
    total_lines: usize,
    available_lines: usize,
    truncated: bool,
}

struct FsReadWindowBuilder {
    shown_lines: Vec<String>,
    display_parts: Vec<String>,
    anchors: Option<Vec<LineAnchor>>,
    line_limit: usize,
    render: FsReadRenderOptions,
}

impl FsReadWindowBuilder {
    fn new(line_limit: usize, render: FsReadRenderOptions) -> Self {
        Self {
            shown_lines: Vec::new(),
            display_parts: Vec::new(),
            anchors: render.hashline_anchors.then(Vec::new),
            line_limit,
            render,
        }
    }

    fn push_visible_line(&mut self, line_number: usize, line: String) {
        if self.shown_lines.len() >= self.line_limit {
            return;
        }

        let rendered = self.render_visible_line(line_number, &line);
        self.shown_lines.push(line);
        self.display_parts.push(rendered);
    }

    fn render_visible_line(&mut self, line_number: usize, line: &str) -> String {
        match self.anchors.as_mut() {
            Some(anchors) => {
                let anchor = build_fs_read_line_anchor(line_number, line);
                let rendered = format_fs_read_hashline_line(&anchor, line);
                anchors.push(anchor);
                rendered
            }
            None => format_fs_read_line(line, line_number, self.render.line_numbers),
        }
    }

    fn finish(self, total_lines: usize, available_lines: usize) -> FsReadWindow {
        FsReadWindow {
            shown_lines: self.shown_lines,
            display_text: self.display_parts.join("\n"),
            anchors: self.anchors,
            total_lines,
            available_lines,
            truncated: available_lines > self.line_limit,
        }
    }
}

fn read_fs_window(
    path: &Path,
    start_line_index: usize,
    line_limit: usize,
    render: FsReadRenderOptions,
) -> Result<FsReadWindow, ToolError> {
    let mut available_lines = 0usize;
    let mut window = FsReadWindowBuilder::new(line_limit, render);
    let mut reader = open_fs_read_file(path)?;

    let total_lines = visit_fs_read_lines(&mut reader, start_line_index, |line_number, line| {
        available_lines += 1;
        window.push_visible_line(line_number, line);
        Ok(())
    })?;

    Ok(window.finish(total_lines, available_lines))
}

fn write_fs_read_artifact_streaming(
    ctx: &ToolContext,
    path: &Path,
    start_line_index: usize,
    render: FsReadRenderOptions,
) -> Result<ArtifactRef, ToolError> {
    let target = prepare_fs_read_artifact_target(ctx)?;
    let mut reader = open_fs_read_file(path)?;
    let artifact = std::fs::File::create(&target.file_path)
        .map_err(|err| ToolError::Execution(format!("failed to write fs.read artifact: {err}")))?;
    let mut artifact_writer = FsReadArtifactWriter::new(artifact);

    visit_fs_read_lines(&mut reader, start_line_index, |line_number, line| {
        let rendered = format_fs_read_output_line(&line, line_number, render);
        artifact_writer.write_rendered_line(&rendered)
    })?;

    Ok(ArtifactRef {
        path: target.artifact_path,
        digest: None,
    })
}

struct FsReadArtifactTarget {
    file_path: PathBuf,
    artifact_path: String,
}

fn prepare_fs_read_artifact_target(ctx: &ToolContext) -> Result<FsReadArtifactTarget, ToolError> {
    let relative = format!("toolcalls/{}/fs.read.redacted.txt", ctx.tool_call_id);
    let file_path = ctx.artifacts_dir.join(&relative);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!(
                "failed to create fs.read artifact directory: {err}"
            ))
        })?;
    }

    Ok(FsReadArtifactTarget {
        file_path,
        artifact_path: format!("artifacts/{relative}"),
    })
}

struct FsReadArtifactWriter<W> {
    artifact: W,
    wrote_any: bool,
}

impl<W: Write> FsReadArtifactWriter<W> {
    fn new(artifact: W) -> Self {
        Self {
            artifact,
            wrote_any: false,
        }
    }

    fn write_rendered_line(&mut self, rendered: &str) -> Result<(), ToolError> {
        if self.wrote_any {
            self.write_bytes(b"\n")?;
        }
        self.write_bytes(rendered.as_bytes())?;
        self.wrote_any = true;
        Ok(())
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), ToolError> {
        self.artifact
            .write_all(bytes)
            .map_err(|err| ToolError::Execution(format!("failed to write fs.read artifact: {err}")))
    }
}

fn open_fs_read_file(path: &Path) -> Result<std::io::BufReader<std::fs::File>, ToolError> {
    let file = std::fs::File::open(path)
        .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
    Ok(std::io::BufReader::new(file))
}

fn visit_fs_read_lines(
    reader: &mut impl BufRead,
    start_line_index: usize,
    mut visit: impl FnMut(usize, String) -> Result<(), ToolError>,
) -> Result<usize, ToolError> {
    let mut raw_line = Vec::new();
    let mut total_lines = 0usize;

    loop {
        raw_line.clear();
        let bytes_read = reader
            .read_until(b'\n', &mut raw_line)
            .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
        if bytes_read == 0 {
            break;
        }

        total_lines += 1;
        let line = decode_fs_read_line(&raw_line)?;
        if total_lines > start_line_index {
            visit(total_lines, line)?;
        }
    }

    Ok(total_lines)
}

fn decode_fs_read_line(raw_line: &[u8]) -> Result<String, ToolError> {
    let line = strip_fs_read_line_terminator(raw_line);
    String::from_utf8(line.to_vec())
        .map_err(|_| ToolError::Execution("binary file not supported".to_string()))
}

fn strip_fs_read_line_terminator(raw_line: &[u8]) -> &[u8] {
    let Some(without_lf) = raw_line.strip_suffix(b"\n") else {
        return raw_line;
    };
    without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
}

fn build_fs_read_anchor_payload(anchors: &[LineAnchor], lines: &[String]) -> serde_json::Value {
    debug_assert_eq!(anchors.len(), lines.len());

    json!(anchors
        .iter()
        .zip(lines)
        .map(|(anchor, line)| {
            json!({
                "line": anchor.line,
                "hash": anchor.hash,
                "text": fs_read_hashline_text(line),
            })
        })
        .collect::<Vec<_>>())
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

const DEFAULT_SHELL_TIMEOUT_MS: u64 = 120_000;

struct ShellRunRequest {
    invocation: ShellInvocation,
    timeout_ms: u64,
}

enum ShellInvocation {
    Direct(DirectShellInvocation),
    Wrapper(WrapperShellInvocation),
}

struct DirectShellInvocation {
    cmd: String,
    args: Vec<String>,
    cwd: Option<String>,
}

struct WrapperShellInvocation {
    command: String,
    workdir: Option<String>,
    description: Option<String>,
}

struct ShellRunExecution {
    output: ShellProcessOutput,
    structured_json: serde_json::Map<String, serde_json::Value>,
}

impl DirectShellInvocation {
    fn into_metadata(
        self,
        output: &ShellProcessOutput,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut metadata = serde_json::Map::new();
        metadata.insert("cmd".to_string(), json!(self.cmd));
        metadata.insert("args".to_string(), json!(self.args));
        metadata.insert("cwd".to_string(), json!(self.cwd));
        insert_shell_execution_status(&mut metadata, output);
        metadata
    }
}

impl WrapperShellInvocation {
    fn into_metadata(
        self,
        output: &ShellProcessOutput,
    ) -> serde_json::Map<String, serde_json::Value> {
        let mut metadata = serde_json::Map::new();
        metadata.insert("description".to_string(), json!(self.description));
        metadata.insert("command".to_string(), json!(self.command));
        metadata.insert("workdir".to_string(), json!(self.workdir));
        insert_shell_execution_status(&mut metadata, output);
        metadata
    }
}

impl ShellRunArgs {
    fn into_request(self) -> Result<ShellRunRequest, ToolError> {
        let timeout_ms = self.timeout.unwrap_or(DEFAULT_SHELL_TIMEOUT_MS);
        let invocation = self.into_invocation()?;

        Ok(ShellRunRequest {
            invocation,
            timeout_ms,
        })
    }

    fn into_invocation(self) -> Result<ShellInvocation, ToolError> {
        let cmd = normalized_shell_command(self.cmd);
        let command = normalized_shell_command(self.command);

        if let Some(cmd) = cmd {
            if command.as_ref().is_some_and(|command| command != &cmd) {
                return Err(shell_invocation_selection_error());
            }

            return Ok(ShellInvocation::Direct(DirectShellInvocation {
                cmd,
                args: self.args,
                cwd: self.cwd.or(self.workdir),
            }));
        }

        if let Some(command) = command {
            Ok(ShellInvocation::Wrapper(WrapperShellInvocation {
                command,
                workdir: self.workdir.or(self.cwd),
                description: self.description,
            }))
        } else {
            Err(shell_invocation_selection_error())
        }
    }
}

fn shell_invocation_selection_error() -> ToolError {
    ToolError::InvalidArguments("provide exactly one of cmd or command".to_string())
}

fn normalized_shell_command(command: Option<String>) -> Option<String> {
    command.and_then(|command| trimmed_non_empty(&command).map(str::to_string))
}

fn insert_shell_execution_status(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    output: &ShellProcessOutput,
) {
    metadata.insert("status".to_string(), json!(output.status));
    metadata.insert("success".to_string(), json!(output.success));
}

impl ShellRunTool {
    async fn run_request(
        &self,
        ctx: &ToolContext,
        request: ShellRunRequest,
    ) -> Result<ShellRunExecution, ToolError> {
        match request.invocation {
            ShellInvocation::Direct(invocation) => {
                self.run_direct_invocation(ctx, invocation, request.timeout_ms)
                    .await
            }
            ShellInvocation::Wrapper(invocation) => {
                self.run_wrapper_invocation(ctx, invocation, request.timeout_ms)
                    .await
            }
        }
    }

    async fn run_direct_invocation(
        &self,
        ctx: &ToolContext,
        invocation: DirectShellInvocation,
        timeout_ms: u64,
    ) -> Result<ShellRunExecution, ToolError> {
        if !self.is_executable_allowed(&invocation.cmd) {
            return Err(ToolError::CommandBlocked(blocked_shell_command_message(
                &invocation.cmd,
            )));
        }

        let resolved_cwd = self.resolve_cwd(ctx, invocation.cwd.as_deref())?;
        let mut command = tokio::process::Command::new(&invocation.cmd);
        command.args(&invocation.args).current_dir(resolved_cwd);
        let output = run_shell_process(command, timeout_ms).await?;
        let structured_json = invocation.into_metadata(&output);

        Ok(ShellRunExecution {
            output,
            structured_json,
        })
    }

    async fn run_wrapper_invocation(
        &self,
        ctx: &ToolContext,
        invocation: WrapperShellInvocation,
        timeout_ms: u64,
    ) -> Result<ShellRunExecution, ToolError> {
        let cwd = self.resolve_cwd(ctx, invocation.workdir.as_deref())?;
        crate::native_tools::validate_bash_command(
            &invocation.command,
            &cwd,
            &ctx.workspace_root,
            &self.allowlist,
        )?;
        let shell = resolve_bash_executable();
        let mut shell_command = tokio::process::Command::new(&shell);
        shell_command
            .arg("-lc")
            .arg(&invocation.command)
            .current_dir(cwd);
        let output = run_shell_process(shell_command, timeout_ms).await?;
        let structured_json = invocation.into_metadata(&output);

        Ok(ShellRunExecution {
            output,
            structured_json,
        })
    }
}

#[async_trait]
impl Tool for ShellRunTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn description(&self) -> &str {
        "Runs an allowlisted shell command inside the workspace and returns stdout/stderr. Use bash for real shell work like git, cargo, or npm—not for file discovery, file search, reading, or editing. Commands such as find, grep/rg, cat, head, tail, sed, and awk are blocked; use glob, grep, list, read, or edit instead."
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
        let args: ShellRunArgs = parse_tool_args(args_json)?;
        let request = args.into_request()?;
        let executed = self.run_request(&ctx, request).await?;

        build_shell_run_result(&ctx, executed)
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
    use std::path::{Path, PathBuf};
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
            current_model_ref: None,
            current_model_settings: None,
            coordinator,
        }
    }

    fn read_spilled_artifact(context: &ToolContext, artifact_path: &str) -> String {
        let relative = artifact_path
            .strip_prefix("artifacts/")
            .expect("artifact path prefix");
        std::fs::read_to_string(context.artifacts_dir.join(relative))
            .expect("read spilled artifact")
    }

    fn write_workspace_file(
        workspace_root: &Path,
        relative_path: &str,
        contents: impl AsRef<[u8]>,
    ) -> PathBuf {
        let path = workspace_root.join(relative_path);
        std::fs::write(&path, contents).expect("write workspace fixture");
        path
    }

    #[test]
    fn tool_parameter_schemas_are_json_objects() {
        let registry = coordinator_registry(ShellAllowlist::default());

        for tool_id in [
            "bash",
            "background_output",
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
    fn preview_tool_output_byte_limit_keeps_utf8_boundaries() {
        let preview = super::preview_tool_output("ééx", usize::MAX, 3);

        assert!(preview.truncated);
        assert_eq!(preview.text, "é");
        assert_eq!(preview.inline_bytes, "é".len());
        assert_eq!(preview.total_bytes, "ééx".len());
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
        write_workspace_file(
            temp.path(),
            "fixture.txt",
            "line one\nline two\nline three\n",
        );

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
        write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

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
    async fn fs_read_strips_crlf_line_endings() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.txt", "alpha\r\nbeta\r\n");

        let tool = FsReadTool::new(false);
        let result = tool
            .call(
                fs_read_context(temp.path(), "toolcall-crlf-read"),
                json!({
                    "path": "fixture.txt",
                }),
            )
            .await
            .expect("fs.read should read CRLF text");

        assert_eq!(result.display_text, "1: alpha\n2: beta");
    }

    #[tokio::test]
    async fn fs_read_normalizes_zero_offset_and_limit_to_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(
            temp.path(),
            "fixture.txt",
            "line one\nline two\nline three\n",
        );

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
        assert_eq!(metadata["limit"], json!(super::READ_DEFAULT_LIMIT));
        assert_eq!(metadata["hashline_anchors"], json!(false));
    }

    #[tokio::test]
    async fn read_tool_normalizes_zero_offset_and_limit_for_model_compatibility() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(
            temp.path(),
            "fixture.txt",
            "line one\nline two\nline three\n",
        );

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
        assert_eq!(metadata["limit"], json!(super::READ_DEFAULT_LIMIT));
        assert_eq!(metadata["hashline_anchors"], json!(true));
    }

    #[tokio::test]
    async fn read_tool_exposes_hashline_anchor_mode_for_model_workflows() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

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
        let source = write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

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
        write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\ngamma\ndelta\n");

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

        let spilled = read_spilled_artifact(&context, &result.artifacts[0].path);
        assert!(spilled.contains("1: alpha"));
        assert!(spilled.contains("4: delta"));
    }

    #[tokio::test]
    async fn fs_read_rejects_binary_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_workspace_file(temp.path(), "fixture.bin", [0xff_u8, 0xfe, 0x00]);

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

        let spilled = read_spilled_artifact(&context, &result.artifacts[0].path);
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

    #[tokio::test]
    async fn shell_run_accepts_duplicate_wrapper_command_when_it_matches_cmd() {
        let temp = tempfile::tempdir().expect("tempdir");
        let shell = super::ShellRunTool::new(ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: Vec::new(),
        });

        let result = shell
            .call(
                fs_read_context(temp.path(), "toolcall-shell-run-duplicate"),
                json!({
                    "cmd": "bash",
                    "command": "bash",
                    "args": ["-lc", "printf shell-ok"],
                    "cwd": ".",
                    "workdir": ".",
                }),
            )
            .await
            .expect("matching duplicate cmd/command should run as direct exec");

        assert_eq!(result.display_text, "shell-ok");
        let structured = result.structured_json.expect("structured shell output");
        assert_eq!(structured.get("cmd"), Some(&json!("bash")));
        assert_eq!(structured.get("stdout"), Some(&json!("shell-ok")));
        assert_eq!(structured.get("success"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn shell_run_rejects_conflicting_cmd_and_command() {
        let temp = tempfile::tempdir().expect("tempdir");
        let shell = super::ShellRunTool::new(ShellAllowlist::default());

        let error = shell
            .call(
                fs_read_context(temp.path(), "toolcall-shell-run-conflicting"),
                json!({
                    "cmd": "printf",
                    "command": "echo shell-ok",
                }),
            )
            .await
            .expect_err("conflicting cmd and command should be rejected");

        match error {
            ToolError::InvalidArguments(message) => {
                assert_eq!(message, "provide exactly one of cmd or command");
            }
            other => panic!("unexpected error variant: {other}"),
        }
    }
}
