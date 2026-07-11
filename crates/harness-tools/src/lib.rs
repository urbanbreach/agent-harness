// allow: SIZE_OK — tools library entry (coordinator registry + tool registration + native tool surface)
//! Built-in tool registry and tool implementations for filesystem, shell, and
//! hashline edit operations.
//!
//! Runtime policy lives in `harness-core`; this crate should focus on tool
//! argument validation, execution, and stable schema exposure.

use harness_core::config::{McpConfig, ShellAllowlist};
use harness_core::event::ActorKind;
use harness_core::tool::{ArtifactRef, Tool, ToolRegistry, ToolResult};
use schemars::JsonSchema;
use serde_json::json;
use std::sync::Arc;

mod arg_parse;
pub(crate) use arg_parse::parse_tool_args;

mod hashline_apply;
use hashline_apply::HashlineApplyTool;

mod apply_patch_tool;
use apply_patch_tool::ApplyPatchTool;

mod exact_edit;
mod exact_edit_match;

mod file_write;
use file_write::WriteTool;

mod hashline_edit;
use hashline_edit::HashlineEditTool;

mod hashline_scan;
use hashline_scan::HashlineScanTool;

mod fs_walk;

mod http_client;

mod fs_glob;
use fs_glob::FsGlobTool;

mod fs_grep;
use fs_grep::FsGrepTool;

mod workspace_edit;
mod workspace_paths;

mod network;
use network::NetworkExecutor;
pub use network::{
    RemoteSearchHttpRequest, RemoteSearchHttpResponse, RemoteSearchHttpTransport,
    RemoteSearchTestConfig, WebFetchHttpRequest, WebFetchHttpResponse, WebFetchHttpTransport,
};

mod mcp;
mod mcp_render;
mod mcp_session;

mod control_plane;
use control_plane::ControlPlaneExecutor;

mod skill_catalog;
pub use skill_catalog::{
    discover_skill_catalog, discover_skill_catalog_with_config, SkillCatalog, SkillCatalogEntry,
    SkillCatalogStatus,
};

mod question_env;
use question_env::coordinator_question_answer_source;

mod env_vars;

mod read_window;

mod fs_read;
pub(crate) use fs_read::FsReadTool;

mod limit_summary;

mod text;

mod shell_run;
pub(crate) use shell_run::ShellRunTool;
pub use shell_run::{ShellCommandInvocation, ShellCommandRunner, ShellProcessOutput};

mod shell_safety;

mod agent_ops;
use agent_ops::AgentOpsExecutor;

mod code_lsp;
use code_lsp::CodeLspExecutor;

mod github;
use github::{GitHubExecutor, GitHubIssueTool, GitHubPullRequestTool};
pub use github::{GitHubHttpRequest, GitHubHttpResponse, GitHubHttpTransport};

mod code_lsp_rename;
use code_lsp_rename::{CodeLspRenameExecutor, CodeLspRenameTool};

mod lsp_support;

mod native_tools;
use native_tools::{
    BackgroundCancelTool, BackgroundOutputTool, BashTool, BatchTool, CodeSearchTool, GlobTool,
    GrepTool, InvalidTool, ListTool, LspTool, QuestionTool, ReadTool, SkillTool, TaskTool,
    TodoReadTool, TodoWriteTool, WebFetchTool, WebSearchTool,
};

mod session_tools;
use session_tools::{SessionInfoTool, SessionListTool, SessionReadTool, SessionSearchTool};

mod ast_grep;
use ast_grep::{AstGrepReplaceTool, AstGrepSearchTool};

pub mod tool_catalog;
pub use tool_catalog::{native_tool_catalog_entries, NativeToolCatalogEntry};

mod plan;
use plan::{PlanEnterTool, PlanExitTool};

pub use harness_core::UnwrapOrAbort;

pub use harness_core::tool::canonical_tool_id_for;

#[doc(hidden)]
pub struct CoordinatorRegistryExecutors {
    network_executor: Arc<NetworkExecutor>,
    github_executor: Arc<GitHubExecutor>,
    question_answer_source: Arc<dyn question_env::QuestionAnswerSource>,
    shell_command_runner: Arc<dyn ShellCommandRunner>,
    ast_grep_command: Option<String>,
}

impl Default for CoordinatorRegistryExecutors {
    fn default() -> Self {
        Self {
            network_executor: Arc::new(NetworkExecutor::new()),
            github_executor: Arc::new(GitHubExecutor::new()),
            question_answer_source: coordinator_question_answer_source(),
            shell_command_runner: ShellRunTool::default_runner(),
            ast_grep_command: None,
        }
    }
}

