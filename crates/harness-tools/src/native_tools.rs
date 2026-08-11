// allow: SIZE_OK — native tool surface (registry + schema)
use std::path::Path;
use std::sync::Arc;

use crate::agent_ops::{
    AgentOpsExecutor, AgentSpawnRequest, BackgroundCancelRequest, BackgroundOutputRequest,
};
use crate::code_lsp::{
    code_lsp_parameters_json_schema, parse_code_lsp_request, CodeLspExecutor, CodeLspRequest,
};
use crate::control_plane::{
    question_parameters_json_schema, todo_write_parameters_json_schema, ControlPlaneExecutor,
};
use crate::fs_glob::DEFAULT_GLOB_LIMIT;
use crate::fs_grep::{DEFAULT_GREP_CONTEXT, DEFAULT_GREP_LIMIT};
use crate::network::{CodeSearchRequest, NetworkExecutor, WebFetchRequest, WebSearchRequest};
use crate::read_window::{
    normalize_read_limit, normalize_read_offset, READ_DEFAULT_LIMIT, READ_DEFAULT_OFFSET,
};
use crate::workspace_paths::resolve_existing_path;
use crate::{
    parse_tool_args, text_json_tool_result, FsGlobTool, FsGrepTool, FsReadTool, ShellCommandRunner,
    ShellRunTool,
};
use async_trait::async_trait;
use harness_core::config::ShellAllowlist;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use harness_core::tool_metadata;
use harness_core::ToolResultExt;
use serde_json::{json, Value};

mod args;
#[cfg(test)]
mod tests;
mod tree;

use self::args::{
    bash_parameters_json_schema, glob_parameters_json_schema, grep_parameters_json_schema,
    read_parameters_json_schema, web_search_parameters_json_schema, BackgroundCancelArgs,
    BackgroundOutputArgs, BashArgs, BatchArgs, CodeSearchArgs, GlobArgs, GrepArgs, InvalidArgs,
    ListArgs, QuestionArgs, ReadArgs, SkillArgs, TaskArgs, TodoWriteArgs, WebFetchArgs,
    WebSearchArgs,
};
use self::tree::{build_recursive_tree, resolve_directory_path};

const DEFAULT_LIST_LIMIT: usize = 100;

pub(crate) struct ReadTool {
    default_hashline_anchors: bool,
}
pub(crate) struct ListTool;
pub(crate) struct GlobTool;
pub(crate) struct GrepTool;
pub(crate) struct BashTool {
    allowlist: ShellAllowlist,
    runner: Arc<dyn ShellCommandRunner>,
}
pub(crate) struct WebFetchTool {
    executor: Arc<NetworkExecutor>,
}
pub(crate) struct TodoWriteTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct TodoReadTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct TaskTool {
    executor: Arc<AgentOpsExecutor>,
}
pub(crate) struct BackgroundOutputTool {
    executor: Arc<AgentOpsExecutor>,
}
pub(crate) struct BackgroundCancelTool {
    executor: Arc<AgentOpsExecutor>,
}
pub(crate) struct BatchTool {
    executor: Arc<AgentOpsExecutor>,
}
pub(crate) struct SkillTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct InvalidTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct WebSearchTool {
    executor: Arc<NetworkExecutor>,
}
pub(crate) struct CodeSearchTool {
    executor: Arc<NetworkExecutor>,
}
pub(crate) struct QuestionTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct LspTool {
    executor: Arc<CodeLspExecutor>,
}

impl WebFetchTool {
    pub(crate) fn new(executor: Arc<NetworkExecutor>) -> Self {
        Self { executor }
    }
}

impl ReadTool {
    pub(crate) fn new(default_hashline_anchors: bool) -> Self {
        Self {
            default_hashline_anchors,
        }
    }
}

impl TodoWriteTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl TodoReadTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl TaskTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

impl BackgroundOutputTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

impl BackgroundCancelTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

impl BatchTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

impl SkillTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl InvalidTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl BashTool {
    pub(crate) fn with_runner(
        allowlist: ShellAllowlist,
        runner: Arc<dyn ShellCommandRunner>,
    ) -> Self {
        Self { allowlist, runner }
    }
}

impl WebSearchTool {
    pub(crate) fn new(executor: Arc<NetworkExecutor>) -> Self {
        Self { executor }
    }
}

impl CodeSearchTool {
    pub(crate) fn new(executor: Arc<NetworkExecutor>) -> Self {
        Self { executor }
    }
}

