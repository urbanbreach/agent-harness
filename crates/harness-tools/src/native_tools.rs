use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent_ops::{
    select_agent_selection, AgentOpsExecutor, AgentSpawnRequest, BackgroundCancelRequest,
    BackgroundOutputRequest, BatchCall,
};
use crate::code_lsp::{code_lsp_parameters_json_schema, parse_code_lsp_request, CodeLspExecutor};
use crate::control_plane::{
    question_parameters_json_schema, todo_write_parameters_json_schema, ControlPlaneExecutor,
    QuestionPrompt, TodoItem,
};
use crate::fs_glob::DEFAULT_GLOB_LIMIT;
use crate::fs_grep::{DEFAULT_GREP_CONTEXT, DEFAULT_GREP_LIMIT};
use crate::fs_walk::{normalize_base_relative_path, normalize_relative_path, should_skip_entry};
use crate::network::{
    CodeSearchRequest, NetworkExecutor, WebFetchFormat, WebFetchRequest, WebSearchRequest,
};
use crate::read_window::{
    normalize_read_limit, normalize_read_offset, READ_DEFAULT_LIMIT, READ_DEFAULT_OFFSET,
};
use crate::text::trimmed_non_empty;
use crate::workspace_paths::resolve_existing_path;
use crate::{
    parse_tool_args, text_json_tool_result, FsGlobTool, FsGrepTool, FsLsTool, FsReadTool,
    ShellCommandRunner, ShellRunTool,
};
use async_trait::async_trait;
use globset::Glob;
use harness_core::config::ShellAllowlist;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use walkdir::WalkDir;

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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    #[serde(rename = "filePath", alias = "path")]
    file_path: String,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default, rename = "hashlineAnchors", alias = "hashline_anchors")]
    hashline_anchors: Option<bool>,
}