impl CoordinatorRegistryExecutors {
    #[doc(hidden)]
    pub fn with_web_fetch_transport(transport: Arc<dyn WebFetchHttpTransport>) -> Self {
        Self {
            network_executor: Arc::new(NetworkExecutor::with_web_fetch_transport(transport)),
            ..Self::default()
        }
    }

    #[doc(hidden)]
    pub fn with_remote_search_transport(
        config: RemoteSearchTestConfig,
        transport: Arc<dyn RemoteSearchHttpTransport>,
    ) -> Result<Self, String> {
        Ok(Self {
            network_executor: Arc::new(NetworkExecutor::with_remote_search_transport(
                config, transport,
            )?),
            ..Self::default()
        })
    }

    #[doc(hidden)]
    pub fn with_github_transport(
        api_base_url: impl Into<String>,
        auth_token: Option<String>,
        transport: Arc<dyn GitHubHttpTransport>,
    ) -> Self {
        Self {
            github_executor: Arc::new(GitHubExecutor::with_transport(
                api_base_url,
                auth_token,
                transport,
            )),
            ..Self::default()
        }
    }

    #[doc(hidden)]
    pub(crate) fn with_question_answer_source(
        question_answer_source: Arc<dyn question_env::QuestionAnswerSource>,
    ) -> Self {
        Self {
            question_answer_source,
            ..Self::default()
        }
    }

    #[doc(hidden)]
    pub fn with_shell_command_runner(
        mut self,
        shell_command_runner: Arc<dyn ShellCommandRunner>,
    ) -> Self {
        self.shell_command_runner = shell_command_runner;
        self
    }

    #[doc(hidden)]
    pub fn with_ast_grep_command(mut self, command: impl Into<String>) -> Self {
        self.ast_grep_command = Some(command.into());
        self
    }
}

#[doc(hidden)]
pub fn coordinator_registry_with_mcp_editing_and_executors(
    shell_allowlist: ShellAllowlist,
    mcp_config: McpConfig,
    editing: EditingToolSurfaceConfig,
    executors: CoordinatorRegistryExecutors,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_coordinator_native_tools_with_executors(
        &mut registry,
        shell_allowlist,
        editing,
        executors,
    );
    mcp::register_mcp_tools(&mut registry, mcp_config);
    registry
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
    coordinator_registry_with_mcp_editing_and_web_fetch_transport(
        shell_allowlist,
        mcp_config,
        editing,
        None,
    )
}

#[doc(hidden)]
pub fn coordinator_registry_with_mcp_editing_and_web_fetch_transport(
    shell_allowlist: ShellAllowlist,
    mcp_config: McpConfig,
    editing: EditingToolSurfaceConfig,
    web_fetch_transport: Option<Arc<dyn WebFetchHttpTransport>>,
) -> ToolRegistry {
    let executors = web_fetch_transport
        .map(CoordinatorRegistryExecutors::with_web_fetch_transport)
        .unwrap_or_default();
    coordinator_registry_with_mcp_editing_and_executors(
        shell_allowlist,
        mcp_config,
        editing,
        executors,
    )
}

#[doc(hidden)]
pub fn coordinator_registry_with_question_answers(
    shell_allowlist: ShellAllowlist,
    answers: Vec<Vec<String>>,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_coordinator_native_tools_with_question_answer_source(
        &mut registry,
        shell_allowlist,
        EditingToolSurfaceConfig::default(),
        Arc::new(question_env::ScriptedQuestionAnswerSource::new(answers)),
    );
    registry
}

#[doc(hidden)]
pub fn coordinator_registry_with_remote_search_transport(
    shell_allowlist: ShellAllowlist,
    config: RemoteSearchTestConfig,
    transport: Arc<dyn RemoteSearchHttpTransport>,
) -> Result<ToolRegistry, String> {
    Ok(coordinator_registry_with_mcp_editing_and_executors(
        shell_allowlist,
        McpConfig::default(),
        EditingToolSurfaceConfig::default(),
        CoordinatorRegistryExecutors::with_remote_search_transport(config, transport)?,
    ))
}

#[doc(hidden)]
pub fn coordinator_registry_with_web_fetch_transport(
    shell_allowlist: ShellAllowlist,
    transport: Arc<dyn WebFetchHttpTransport>,
) -> ToolRegistry {
    coordinator_registry_with_mcp_editing_and_executors(
        shell_allowlist,
        McpConfig::default(),
        EditingToolSurfaceConfig::default(),
        CoordinatorRegistryExecutors::with_web_fetch_transport(transport),
    )
}