impl QuestionTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl LspTool {
    pub(crate) fn new(executor: Arc<CodeLspExecutor>) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn id(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Reads a file or directory. Provide path, and use offset/limit to continue through large text files. Directories are listed; file output is bounded for model readability."
    }

    fn parameters_json_schema(&self) -> Value {
        read_parameters_json_schema(self.default_hashline_anchors)
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ReadArgs = parse_tool_args(args_json)?;
        let offset = normalize_read_offset(args.offset.unwrap_or(READ_DEFAULT_OFFSET));
        let limit = normalize_read_limit(args.limit.unwrap_or(READ_DEFAULT_LIMIT));
        let hashline_anchors = args
            .hashline_anchors
            .unwrap_or(self.default_hashline_anchors);

        let resolved = resolve_existing_path(&ctx, &args.file_path)?;
        if resolved.is_dir() {
            return build_directory_read_result(
                &args.file_path,
                &resolved,
                offset as usize,
                limit as usize,
            );
        }

        FsReadTool::new(self.default_hashline_anchors)
            .call(
                ctx,
                json!({
                    "path": args.file_path,
                    "offset": offset,
                    "limit": limit,
                    "line_numbers": true,
                    "hashline_anchors": hashline_anchors,
                }),
            )
            .await
    }
}

fn build_directory_read_result(
    request_path: &str,
    resolved: &Path,
    offset: usize,
    limit: usize,
) -> Result<ToolResult, ToolError> {
    let all_entries = read_directory_entries(resolved)?;
    let total_entries = all_entries.len();
    let start = offset.saturating_sub(1);
    let entries = all_entries
        .into_iter()
        .skip(start)
        .take(limit)
        .collect::<Vec<_>>();
    let truncated = start + entries.len() < total_entries;
    let summary = if truncated {
        format!(
            "\n(Showing {} of {total_entries} entries. Use 'offset' parameter to read beyond entry {})",
            entries.len(),
            offset + entries.len()
        )
    } else {
        format!("\n({total_entries} entries)")
    };
    let display_text = format!(
        "<path>{}</path>\n<type>directory</type>\n<entries>\n{}{}\n</entries>",
        resolved.display(),
        entries.join("\n"),
        summary
    );
    let structured = json!({
        "title": request_path,
        "path": request_path,
        "resolved_path": resolved.display().to_string(),
        "entries": entries,
        "total_count": total_entries,
        "returned_count": entries.len(),
        "truncated_count": total_entries.saturating_sub(start + entries.len()),
        "truncated": truncated,
        "metadata": {
            "preview": entries.iter().take(20).cloned().collect::<Vec<_>>().join("\n"),
            "truncated": truncated,
            "loaded": [],
            "display": {
                "type": "directory",
                "path": resolved.display().to_string(),
                "entries": entries,
                "offset": offset,
                "totalEntries": total_entries,
                "truncated": truncated,
            }
        }
    });
    Ok(text_json_tool_result(display_text, structured))
}

fn read_directory_entries(directory: &Path) -> Result<Vec<String>, ToolError> {
    let mut entries = std::fs::read_dir(directory)
        .tool_err("failed to list directory")?
        .map(directory_entry_display_name)
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    Ok(entries)
}

fn directory_entry_display_name(
    entry: std::io::Result<std::fs::DirEntry>,
) -> Result<String, ToolError> {
    let entry = entry.tool_err("failed to read directory entry")?;
    let file_type = entry.file_type().map_err(|err| {
        ToolError::Execution(format!("failed to read directory entry type: {err}"))
    })?;

    let mut name = entry.file_name().to_string_lossy().to_string();
    if file_type.is_dir() {
        name.push('/');
    }
    Ok(name)
}

#[async_trait]
impl Tool for ListTool {
    tool_metadata!(
        "list",
        "Recursively lists directory contents in the canonical tree format.",
        ToolCapability::ReadFs,
        super::json_schema_for::<ListArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ListArgs = parse_tool_args(args_json)?;
        let root = resolve_directory_path(&ctx, args.path.as_deref().unwrap_or("."))?;
        let ignore = args.ignore.unwrap_or_default();
        let tree = build_recursive_tree(&ctx, &root, &ignore, DEFAULT_LIST_LIMIT)?;
        Ok(text_json_tool_result(
            tree.rendered,
            json!({
                "path": root.display().to_string(),
                "count": tree.count,
                "truncated": tree.truncated,
            }),
        ))
    }
}

