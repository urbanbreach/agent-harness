use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent_ops::{select_profile_name, AgentOpsExecutor, AgentSpawnRequest, BatchCall};
use crate::code_lsp::{lsp_parameters_json_schema, CodeLspExecutor, CodeLspRequest};
use crate::control_plane::{ControlPlaneExecutor, QuestionPrompt, TodoItem};
use crate::network::{
    CodeSearchRequest, NetworkExecutor, WebFetchFormat, WebFetchRequest, WebSearchRequest,
};
use crate::workspace_edit::WorkspaceEditExecutor;
use crate::{FsGlobTool, FsGrepTool, FsLsTool, FsReadTool, ShellRunTool};
use async_trait::async_trait;
use globset::Glob;
use harness_core::config::ShellAllowlist;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use walkdir::{DirEntry, WalkDir};

const DEFAULT_READ_LIMIT: usize = 2_000;
const DEFAULT_GLOB_LIMIT: usize = 100;
const DEFAULT_GREP_LIMIT: usize = 100;
const DEFAULT_LIST_LIMIT: usize = 100;
const SKIPPED_DIR_NAMES: &[&str] = &[".git", "target"];
const SKIPPED_RELATIVE_DIRS: &[&str] = &[".agent-harness/sessions"];

pub(crate) struct ReadCompatTool;
pub(crate) struct ListCompatTool;
pub(crate) struct GlobCompatTool;
pub(crate) struct GrepCompatTool;
pub(crate) struct BashCompatTool {
    allowlist: ShellAllowlist,
}
pub(crate) struct WriteCompatTool {
    executor: Arc<WorkspaceEditExecutor>,
}
pub(crate) struct ApplyPatchCompatTool {
    executor: Arc<WorkspaceEditExecutor>,
}
pub(crate) struct WebFetchCompatTool {
    executor: Arc<NetworkExecutor>,
}
pub(crate) struct TodoWriteCompatTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct TodoReadCompatTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct TaskCompatTool {
    executor: Arc<AgentOpsExecutor>,
}
pub(crate) struct BatchCompatTool {
    executor: Arc<AgentOpsExecutor>,
}
pub(crate) struct SkillCompatTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct InvalidCompatTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct PlanExitCompatTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct WebSearchCompatTool {
    executor: Arc<NetworkExecutor>,
}
pub(crate) struct CodeSearchCompatTool {
    executor: Arc<NetworkExecutor>,
}
pub(crate) struct QuestionCompatTool {
    executor: Arc<ControlPlaneExecutor>,
}
pub(crate) struct LspCompatTool {
    executor: Arc<CodeLspExecutor>,
}

impl WebFetchCompatTool {
    pub(crate) fn new(executor: Arc<NetworkExecutor>) -> Self {
        Self { executor }
    }
}

impl WriteCompatTool {
    pub(crate) fn new(executor: Arc<WorkspaceEditExecutor>) -> Self {
        Self { executor }
    }
}

impl ApplyPatchCompatTool {
    pub(crate) fn new(executor: Arc<WorkspaceEditExecutor>) -> Self {
        Self { executor }
    }
}

impl TodoWriteCompatTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl TodoReadCompatTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl TaskCompatTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

impl BatchCompatTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

impl SkillCompatTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl InvalidCompatTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl PlanExitCompatTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl BashCompatTool {
    pub(crate) fn new(allowlist: ShellAllowlist) -> Self {
        Self { allowlist }
    }
}

impl WebSearchCompatTool {
    pub(crate) fn new(executor: Arc<NetworkExecutor>) -> Self {
        Self { executor }
    }
}

impl CodeSearchCompatTool {
    pub(crate) fn new(executor: Arc<NetworkExecutor>) -> Self {
        Self { executor }
    }
}

impl QuestionCompatTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

