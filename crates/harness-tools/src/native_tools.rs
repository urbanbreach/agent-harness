use std::sync::Arc;

use crate::agent_ops::{
    select_agent_selection, AgentOpsExecutor, AgentSpawnRequest, BackgroundCancelRequest,
    BackgroundOutputRequest,
};
use crate::code_lsp::{code_lsp_parameters_json_schema, parse_code_lsp_request, CodeLspExecutor};
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
    parse_tool_args, text_json_tool_result, FsGlobTool, FsGrepTool, FsLsTool, FsReadTool,
    ShellCommandRunner, ShellRunTool,
};
use async_trait::async_trait;
use harness_core::config::ShellAllowlist;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde_json::{json, Value};

mod args;
#[cfg(test)]
mod tests;
mod tree;

use self::args::{
    read_parameters_json_schema, BackgroundCancelArgs, BackgroundOutputArgs, BashArgs, BatchArgs,
    CodeSearchArgs, GlobArgs, GrepArgs, InvalidArgs, ListArgs, QuestionArgs, ReadArgs, SkillArgs,
    TaskArgs, TodoWriteArgs, WebFetchArgs, WebSearchArgs,
};
use self::tree::{build_recursive_tree, resolve_directory_path};

const DEFAULT_LIST_LIMIT: usize = 100;
const TODO_WRITE_DESCRIPTION: &str = r#"Use this tool to create and manage a structured task list for your current coding session. This helps track progress on complex work and makes progress visible to the user.

Use todowrite proactively for complex multi-step tasks, non-trivial work that requires planning, explicit user requests for a todo item/list/checklist/test todo, user requests with multiple tasks, and whenever new instructions change the current plan. If the user explicitly asks you to make/add/create/update a todo, call this tool even when the request is otherwise trivial; never reply only "Done". Skip it only for single straightforward tasks, trivial work that does not benefit from tracking, and purely informational answers that did not ask for todos.

Task states are: pending, in_progress, completed, cancelled. Priority values are: high, medium, low. Keep at most one item in_progress, mark tasks completed immediately after finishing them, and update the list in real time as work progresses."#;

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
        if self.default_hashline_anchors {
            "Reads a file or directory using the canonical harness tool contract. Hashline anchor mode is on by default, so each line is returned as LINE#HASH|text for robust edits; set hashlineAnchors=false if you need plain numbered output."
        } else {
            "Reads a file or directory using the canonical harness tool contract. For robust edits, set hashlineAnchors=true to get LINE#HASH|text anchors you can feed into the hashline edit workflow."
        }
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
            return FsLsTool
                .call(
                    ctx,
                    json!({
                        "path": args.file_path,
                        "limit": limit,
                    }),
                )
                .await;
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

#[async_trait]
impl Tool for ListTool {
    fn id(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "Recursively lists directory contents in the canonical tree format."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<ListArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

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
    fn id(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Matches files by glob pattern with canonical harness arguments."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<GlobArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GlobArgs = parse_tool_args(args_json)?;
        FsGlobTool
            .call(
                ctx,
                json!({
                    "pattern": args.pattern,
                    "path": args.path,
                    "limit": DEFAULT_GLOB_LIMIT,
                }),
            )
            .await
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn id(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Searches file or directory contents by regex using canonical harness arguments. Set `literal: true` to search for the pattern as plain text."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<GrepArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

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
                    "limit": DEFAULT_GREP_LIMIT,
                    "context": DEFAULT_GREP_CONTEXT,
                }),
            )
            .await
    }
}

#[async_trait]
impl Tool for BashTool {
    fn id(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Executes a shell command using the canonical bash arguments. Use bash for real shell work like git, cargo, or npm-not for file discovery, content search, reading files, or editing. Commands such as find, grep/rg, cat, head, tail, sed, and awk are blocked; use glob, grep, list, read, or edit instead."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<BashArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BashArgs = parse_tool_args(args_json)?;
        ShellRunTool::with_runner(self.allowlist.clone(), self.runner.clone())
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
    fn id(&self) -> &str {
        "webfetch"
    }

    fn description(&self) -> &str {
        "Fetches web content and returns text, markdown, or html."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<WebFetchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }

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
    fn id(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        TODO_WRITE_DESCRIPTION
    }

    fn parameters_json_schema(&self) -> Value {
        todo_write_parameters_json_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TodoWriteArgs = parse_tool_args(args_json)?;
        self.executor.write_todos(&ctx, args.todos)
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    fn id(&self) -> &str {
        "todoread"
    }

    fn description(&self) -> &str {
        "Reads the per-run todo list stored by todowrite."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        })
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor.read_todos(&ctx)
    }
}