#[async_trait]
impl Tool for GlobTool {
    tool_metadata!(
        "glob",
        "Matches files by glob pattern, sorted by modification time (newest first).",
        ToolCapability::ReadFs,
        glob_parameters_json_schema()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GlobArgs = parse_tool_args(args_json)?;
        FsGlobTool
            .call(
                ctx,
                json!({
                    "pattern": args.pattern,
                    "path": args.path,
                    "limit": args.limit.unwrap_or(u32::try_from(DEFAULT_GLOB_LIMIT).unwrap_or(u32::MAX)),
                }),
            )
            .await
    }
}

#[async_trait]
impl Tool for GrepTool {
    tool_metadata!(
        "grep",
        "Searches file or directory contents by regex. Use path, include, and limit to narrow results.",
        ToolCapability::ReadFs,
        grep_parameters_json_schema()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GrepArgs = parse_tool_args(args_json)?;
        FsGrepTool
            .call(
                ctx,
                json!({
                    "pattern": args.pattern,
                    "path": args.path,
                    "include": args.include,
                    "literal": args.literal,
                    "limit": args.limit.unwrap_or(u32::try_from(DEFAULT_GREP_LIMIT).unwrap_or(u32::MAX)),
                    "context": DEFAULT_GREP_CONTEXT,
                    "output_mode": args.output_mode,
                    "head_limit": args.head_limit,
                }),
            )
            .await
    }
}