#[doc(hidden)]
pub fn coordinator_registry_with_github_transport(
    shell_allowlist: ShellAllowlist,
    api_base_url: impl Into<String>,
    auth_token: Option<String>,
    transport: Arc<dyn GitHubHttpTransport>,
) -> ToolRegistry {
    coordinator_registry_with_mcp_editing_and_executors(
        shell_allowlist,
        McpConfig::default(),
        EditingToolSurfaceConfig::default(),
        CoordinatorRegistryExecutors::with_github_transport(api_base_url, auth_token, transport),
    )
}

#[doc(hidden)]
pub fn coordinator_registry_with_ast_grep_command(
    shell_allowlist: ShellAllowlist,
    command: impl Into<String>,
) -> ToolRegistry {
    coordinator_registry_with_mcp_editing_and_executors(
        shell_allowlist,
        McpConfig::default(),
        EditingToolSurfaceConfig::default(),
        CoordinatorRegistryExecutors::default().with_ast_grep_command(command),
    )
}

fn register_coordinator_native_tools_with_question_answer_source(
    registry: &mut ToolRegistry,
    shell_allowlist: ShellAllowlist,
    editing: EditingToolSurfaceConfig,
    question_answer_source: Arc<dyn question_env::QuestionAnswerSource>,
) {
    register_coordinator_native_tools_with_executors(
        registry,
        shell_allowlist,
        editing,
        CoordinatorRegistryExecutors::with_question_answer_source(question_answer_source),
    );
}

fn register_coordinator_native_tools_with_executors(
    registry: &mut ToolRegistry,
    shell_allowlist: ShellAllowlist,
    editing: EditingToolSurfaceConfig,
    executors: CoordinatorRegistryExecutors,
) {
    for tool in coordinator_native_tool_surface(shell_allowlist, editing, executors) {
        registry.register(tool);
    }
}

fn coordinator_native_tool_surface(
    shell_allowlist: ShellAllowlist,
    editing: EditingToolSurfaceConfig,
    executors: CoordinatorRegistryExecutors,
) -> Vec<Arc<dyn Tool>> {
    let question_answer_source = Arc::clone(&executors.question_answer_source);
    let network_executor = Arc::clone(&executors.network_executor);
    let github_executor = Arc::clone(&executors.github_executor);
    let shell_command_runner = Arc::clone(&executors.shell_command_runner);
    let ast_grep_command = executors.ast_grep_command.clone();
    let ast_grep_tool = ast_grep_command
        .clone()
        .map(AstGrepSearchTool::with_command)
        .unwrap_or_else(AstGrepSearchTool::new);
    let ast_grep_replace_tool = ast_grep_command
        .map(AstGrepReplaceTool::with_command)
        .unwrap_or_else(AstGrepReplaceTool::new);
    let agent_ops_executor = Arc::new(AgentOpsExecutor::with_question_answer_source(Arc::clone(
        &question_answer_source,
    )));
    let control_plane_executor = Arc::new(ControlPlaneExecutor::with_question_answer_source(
        Arc::clone(&question_answer_source),
    ));
    let code_lsp_executor = Arc::new(CodeLspExecutor::new());
    let code_lsp_rename_executor = Arc::new(CodeLspRenameExecutor::new());

    vec![
        boxed_tool(ReadTool::new(editing.hashline_edit)),
        boxed_tool(ListTool),
        boxed_tool(GlobTool),
        boxed_tool(GrepTool),
        boxed_tool(TaskTool::new(Arc::clone(&agent_ops_executor))),
        boxed_tool(BackgroundOutputTool::new(Arc::clone(&agent_ops_executor))),
        boxed_tool(BackgroundCancelTool::new(Arc::clone(&agent_ops_executor))),
        boxed_tool(SessionListTool),
        boxed_tool(SessionReadTool),
        boxed_tool(SessionSearchTool),
        boxed_tool(SessionInfoTool),
        boxed_tool(ast_grep_tool),
        boxed_tool(ast_grep_replace_tool),
        boxed_tool(PlanEnterTool::new(Arc::clone(&question_answer_source))),
        boxed_tool(PlanExitTool::new(Arc::clone(&question_answer_source))),
        boxed_tool(HashlineEditTool),
        boxed_tool(WriteTool),
        boxed_tool(ApplyPatchTool),
        boxed_tool(ShellRunTool::with_runner(
            shell_allowlist.clone(),
            Arc::clone(&shell_command_runner),
        )),
        boxed_tool(BashTool::with_runner(shell_allowlist, shell_command_runner)),
        boxed_tool(WebFetchTool::new(Arc::clone(&network_executor))),
        boxed_tool(WebSearchTool::new(Arc::clone(&network_executor))),
        boxed_tool(CodeSearchTool::new(Arc::clone(&network_executor))),
        boxed_tool(GitHubIssueTool::new(Arc::clone(&github_executor))),
        boxed_tool(GitHubPullRequestTool::new(github_executor)),
        boxed_tool(TodoWriteTool::new(Arc::clone(&control_plane_executor))),
        boxed_tool(TodoReadTool::new(Arc::clone(&control_plane_executor))),
        boxed_tool(SkillTool::new(Arc::clone(&control_plane_executor))),
        boxed_tool(BatchTool::new(Arc::clone(&agent_ops_executor))),
        boxed_tool(QuestionTool::new(Arc::clone(&control_plane_executor))),
        boxed_tool(CodeLspRenameTool::new(code_lsp_rename_executor)),
        boxed_tool(LspTool::new(code_lsp_executor)),
        boxed_tool(InvalidTool::new(control_plane_executor)),
    ]
}