#[async_trait]
impl Tool for TaskTool {
    fn id(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Delegates work to another configured harness profile/category. `run_in_background` is required: run_in_background=false waits and returns the child result synchronously; sync child tasks do not emit background wakeup notifications. run_in_background=true returns task_id/request_id immediately and is required when testing or exercising background scheduling, completion reminders, or background_output retrieval. `load_skills` is required, even when empty; listed skills are resolved before spawning and injected into the child prompt. For non-trivial delegation, make `prompt` a structured body with sections: context, goal, downstream use, request, required tools, must-do, must-not-do. Use background_output with the returned request_id whenever you need status, result, or cancellation. The coordinator may also send a completion reminder later for background tasks, but do not wait for that reminder when the result is needed. `command` is prepended to the child prompt as explicit delegation context."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<TaskArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TaskArgs = parse_tool_args(args_json)?;
        let selection =
            select_agent_selection(args.category.as_deref(), args.subagent_type.as_deref())?;
        self.executor
            .spawn_agent(
                &ctx,
                AgentSpawnRequest {
                    description: args.description,
                    profile_name: selection.profile_name,
                    category_selector: selection.category_selector,
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
    fn id(&self) -> &str {
        "background_output"
    }

    fn description(&self) -> &str {
        "Provides durable retrieval of the current status or terminal result for a child task scheduled with task(run_in_background=true). Use this tool when you need the background result; completion reminders are notifications only and do not replace explicit retrieval. Prefer request_id from the task result; task_id/session_id resolve to the latest child request for compatibility. Set cancel=true to request coordinator cancellation for a non-terminal child task."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<BackgroundOutputArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BackgroundOutputArgs = parse_tool_args(args_json)?;
        self.executor
            .background_output(
                &ctx,
                BackgroundOutputRequest {
                    task_id: args.task_id,
                    session_id: args.session_id,
                    request_id: args.request_id,
                    block: args.block,
                    timeout_ms: args.timeout,
                    cancel: args.cancel,
                    reason: args.reason,
                },
            )
            .await
    }
}

#[async_trait]
impl Tool for BackgroundCancelTool {
    fn id(&self) -> &str {
        "background_cancel"
    }

    fn description(&self) -> &str {
        "Requests coordinator-owned cancellation for a non-terminal background task by request_id. This is the explicit cancellation form; background_output(cancel=true) remains available for compatibility."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<BackgroundCancelArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BackgroundCancelArgs = parse_tool_args(args_json)?;
        self.executor
            .background_cancel(
                &ctx,
                BackgroundCancelRequest {
                    request_id: args.request_id,
                    reason: args.reason,
                },
            )
            .await
    }
}

#[async_trait]
impl Tool for BatchTool {
    fn id(&self) -> &str {
        "batch"
    }

    fn description(&self) -> &str {
        "Executes multiple tool calls through the coordinator and waits for all results."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<BatchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BatchArgs = parse_tool_args(args_json)?;
        self.executor.execute_batch(&ctx, args.tool_calls).await
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn id(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Loads user-installed skills from the configured skill directories."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<SkillArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SkillArgs = parse_tool_args(args_json)?;
        self.executor.load_skill(&ctx, &args.name).await
    }
}

#[async_trait]
impl Tool for InvalidTool {
    fn id(&self) -> &str {
        "invalid"
    }

    fn description(&self) -> &str {
        "Builds a deterministic invalid-tool response."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<InvalidArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: InvalidArgs = parse_tool_args(args_json)?;
        Ok(self.executor.invalid_tool(&args.tool, &args.error))
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn id(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        "Searches the public web via the configured backend."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<WebSearchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WebSearchArgs = parse_tool_args(args_json)?;
        self.executor
            .web_search(WebSearchRequest {
                query: args.query,
                num_results: args.num_results,
                livecrawl: args.livecrawl,
                search_type: args.r#type,
                context_max_characters: args.context_max_characters,
            })
            .await
    }
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn id(&self) -> &str {
        "codesearch"
    }

    fn description(&self) -> &str {
        "Searches code context via the configured backend."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<CodeSearchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }

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
    fn id(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "Asks structured questions and waits for answers from the coordinator."
    }

    fn parameters_json_schema(&self) -> Value {
        question_parameters_json_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: QuestionArgs = parse_tool_args(args_json)?;
        self.executor.user_question(&ctx, args.questions).await
    }
}

#[async_trait]
impl Tool for LspTool {
    fn id(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Performs LSP operations through local language servers."
    }

    fn parameters_json_schema(&self) -> Value {
        code_lsp_parameters_json_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor
            .execute(&ctx, parse_code_lsp_request(args_json)?)
            .await
    }
}