#[async_trait]
impl Tool for BashTool {
    tool_metadata!(
        "bash",
        "Executes a shell command using the canonical bash arguments. Use bash for real shell work like git, cargo, or npm—not for file discovery, content search, reading files, or editing. Native `read`, `glob`, `grep`, `list`, and `edit` tools are preferred for file IO, search, and edits. Shell file/search/edit utilities are controlled by permission patterns and workspace path safety, not a static executable allowlist.",
        ToolCapability::Shell,
        bash_parameters_json_schema()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BashArgs = parse_tool_args(args_json)?;
        ShellRunTool::with_runner(self.allowlist.clone(), Arc::clone(&self.runner))
            .call(
                ctx,
                json!({
                    "command": args.command,
                    "workdir": args.workdir,
                    "timeout": args.timeout,
                    "description": args.description,
                }),
            )
            .await
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    tool_metadata!(
        "webfetch",
        "Fetches web content and returns text, markdown, or html.",
        ToolCapability::Network,
        super::json_schema_for::<WebFetchArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WebFetchArgs = parse_tool_args(args_json)?;
        self.executor
            .web_fetch(
                &ctx,
                WebFetchRequest {
                    url: args.url,
                    format: args.format,
                    timeout_secs: args.timeout,
                },
            )
            .await
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    tool_metadata!(
        "todowrite",
        r#"Create and maintain a structured task list for the current coding session. Tracks progress, organizes multi-step work, and surfaces status to the user.

## When to use
Use proactively when:
- The task requires 3+ distinct steps or actions (not just 3 tool calls for a single conceptual step)
- The work is non-trivial and benefits from planning
- The user provides multiple tasks (numbered or comma-separated) or explicitly asks for a todo list
- New instructions arrive - capture them as todos
- You start a task - mark it `in_progress` (only one at a time) before working
- You finish a task - mark it `completed` and add any follow-ups discovered during the work

## When NOT to use
Skip when:
- The work is a single, straightforward task (or <3 trivial steps)
- The request is purely informational or conversational
- Tracking adds no organizational value

## States
- `pending` - not started
- `in_progress` - actively working (exactly ONE at a time)
- `completed` - finished successfully
- `cancelled` - no longer needed

## Rules
- Update status in real time; don't batch completions
- Mark `completed` only after the required work is actually done, including any required verification. Never based on intent.
- Keep exactly one `in_progress` while work remains
- If blocked or partial, keep it `in_progress` and add a follow-up todo describing the blocker
- Preserve user-provided commands verbatim (flags, args, order)
- Items should be specific and actionable; break large work into smaller steps

## Examples

Use it:
- "Add a dark mode toggle and run the tests" -> multi-step feature + explicit verification
- "Rename getCwd -> getCurrentWorkingDirectory across the repo" -> grep reveals 15 occurrences in 8 files
- "Implement registration, catalog, cart, checkout" -> multiple complex features

Skip it:
- "How do I print Hello World in Python?" -> informational
- "Add a comment to calculateTotal" -> single edit
- "Run npm install and tell me what happened" -> one command

When in doubt, use it."#,
        ToolCapability::ReadFs,
        todo_write_parameters_json_schema()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TodoWriteArgs = parse_tool_args(args_json)?;
        self.executor.write_todos(&ctx, args.todos)
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    tool_metadata!(
        "todoread",
        "Reads the per-run todo list stored by todowrite.",
        ToolCapability::ReadFs,
        json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        })
    );

    async fn call(&self, ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor.read_todos(&ctx)
    }
}

fn generate_task_description(prompt: &str) -> String {
    let words: Vec<&str> = prompt.split_whitespace().take(5).collect();
    let joined = words.join(" ");
    if joined.chars().count() > 40 {
        let truncated: String = joined.chars().take(40).collect();
        format!("{truncated}...")
    } else {
        joined
    }
}

#[async_trait]
impl Tool for TaskTool {
    tool_metadata!(
        "task",
        "Delegates work to a named generic child selected by required `subagent_type`: `explore`, `general`, or `librarian`. For a new child omit task_id/session_id; provide one only when continuing a prior task with the same subagent_type. `run_in_background` is required: false waits and returns the child result synchronously, while true returns task_id/request_id immediately. `load_skills` is required; pass an empty list when no skills are needed. Listed skills are resolved before spawning and injected into the child prompt. `description` is optional; when omitted, a short label is auto-generated from the first few words of the prompt. For non-trivial delegation, make `prompt` a structured body with sections: context, goal, downstream use, request, required tools, must-do, and must-not-do. Use background_output with the returned request_id for interim status checks, and with cancel=true anytime for cancellation. For the final result, wait for the coordinator/system completion notification before retrieving with background_output. `command` is prepended to the child prompt as explicit delegation context.",
        ToolCapability::SpawnAgent,
        super::json_schema_for::<TaskArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TaskArgs = parse_tool_args(args_json)?;
        let description = args
            .description
            .unwrap_or_else(|| generate_task_description(&args.prompt));
        self.executor
            .spawn_agent(
                &ctx,
                AgentSpawnRequest {
                    description,
                    profile_name: args.subagent_type.as_str().to_string(),
                    prompt: args.prompt,
                    task_id: args.task_id,
                    session_id: args.session_id,
                    run_in_background: args.run_in_background,
                    load_skills: args.load_skills,
                    command: args.command,
                },
            )
            .await
    }
}

#[async_trait]
impl Tool for BackgroundOutputTool {
    tool_metadata!(
        "background_output",
        "Provides durable retrieval of the current status or terminal result for a child task scheduled with task(run_in_background=true). Use it for interim status checks before completion, and for final result retrieval only after the coordinator/system completion notification. Prefer request_id from the task result; task_id/session_id resolve to the latest child request for compatibility. Set cancel=true anytime to request coordinator cancellation for a non-terminal child task. For multi-wait, pass request_ids with wait_mode=`any` (first terminal) or `all` (every terminal) and block=true.",
        ToolCapability::SpawnAgent,
        super::json_schema_for::<BackgroundOutputArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BackgroundOutputArgs = parse_tool_args(args_json)?;
        self.executor
            .background_output(
                &ctx,
                BackgroundOutputRequest {
                    task_id: args.task_id,
                    session_id: args.session_id,
                    request_id: args.request_id,
                    request_ids: args.request_ids,
                    wait_mode: args.wait_mode,
                    block: args.block,
                    timeout_ms: args.timeout,
                    cancel: args.cancel,
                    reason: args.reason,
                    full_session: args.full_session,
                    include_thinking: args.include_thinking,
                    message_limit: args.message_limit,
                    since_message_id: args.since_message_id,
                    include_tool_results: args.include_tool_results,
                    thinking_max_chars: args.thinking_max_chars,
                    from_end: args.from_end,
                },
            )
            .await
    }
}

#[async_trait]
impl Tool for BackgroundCancelTool {
    tool_metadata!(
        "background_cancel",
        "Requests coordinator-owned cancellation for a non-terminal background task by request_id, or cancels all non-terminal background tasks for the current session when all=true. This is the explicit cancellation form; background_output(cancel=true) remains available for compatibility.",
        ToolCapability::SpawnAgent,
        super::json_schema_for::<BackgroundCancelArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BackgroundCancelArgs = parse_tool_args(args_json)?;
        if args.all {
            self.executor
                .cancel_all_background_tasks(&ctx, args.reason)
                .await
        } else {
            let request_id = args.request_id.ok_or_else(|| {
                ToolError::InvalidArguments("request_id is required when all is false".to_string())
            })?;
            self.executor
                .background_cancel(
                    &ctx,
                    BackgroundCancelRequest {
                        request_id,
                        reason: args.reason,
                    },
                )
                .await
        }
    }
}

#[async_trait]
impl Tool for BatchTool {
    tool_metadata!(
        "batch",
        "Executes multiple tool calls through the coordinator and waits for all results.",
        ToolCapability::ReadFs,
        super::json_schema_for::<BatchArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BatchArgs = parse_tool_args(args_json)?;
        self.executor.execute_batch(&ctx, args.tool_calls).await
    }
}

#[async_trait]
impl Tool for SkillTool {
    tool_metadata!(
        "skill",
        "Loads user-installed skills from the configured skill directories.",
        ToolCapability::ReadFs,
        super::json_schema_for::<SkillArgs>()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SkillArgs = parse_tool_args(args_json)?;
        self.executor.load_skill(&ctx, &args.name).await
    }
}

#[async_trait]
impl Tool for InvalidTool {
    tool_metadata!(
        "invalid",
        "Builds a deterministic invalid-tool response.",
        ToolCapability::ReadFs,
        super::json_schema_for::<InvalidArgs>()
    );

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: InvalidArgs = parse_tool_args(args_json)?;
        Ok(self.executor.invalid_tool(&args.tool, &args.error))
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    tool_metadata!(
        "websearch",
        "Searches the public web via the configured backend.",
        ToolCapability::Network,
        web_search_parameters_json_schema()
    );

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WebSearchArgs = parse_tool_args(args_json)?;
        self.executor
            .web_search(WebSearchRequest {
                query: args.query,
                num_results: args.num_results,
                livecrawl: args.livecrawl.map(|value| value.as_str().to_string()),
                search_type: args.r#type.map(|value| value.as_str().to_string()),
                context_max_characters: args.context_max_characters,
            })
            .await
    }
}