fn read_parameters_json_schema(default_hashline_anchors: bool) -> Value {
    json!({
        "type": "object",
        "properties": {
            "filePath": {
                "type": "string",
                "description": "The path to the file or directory to read"
            },
            "offset": {
                "type": "integer",
                "minimum": 1,
                "description": "The line number to start reading from (1-indexed)"
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": format!("The maximum number of lines to read (defaults to {READ_DEFAULT_LIMIT})")
            },
            "hashlineAnchors": {
                "type": "boolean",
                "default": default_hashline_anchors,
                "description": "When true, render lines as LINE#HASH|text and return anchor metadata for robust hashline edits"
            }
        },
        "required": ["filePath"],
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    ignore: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include: Option<String>,
    #[serde(default)]
    literal: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BashArgs {
    command: String,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    workdir: Option<String>,
    description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WebFetchArgs {
    url: String,
    #[serde(default = "default_webfetch_format")]
    format: WebFetchFormat,
    #[serde(default)]
    timeout: Option<u64>,
}

fn default_webfetch_format() -> WebFetchFormat {
    WebFetchFormat::Markdown
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TodoWriteArgs {
    todos: Vec<TodoItem>,
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskArgs {
    description: String,
    prompt: String,
    subagent_type: Option<String>,
    category: Option<String>,
    task_id: Option<String>,
    session_id: Option<String>,
    run_in_background: bool,
    load_skills: Vec<String>,
    command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BackgroundOutputArgs {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    block: bool,
    #[serde(default = "default_background_output_timeout_ms", alias = "timeout_ms")]
    timeout: u64,
    #[serde(default)]
    cancel: bool,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BackgroundCancelArgs {
    request_id: String,
    #[serde(default)]
    reason: Option<String>,
}

fn default_background_output_timeout_ms() -> u64 {
    120_000
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskArgsCompat {
    description: String,
    prompt: String,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default, alias = "profileName")]
    profile_name: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default, alias = "background")]
    run_in_background: Option<bool>,
    #[serde(default, alias = "skills")]
    load_skills: Option<Vec<String>>,
    #[serde(default)]
    command: Option<String>,
}

impl<'de> Deserialize<'de> for TaskArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = TaskArgsCompat::deserialize(deserializer)?;
        let run_in_background = compat
            .run_in_background
            .ok_or_else(|| D::Error::custom("missing required field `run_in_background`"))?;
        let load_skills = compat
            .load_skills
            .ok_or_else(|| D::Error::custom("missing required field `load_skills`"))?;

        Ok(Self {
            description: compat.description,
            prompt: compat.prompt,
            subagent_type: compat
                .subagent_type
                .or(compat.agent)
                .or(compat.profile)
                .or(compat.profile_name),
            category: compat.category,
            task_id: compat.task_id,
            session_id: compat.session_id,
            run_in_background,
            load_skills,
            command: compat.command,
        })
    }
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    tool_calls: Vec<BatchCall>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchWrapperCall {
    recipient_name: String,
    #[serde(default)]
    parameters: Option<Value>,
    #[serde(default)]
    arguments: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BatchArgsCompat {
    Wrapped { tool_calls: Vec<BatchCall> },
    WrappedWrapper { tool_calls: Vec<BatchWrapperCall> },
    Wrapper { tool_uses: Vec<BatchWrapperCall> },
    Calls { calls: Vec<BatchCall> },
    List(Vec<BatchCall>),
}

impl<'de> Deserialize<'de> for BatchArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;

        let tool_calls = match BatchArgsCompat::deserialize(deserializer)? {
            BatchArgsCompat::Wrapped { tool_calls } => tool_calls,
            BatchArgsCompat::WrappedWrapper { tool_calls } => tool_calls
                .into_iter()
                .map(BatchCall::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(D::Error::custom)?,
            BatchArgsCompat::Calls { calls } => calls,
            BatchArgsCompat::List(calls) => calls,
            BatchArgsCompat::Wrapper { tool_uses } => tool_uses
                .into_iter()
                .map(BatchCall::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(D::Error::custom)?,
        };

        Ok(Self { tool_calls })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SkillArgs {
    name: String,
    #[serde(default)]
    user_message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InvalidArgs {
    tool: String,
    error: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WebSearchArgs {
    query: String,
    #[serde(default)]
    #[serde(rename = "numResults", alias = "num_results")]
    num_results: Option<u32>,
    #[serde(default)]
    livecrawl: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    #[serde(rename = "contextMaxCharacters", alias = "context_max_characters")]
    context_max_characters: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeSearchArgs {
    query: String,
    #[serde(default)]
    #[serde(rename = "tokensNum", alias = "tokens_num")]
    tokens_num: Option<u32>,
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QuestionArgs {
    questions: Vec<QuestionPrompt>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum QuestionArgsCompat {
    Wrapped { questions: Vec<QuestionPrompt> },
    List(Vec<QuestionPrompt>),
    Single(QuestionPrompt),
}

impl<'de> Deserialize<'de> for QuestionArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match QuestionArgsCompat::deserialize(deserializer)? {
            QuestionArgsCompat::Wrapped { questions } => Self { questions },
            QuestionArgsCompat::List(questions) => Self { questions },
            QuestionArgsCompat::Single(question) => Self {
                questions: vec![question],
            },
        })
    }
}

impl TryFrom<BatchWrapperCall> for BatchCall {
    type Error = String;

    fn try_from(value: BatchWrapperCall) -> Result<Self, Self::Error> {
        let tool = value
            .recipient_name
            .rsplit('.')
            .next()
            .and_then(trimmed_non_empty)
            .ok_or_else(|| "batch wrapper call requires a non-empty recipient_name".to_string())?
            .to_string();
        let parameters = value.parameters.or(value.arguments).unwrap_or(Value::Null);
        Ok(Self { tool, parameters })
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
        "Delegates work to another configured harness profile/category. `run_in_background` is required: run_in_background=false waits and returns the child result synchronously; sync child tasks do not emit background wakeup notifications. run_in_background=true returns task_id/request_id immediately and is required when testing or exercising background scheduling, completion reminders, or background_output retrieval. `load_skills` is required, even when empty; listed skills are resolved before spawning and injected into the child prompt. Use background_output with the returned request_id whenever you need status, result, or cancellation. The coordinator may also send a completion reminder later for background tasks, but do not wait for that reminder when the result is needed. `command` is prepended to the child prompt as explicit delegation context."
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
        self.executor
            .load_skill(&ctx, &args.name, args.user_message)
            .await
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

fn resolve_directory_path(ctx: &ToolContext, input: &str) -> Result<PathBuf, ToolError> {
    let path = resolve_existing_path(ctx, input)?;
    if !path.is_dir() {
        return Err(ToolError::InvalidArguments(format!(
            "path must resolve to a directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

struct RenderedTree {
    rendered: String,
    count: usize,
    truncated: bool,
}

fn build_recursive_tree(
    ctx: &ToolContext,
    root: &Path,
    ignore: &[String],
    limit: usize,
) -> Result<RenderedTree, ToolError> {
    let ignore_matchers = ignore
        .iter()
        .filter_map(|pattern| Glob::new(pattern).ok().map(|glob| glob.compile_matcher()))
        .collect::<Vec<_>>();
    let mut child_dirs_by_dir = BTreeMap::<String, BTreeSet<String>>::new();
    let mut files_by_dir = BTreeMap::<String, BTreeSet<String>>::new();
    let mut count = 0usize;
    let mut truncated = false;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(&ctx.workspace_root, entry))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let rel = normalize_base_relative_path(root, entry.path());
        if ignore_matchers.iter().any(|matcher| matcher.is_match(&rel)) {
            continue;
        }
        count += 1;
        if count > limit {
            truncated = true;
            break;
        }
        let dir = Path::new(&rel)
            .parent()
            .map(normalize_relative_path)
            .unwrap_or_else(|| ".".to_string());
        register_tree_dirs(&mut child_dirs_by_dir, &dir);
        files_by_dir.entry(dir.clone()).or_default().insert(
            Path::new(&rel)
                .file_name()
                .unwrap_or_else(|| OsStr::new(&rel))
                .to_string_lossy()
                .to_string(),
        );
    }

    fn render_dir(
        dir_path: &str,
        child_dirs_by_dir: &BTreeMap<String, BTreeSet<String>>,
        files_by_dir: &BTreeMap<String, BTreeSet<String>>,
        depth: usize,
    ) -> String {
        let indent = "  ".repeat(depth);
        let mut output = String::new();
        if depth > 0 {
            output.push_str(&format!(
                "{}{}/\n",
                indent,
                Path::new(dir_path)
                    .file_name()
                    .unwrap_or_else(|| OsStr::new(dir_path))
                    .to_string_lossy()
            ));
        }
        let child_indent = "  ".repeat(depth + 1);
        if let Some(children) = child_dirs_by_dir.get(dir_path) {
            for child in children {
                output.push_str(&render_dir(
                    child,
                    child_dirs_by_dir,
                    files_by_dir,
                    depth + 1,
                ));
            }
        }
        if let Some(files) = files_by_dir.get(dir_path) {
            for file in files {
                output.push_str(&format!("{child_indent}{file}\n"));
            }
        }
        output
    }

    let mut rendered = format!("{}/\n", root.display());
    rendered.push_str(&render_dir(".", &child_dirs_by_dir, &files_by_dir, 0));
    Ok(RenderedTree {
        rendered: rendered.trim_end().to_string(),
        count: count.min(limit),
        truncated,
    })
}

fn register_tree_dirs(child_dirs_by_dir: &mut BTreeMap<String, BTreeSet<String>>, dir: &str) {
    child_dirs_by_dir.entry(".".to_string()).or_default();
    if dir == "." {
        return;
    }

    let parts = dir.split('/').collect::<Vec<_>>();
    for idx in 0..parts.len() {
        let parent = if idx == 0 {
            ".".to_string()
        } else {
            parts[..idx].join("/")
        };
        let child = parts[..=idx].join("/");
        child_dirs_by_dir
            .entry(parent)
            .or_default()
            .insert(child.clone());
        child_dirs_by_dir.entry(child).or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_recursive_tree, AgentOpsExecutor, BackgroundOutputTool, BatchArgs, QuestionArgs,
        TaskArgs, TaskTool,
    };
    use std::sync::Arc;

    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolContext, ToolRunState};
    use serde_json::json;

    #[tokio::test]
    async fn recursive_tree_renders_direct_children_once_in_sorted_order() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path();
        std::fs::create_dir_all(root.join("src/zeta")).expect("create zeta");
        std::fs::create_dir_all(root.join("src/alpha")).expect("create alpha");
        std::fs::write(root.join("src/zeta/mod.rs"), "").expect("write zeta mod");
        std::fs::write(root.join("src/alpha/lib.rs"), "").expect("write alpha lib");
        std::fs::write(root.join("README.md"), "").expect("write readme");

        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let ctx = ToolContext {
            run_id: "run-tree-tests".to_string(),
            workspace_root: root.to_path_buf(),
            artifacts_dir: root.join("artifacts"),
            actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
            category: Some("quick".to_string()),
            tool_call_id: "tree-test".to_string(),
            current_model_ref: None,
            current_model_settings: None,
            tool_state: ToolRunState::default(),
            coordinator,
        };

        let tree = build_recursive_tree(&ctx, root, &[], 100).expect("render tree");

        assert_eq!(tree.count, 3);
        assert!(!tree.truncated);
        assert_eq!(
            tree.rendered,
            format!(
                "{}/\n  src/\n    alpha/\n      lib.rs\n    zeta/\n      mod.rs\n  README.md",
                root.display()
            )
        );
    }

    #[test]
    fn question_args_accept_top_level_array_and_prompt_aliases() {
        let args: QuestionArgs = serde_json::from_value(json!([
            {
                "prompt": "Choose a mode",
                "title": "Mode",
                "choices": ["fast", "thorough"],
                "allowMultiple": true
            }
        ]))
        .expect("question args should accept top-level arrays");

        assert_eq!(args.questions.len(), 1);
        assert_eq!(args.questions[0].question, "Choose a mode");
        assert_eq!(args.questions[0].header, "Mode");
        assert_eq!(args.questions[0].options[0].label, "fast");
        assert_eq!(args.questions[0].multiple, Some(true));
    }

    #[test]
    fn question_args_accept_allow_freeform_legacy_field() {
        let args: QuestionArgs = serde_json::from_value(json!({
            "questions": [
                {
                    "question": "Choose a mode",
                    "options": ["fast", "thorough"],
                    "allowFreeform": false
                }
            ]
        }))
        .expect("question args should ignore allowFreeform legacy field");

        assert_eq!(args.questions.len(), 1);
        assert_eq!(args.questions[0].question, "Choose a mode");
        assert_eq!(args.questions[0].options[1].label, "thorough");
    }

    #[test]
    fn task_args_accept_agent_alias_fields() {
        let args: TaskArgs = serde_json::from_value(json!({
            "description": "Explore codebase",
            "prompt": "Find auth flow",
            "agent": "explorer",
            "background": true,
            "load_skills": []
        }))
        .expect("task args should accept agent aliases");

        assert_eq!(args.subagent_type.as_deref(), Some("explorer"));
        assert!(args.run_in_background);
    }

    #[test]
    fn task_args_accept_skills_alias_for_load_skills() {
        let args: TaskArgs = serde_json::from_value(json!({
            "description": "Explore codebase",
            "prompt": "Find auth flow",
            "category": "explore",
            "run_in_background": false,
            "skills": ["rust-best-practices"]
        }))
        .expect("task args should accept skills alias");

        assert_eq!(args.load_skills, vec!["rust-best-practices".to_string()]);
    }

    #[test]
    fn task_args_require_explicit_background_and_skills_fields() {
        let missing_background = serde_json::from_value::<TaskArgs>(json!({
            "description": "Explore codebase",
            "prompt": "Find auth flow",
            "category": "explore",
            "load_skills": []
        }))
        .expect_err("task args should require run_in_background");
        assert!(missing_background
            .to_string()
            .contains("missing required field `run_in_background`"));

        let missing_skills = serde_json::from_value::<TaskArgs>(json!({
            "description": "Explore codebase",
            "prompt": "Find auth flow",
            "category": "explore",
            "run_in_background": true
        }))
        .expect_err("task args should require load_skills");
        assert!(missing_skills
            .to_string()
            .contains("missing required field `load_skills`"));
    }

    #[test]
    fn task_and_background_output_descriptions_prefer_explicit_retrieval() {
        let executor = Arc::new(AgentOpsExecutor::new());
        let task = TaskTool::new(executor.clone());
        let background_output = BackgroundOutputTool::new(executor);

        let task_description = task.description();
        assert!(task_description.contains("`run_in_background` is required"));
        assert!(task_description.contains("`load_skills` is required"));
        assert!(task_description.contains("run_in_background=true"));
        assert!(task_description.contains("returns task_id/request_id immediately"));
        assert!(task_description
            .contains("sync child tasks do not emit background wakeup notifications"));
        assert!(task_description.contains("testing or exercising background scheduling"));
        assert!(task_description.contains("injected into the child prompt"));
        assert!(task_description.contains("Use background_output"));
        assert!(task_description.contains("completion reminder later for background tasks"));

        let background_output_description = background_output.description();
        assert!(background_output_description.contains("Use this tool when you need"));
        assert!(background_output_description.contains("do not replace explicit retrieval"));
        assert!(background_output_description.contains("cancel=true"));
    }

    #[test]
    fn batch_args_accept_parallel_wrapper_shape() {
        let args: BatchArgs = serde_json::from_value(json!({
            "tool_uses": [
                {
                    "recipient_name": "functions.read",
                    "parameters": {"filePath": "/tmp/demo.txt"}
                },
                {
                    "recipient_name": "functions.bash",
                    "arguments": {"command": "ls"}
                }
            ]
        }))
        .expect("batch args should accept wrapper shape");

        assert_eq!(args.tool_calls.len(), 2);
        assert_eq!(args.tool_calls[0].tool, "read");
        assert_eq!(args.tool_calls[1].tool, "bash");
        assert_eq!(args.tool_calls[1].parameters, json!({"command": "ls"}));
    }

    #[test]
    fn batch_args_accept_wrapper_shape_inside_tool_calls() {
        let args: BatchArgs = serde_json::from_value(json!({
            "tool_calls": [
                {
                    "recipient_name": "functions.read",
                    "parameters": {"filePath": "Cargo.toml"}
                }
            ]
        }))
        .expect("batch args should accept wrapper calls inside tool_calls");

        assert_eq!(args.tool_calls.len(), 1);
        assert_eq!(args.tool_calls[0].tool, "read");
        assert_eq!(
            args.tool_calls[0].parameters,
            json!({"filePath": "Cargo.toml"})
        );
    }

    #[test]
    fn batch_args_accept_args_alias_inside_tool_calls() {
        let args: BatchArgs = serde_json::from_value(json!({
            "tool_calls": [
                {
                    "tool": "read",
                    "args": {"filePath": "Cargo.toml"}
                }
            ]
        }))
        .expect("batch args should accept args alias");

        assert_eq!(args.tool_calls.len(), 1);
        assert_eq!(args.tool_calls[0].tool, "read");
        assert_eq!(
            args.tool_calls[0].parameters,
            json!({"filePath": "Cargo.toml"})
        );
    }
}