impl LspCompatTool {
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
struct WriteArgs {
    content: String,
    #[serde(rename = "filePath", alias = "path")]
    file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ApplyPatchArgs {
    #[serde(rename = "patchText")]
    patch_text: String,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskArgs {
    description: String,
    prompt: String,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    run_in_background: Option<bool>,
    #[serde(default)]
    load_skills: Vec<String>,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    tool_calls: Vec<BatchCall>,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QuestionArgs {
    questions: Vec<QuestionPrompt>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanExitArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LspArgs {
    operation: String,
    #[serde(rename = "filePath")]
    file_path: String,
    line: Option<i32>,
    character: Option<i32>,
}

#[async_trait]
impl Tool for ReadCompatTool {
    fn id(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Reads a file or directory using Opencode-compatible arguments and formatting."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<ReadArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ReadArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        let offset = args.offset.unwrap_or(1);
        let limit = args.limit.unwrap_or(DEFAULT_READ_LIMIT as u32);
        if offset == 0 || limit == 0 {
            return Err(ToolError::InvalidArguments(
                "offset and limit must be >= 1".to_string(),
            ));
        }

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

        FsReadTool
            .call(
                ctx,
                json!({
                    "path": args.file_path,
                    "offset": offset,
                    "limit": limit,
                    "line_numbers": true,
                }),
            )
            .await
    }
}

#[async_trait]
impl Tool for ListCompatTool {
    fn id(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "Recursively lists directory contents in an Opencode-compatible tree format."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<ListArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ListArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        let root = resolve_directory_path(&ctx, args.path.as_deref().unwrap_or("."))?;
        let ignore = args.ignore.unwrap_or_default();
        let tree = build_recursive_tree(&ctx, &root, &ignore, DEFAULT_LIST_LIMIT)?;
        Ok(ToolResult {
            display_text: tree.rendered,
            structured_json: Some(json!({
                "path": root.display().to_string(),
                "count": tree.count,
                "truncated": tree.truncated,
            })),
            artifacts: Vec::new(),
        })
    }
}

#[async_trait]
impl Tool for GlobCompatTool {
    fn id(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Matches files by glob pattern with absolute/relative Opencode-compatible arguments."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<GlobArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GlobArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
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
impl Tool for GrepCompatTool {
    fn id(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Searches file contents by regex using Opencode-compatible arguments."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<GrepArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: GrepArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        FsGrepTool
            .call(
                ctx,
                json!({
                    "pattern": args.pattern,
                    "path": args.path,
                    "include": args.include,
                    "limit": DEFAULT_GREP_LIMIT,
                    "context": 0,
                }),
            )
            .await
    }
}

#[async_trait]
impl Tool for BashCompatTool {
    fn id(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Executes a shell command using Opencode-compatible bash arguments."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<BashArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BashArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        ShellRunTool::new(self.allowlist.clone())
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
impl Tool for WriteCompatTool {
    fn id(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Writes file contents using Opencode-compatible write semantics."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<WriteArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WriteArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor
            .write_file(&ctx, &args.file_path, &args.content)
    }
}

#[async_trait]
impl Tool for ApplyPatchCompatTool {
    fn id(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Applies stripped-down apply_patch diffs compatible with Opencode's patch envelope."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<ApplyPatchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: ApplyPatchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.apply_patch(&ctx, &args.patch_text)
    }
}

#[async_trait]
impl Tool for WebFetchCompatTool {
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
        let args: WebFetchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
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
impl Tool for TodoWriteCompatTool {
    fn id(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        "Stores a per-run todo list compatible with Opencode's todowrite contract."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<TodoWriteArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TodoWriteArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.write_todos(&ctx, args.todos)
    }
}

#[async_trait]
impl Tool for TodoReadCompatTool {
    fn id(&self) -> &str {
        "todoread"
    }

    fn description(&self) -> &str {
        "Reads the per-run todo list stored by todowrite."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": false})
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor.read_todos(&ctx)
    }
}

#[async_trait]
impl Tool for TaskCompatTool {
    fn id(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Delegates work to another configured harness category/profile."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<TaskArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TaskArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        let profile_name =
            select_profile_name(args.category.as_deref(), args.subagent_type.as_deref())?;
        self.executor
            .spawn_agent(
                &ctx,
                AgentSpawnRequest {
                    description: args.description,
                    profile_name,
                    prompt: args.prompt,
                    task_id: args.task_id,
                    session_id: args.session_id,
                    run_in_background: args.run_in_background.unwrap_or(false),
                    load_skills: args.load_skills,
                    command: args.command,
                },
            )
            .await
    }
}

#[async_trait]
impl Tool for BatchCompatTool {
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
        let args: BatchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.execute_batch(&ctx, args.tool_calls).await
    }
}

#[async_trait]
impl Tool for SkillCompatTool {
    fn id(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Loads user-installed skills from common Opencode skill directories."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<SkillArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SkillArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.load_skill(&args.name, args.user_message)
    }
}

#[async_trait]
impl Tool for InvalidCompatTool {
    fn id(&self) -> &str {
        "invalid"
    }

    fn description(&self) -> &str {
        "Fallback invalid-tool response compatible with Opencode's invalid tool."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<InvalidArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: InvalidArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        Ok(self.executor.invalid_tool(&args.tool, &args.error))
    }
}

#[async_trait]
impl Tool for PlanExitCompatTool {
    fn id(&self) -> &str {
        "plan_exit"
    }

    fn description(&self) -> &str {
        "Compatibility alias for plan.exit."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<PlanExitArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor.plan_exit(&ctx).await
    }
}

#[async_trait]
impl Tool for WebSearchCompatTool {
    fn id(&self) -> &str {
        "websearch"
    }

    fn description(&self) -> &str {
        "Compatibility alias for search.web."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<WebSearchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WebSearchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
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
impl Tool for CodeSearchCompatTool {
    fn id(&self) -> &str {
        "codesearch"
    }

    fn description(&self) -> &str {
        "Compatibility alias for search.code."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<CodeSearchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: CodeSearchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor
            .code_search(CodeSearchRequest {
                query: args.query,
                tokens_num: args.tokens_num,
            })
            .await
    }
}

#[async_trait]
impl Tool for QuestionCompatTool {
    fn id(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "Translates compat question calls into the native user.question flow."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<QuestionArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: QuestionArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.user_question(&ctx, args.questions).await
    }
}

#[async_trait]
impl Tool for LspCompatTool {
    fn id(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Performs Opencode-compatible LSP operations through local language servers."
    }

    fn parameters_json_schema(&self) -> Value {
        lsp_parameters_json_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: LspArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor
            .execute(
                &ctx,
                CodeLspRequest {
                    operation: args.operation,
                    file_path: args.file_path,
                    line: args.line,
                    character: args.character,
                },
            )
            .await
    }
}

fn resolve_existing_path(ctx: &ToolContext, input: &str) -> Result<PathBuf, ToolError> {
    let workspace = canonical_workspace_root(ctx)?;
    let candidate = normalize_workspace_target_path(&workspace, Path::new(input))?;
    let canonical = candidate
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve path: {err}")))?;
    ensure_within_workspace_path(&workspace, &canonical)?;
    Ok(canonical)
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

fn canonical_workspace_root(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    ctx.workspace_root
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve workspace root: {err}")))
}

fn ensure_within_workspace_path(workspace: &Path, candidate: &Path) -> Result<(), ToolError> {
    if candidate.starts_with(workspace) {
        Ok(())
    } else {
        Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: candidate.display().to_string(),
        })
    }
}

fn normalize_workspace_target_path(workspace: &Path, input: &Path) -> Result<PathBuf, ToolError> {
    let relative = if input.is_absolute() {
        input
            .strip_prefix(workspace)
            .map_err(|_| ToolError::PathEscapesWorkspace {
                workspace_root: workspace.display().to_string(),
                path: input.display().to_string(),
            })?
    } else {
        input
    };

    let mut normalized = workspace.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => normalized.push(segment),
            std::path::Component::ParentDir => {
                if normalized == workspace {
                    return Err(ToolError::PathEscapesWorkspace {
                        workspace_root: workspace.display().to_string(),
                        path: input.display().to_string(),
                    });
                }
                normalized.pop();
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(ToolError::InvalidArguments(
                    "path must be workspace-relative or inside the workspace".to_string(),
                ));
            }
        }
    }
    Ok(normalized)
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
    let mut dirs = BTreeSet::new();
    let mut files_by_dir = BTreeMap::<String, Vec<String>>::new();
    let mut count = 0usize;
    let mut truncated = false;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(ctx, entry))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let rel = normalize_relative_path(entry.path().strip_prefix(root).unwrap_or(entry.path()));
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
        let parts = if dir == "." {
            Vec::new()
        } else {
            dir.split('/').map(ToOwned::to_owned).collect::<Vec<_>>()
        };
        for idx in 0..=parts.len() {
            let dir_path = if idx == 0 {
                ".".to_string()
            } else {
                parts[..idx].join("/")
            };
            dirs.insert(dir_path);
        }
        files_by_dir.entry(dir.clone()).or_default().push(
            Path::new(&rel)
                .file_name()
                .unwrap_or_else(|| OsStr::new(&rel))
                .to_string_lossy()
                .to_string(),
        );
    }

    fn render_dir(
        dir_path: &str,
        dirs: &BTreeSet<String>,
        files_by_dir: &BTreeMap<String, Vec<String>>,
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
        let children = dirs
            .iter()
            .filter(|child| {
                if child.as_str() == dir_path {
                    return false;
                }
                Path::new(child)
                    .parent()
                    .map(normalize_relative_path)
                    .unwrap_or_else(|| ".".to_string())
                    == dir_path
            })
            .cloned()
            .collect::<Vec<_>>();
        for child in children {
            output.push_str(&render_dir(&child, dirs, files_by_dir, depth + 1));
        }
        if let Some(files) = files_by_dir.get(dir_path) {
            let mut files = files.clone();
            files.sort();
            for file in files {
                output.push_str(&format!("{child_indent}{file}\n"));
            }
        }
        output
    }

    let mut rendered = format!("{}/\n", root.display());
    rendered.push_str(&render_dir(".", &dirs, &files_by_dir, 0));
    Ok(RenderedTree {
        rendered: rendered.trim_end().to_string(),
        count: count.min(limit),
        truncated,
    })
}

fn should_skip_entry(ctx: &ToolContext, entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    let path = entry.path();
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    if entry.file_type().is_dir() && SKIPPED_DIR_NAMES.contains(&name) {
        return true;
    }
    let Ok(relative) = path.strip_prefix(&ctx.workspace_root) else {
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

pub(crate) fn validate_bash_command(
    command: &str,
    cwd: &Path,
    workspace_root: &Path,
    allowlist: &ShellAllowlist,
) -> Result<(), ToolError> {
    reject_unsupported_bash_constructs(command)?;
    let segments = split_shell_segments(command)?;
    if segments.is_empty() {
        return Err(ToolError::InvalidArguments(
            "command must not be empty".to_string(),
        ));
    }

    let mut virtual_cwd = cwd.to_path_buf();
    for segment in segments {
        let Some(executable) = first_shell_command_name(&segment)? else {
            continue;
        };
        if executable == "cd" {
            virtual_cwd =
                resolve_shell_cd_target(&segment, &virtual_cwd, workspace_root, allowlist)?;
            continue;
        }
        if matches!(executable.as_str(), "source" | ".") {
            return Err(ToolError::CommandBlocked(
                "source and . are not allowed in compat bash".to_string(),
            ));
        }
        if is_shell_builtin_allowed(executable.as_str()) {
            continue;
        }
        if !allowlist
            .executables
            .iter()
            .any(|allowed| allowed == &executable)
        {
            return Err(ToolError::CommandBlocked(executable));
        }
        validate_shell_path_arguments(&segment, &virtual_cwd, workspace_root)?;
    }

    Ok(())
}

fn reject_unsupported_bash_constructs(command: &str) -> Result<(), ToolError> {
    if command.contains("$(")
        || command.contains('`')
        || command.contains("<(")
        || command.contains(">(")
    {
        return Err(ToolError::CommandBlocked(
            "command substitution and process substitution are not allowed in compat bash"
                .to_string(),
        ));
    }

    Ok(())
}

fn split_shell_segments(command: &str) -> Result<Vec<String>, ToolError> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => {
                current.push(ch);
                escaped = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
            }
            '&' if !in_single && !in_double && chars.peek() == Some(&'&') => {
                chars.next();
                push_shell_segment(&mut segments, &mut current);
            }
            '|' if !in_single && !in_double && chars.peek() == Some(&'|') => {
                chars.next();
                push_shell_segment(&mut segments, &mut current);
            }
            '|' | ';' | '\n' if !in_single && !in_double => {
                push_shell_segment(&mut segments, &mut current);
            }
            _ => current.push(ch),
        }
    }

    if in_single || in_double || escaped {
        return Err(ToolError::InvalidArguments(
            "failed to parse command string".to_string(),
        ));
    }

    push_shell_segment(&mut segments, &mut current);
    Ok(segments)
}

fn push_shell_segment(segments: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        segments.push(trimmed.to_string());
    }
    current.clear();
}

fn first_shell_command_name(segment: &str) -> Result<Option<String>, ToolError> {
    let tokens = shell_words::split(segment).map_err(|err| {
        ToolError::InvalidArguments(format!("failed to parse command string: {err}"))
    })?;
    for token in tokens {
        if is_shell_env_assignment(&token) {
            continue;
        }
        return Ok(Some(token));
    }
    Ok(None)
}

fn is_shell_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn is_shell_builtin_allowed(executable: &str) -> bool {
    matches!(
        executable,
        "alias"
            | "echo"
            | "export"
            | "false"
            | "printf"
            | "pwd"
            | "source"
            | "."
            | "test"
            | "true"
            | "unset"
            | "["
    )
}

fn resolve_shell_cd_target(
    segment: &str,
    cwd: &Path,
    workspace_root: &Path,
    allowlist: &ShellAllowlist,
) -> Result<PathBuf, ToolError> {
    let tokens = shell_words::split(segment).map_err(|err| {
        ToolError::InvalidArguments(format!("failed to parse command string: {err}"))
    })?;
    let mut args = tokens
        .into_iter()
        .filter(|token| !is_shell_env_assignment(token));
    let _ = args.next();
    let target = args.next().ok_or_else(|| {
        ToolError::CommandBlocked("cd without an explicit target is not allowed".to_string())
    })?;
    if target == "-" || target.starts_with('~') || target.contains('$') {
        return Err(ToolError::CommandBlocked(
            "cd target must be an explicit workspace path".to_string(),
        ));
    }

    let candidate = if Path::new(&target).is_absolute() {
        PathBuf::from(&target)
    } else {
        cwd.join(&target)
    };
    let candidate = normalize_workspace_target_path(workspace_root, &candidate)?;
    if allowlist.cwd_roots.is_empty() {
        return Ok(candidate);
    }

    let canonical_workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let allowed = allowlist.cwd_roots.iter().any(|root| {
        normalize_workspace_target_path(&canonical_workspace, Path::new(root))
            .map(|allowed_root| candidate.starts_with(allowed_root))
            .unwrap_or(false)
    });
    if allowed {
        Ok(candidate)
    } else {
        Err(ToolError::CommandBlocked(format!(
            "cd target {} is not in allowlist",
            candidate.display()
        )))
    }
}

fn validate_shell_path_arguments(
    segment: &str,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<(), ToolError> {
    let tokens = shell_words::split(segment).map_err(|err| {
        ToolError::InvalidArguments(format!("failed to parse command string: {err}"))
    })?;
    let mut seen_command = false;
    for token in tokens {
        if !seen_command {
            if is_shell_env_assignment(&token) {
                continue;
            }
            seen_command = true;
            continue;
        }

        if !looks_like_shell_path_argument(&token) {
            continue;
        }

        let candidate = if Path::new(&token).is_absolute() {
            PathBuf::from(&token)
        } else {
            cwd.join(&token)
        };
        let _ = normalize_workspace_target_path(workspace_root, &candidate)?;
    }

    Ok(())
}

fn looks_like_shell_path_argument(token: &str) -> bool {
    !token.starts_with('-')
        && !token.contains('*')
        && !token.contains('?')
        && !token.contains('[')
        && !token.contains('$')
        && !token.starts_with('~')
        && (token.starts_with('/')
            || token.starts_with("./")
            || token.starts_with("../")
            || token == "."
            || token == "..")
}

#[cfg(test)]
mod tests {
    use super::validate_bash_command;
    use harness_core::config::ShellAllowlist;
    use harness_core::tool::ToolError;

    #[test]
    fn validate_bash_command_rejects_command_substitution() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let allowlist = ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        };
        let err = validate_bash_command("ls $(pwd)", tempdir.path(), tempdir.path(), &allowlist)
            .expect_err("command substitution should be blocked");
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_rejects_external_path_arguments() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let allowlist = ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        };
        let err = validate_bash_command("ls /tmp", tempdir.path(), tempdir.path(), &allowlist)
            .expect_err("external path should be blocked");
        assert!(matches!(err, ToolError::PathEscapesWorkspace { .. }));
    }
}