#[async_trait]
impl Tool for CodeSearchTool {
    tool_metadata!(
        "codesearch",
        "Searches remote/public code context via the configured backend. This is not local workspace symbol search. Use grep, ast_grep_search, or lsp for local workspace code and first-party symbols.",
        ToolCapability::Network,
        super::json_schema_for::<CodeSearchArgs>()
    );

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: CodeSearchArgs = parse_tool_args(args_json)?;
        self.executor
            .code_search(CodeSearchRequest {
                query: args.query,
                tokens_num: args.tokens_num,
            })
            .await
    }
}

#[async_trait]
impl Tool for QuestionTool {
    tool_metadata!(
        "question",
        "Asks structured questions and waits for answers from the coordinator.",
        ToolCapability::ReadFs,
        question_parameters_json_schema()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: QuestionArgs = parse_tool_args(args_json)?;
        self.executor.user_question(&ctx, args.questions).await
    }
}

#[async_trait]
impl Tool for LspTool {
    tool_metadata!(
        "lsp",
        "Performs LSP operations through local language servers.",
        ToolCapability::ReadFs,
        code_lsp_parameters_json_schema()
    );

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let request = parse_code_lsp_request(args_json)?;
        match request {
            CodeLspRequest::InstallDecision {
                server_id,
                decision,
            } => record_lsp_install_decision(&ctx, &server_id, &decision),
            _ => self.executor.execute(&ctx, request).await,
        }
    }
}

fn record_lsp_install_decision(
    ctx: &ToolContext,
    server_id: &str,
    decision: &str,
) -> Result<ToolResult, ToolError> {
    let artifact_path = ctx.artifacts_dir.join("lsp-install-decisions.json");

    let mut decisions: Value = if artifact_path.exists() {
        let content = std::fs::read_to_string(&artifact_path)
            .tool_err("failed to read lsp install decisions artifact")?;
        serde_json::from_str(&content).tool_err("failed to parse lsp install decisions artifact")?
    } else {
        if let Some(parent) = artifact_path.parent() {
            std::fs::create_dir_all(parent).tool_err("failed to create artifacts directory")?;
        }
        json!({})
    };

    if let Some(map) = decisions.as_object_mut() {
        map.insert(server_id.to_string(), json!(decision));
    }

    let serialized = serde_json::to_string_pretty(&decisions)
        .tool_err("failed to serialize lsp install decisions")?;
    std::fs::write(&artifact_path, serialized)
        .tool_err("failed to write lsp install decisions artifact")?;

    let display_text = format!("Recorded decision '{decision}' for LSP server '{server_id}'.");
    let structured_json = json!({
        "operation": "installDecision",
        "serverId": server_id,
        "decision": decision,
        "artifactPath": artifact_path.display().to_string(),
    });

    Ok(text_json_tool_result(display_text, structured_json))
}