fn boxed_tool<T: Tool + 'static>(tool: T) -> Arc<dyn Tool> {
    Arc::new(tool)
}

pub fn coordinator_registry_with_internal_hashline_tools(
    shell_allowlist: ShellAllowlist,
) -> ToolRegistry {
    let mut registry = coordinator_registry(shell_allowlist);
    register_internal_hashline_tools(&mut registry);
    registry
}

fn register_internal_hashline_tools(registry: &mut ToolRegistry) {
    for tool in internal_hashline_tool_surface() {
        registry.register(tool);
    }
}

fn internal_hashline_tool_surface() -> Vec<Arc<dyn Tool>> {
    vec![boxed_tool(HashlineScanTool), boxed_tool(HashlineApplyTool)]
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

fn json_schema_for<T: JsonSchema>() -> serde_json::Value {
    let mut schema = serde_json::to_value(
        schemars::generate::SchemaSettings::draft07()
            .into_generator()
            .into_root_schema_for::<T>(),
    )
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
pub(crate) mod test_support {
    use super::UnwrapOrAbort;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{ToolContext, ToolRunState};

    pub(crate) fn tool_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-harness-tools-tests".into(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: workspace_root.join(".artifacts"),
            actor: EventActor::new(ActorKind::Supervisor, None),
            category: Some("deep".to_string()),
            tool_call_id: tool_call_id.into(),
            current_model_ref: None,
            current_model_settings: None,
            tool_state: ToolRunState::default(),
            coordinator,
        }
    }

    pub(crate) fn read_spilled_artifact(context: &ToolContext, artifact_path: &str) -> String {
        let relative = artifact_path.strip_prefix("artifacts/").unwrap_or_abort();
        std::fs::read_to_string(context.artifacts_dir.join(relative)).unwrap_or_abort()
    }

    pub(crate) fn write_workspace_file(
        workspace_root: &Path,
        relative_path: &str,
        contents: impl AsRef<[u8]>,
    ) -> PathBuf {
        let path = workspace_root.join(relative_path);
        std::fs::write(&path, contents).unwrap_or_abort();
        path
    }
}

#[cfg(test)]
mod tests {
    use super::coordinator_registry;
    use crate::UnwrapOrAbort;
    use harness_core::config::ShellAllowlist;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn tool_parameter_schemas_are_json_objects() {
        let registry = coordinator_registry(ShellAllowlist::default());

        for tool_id in [
            "bash",
            "ast_grep_replace",
            "ast_grep_search",
            "background_cancel",
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
            "session_info",
            "session_list",
            "session_read",
            "session_search",
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
    #[allow(
        clippy::empty_structs_with_brackets,
        reason = "unit struct changes JSON schema representation"
    )]
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
            .unwrap_or_abort()
            .parameters_json_schema();
        let properties = schema["properties"].as_object().unwrap_or_abort();

        assert_eq!(properties["offset"]["minimum"], json!(1));
        assert_eq!(properties["limit"]["minimum"], json!(1));
        assert!(properties.get("hashlineAnchors").is_none());
        assert_eq!(schema["required"], json!(["path"]));
    }

    #[test]
    fn bash_description_warns_against_find_style_repo_exploration() {
        let registry = coordinator_registry(ShellAllowlist::default());
        let bash = registry.get("bash").unwrap_or_abort();
        let description = bash.description();

        assert!(description.contains("glob"));
        assert!(description.contains("grep"));
        assert!(description.contains("read"));
        assert!(description.contains("permission patterns"));
        assert!(description.contains("workspace path safety"));
    }
}
