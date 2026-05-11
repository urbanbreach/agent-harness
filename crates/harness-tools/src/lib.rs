//! Built-in tool registry and tool implementations for filesystem, shell, and
//! hashline edit operations.
//!
//! Runtime policy lives in `harness-core`; this crate should focus on tool
//! argument validation, execution, and stable schema exposure.

use harness_core::config::{McpConfig, ShellAllowlist};
use harness_core::event::ActorKind;
use harness_core::tool::{ArtifactRef, ToolError, ToolRegistry, ToolResult};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::sync::Arc;

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
mod workspace_paths;

mod network;
use network::NetworkExecutor;

mod mcp;

mod control_plane;
use control_plane::ControlPlaneExecutor;

mod question_env;

mod env_vars;

mod read_window;

mod fs_read;
pub(crate) use fs_read::FsReadTool;

mod limit_summary;

mod text;

mod shell_run;
pub(crate) use shell_run::ShellRunTool;

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
    BackgroundOutputTool, BashTool, BatchTool, CodeSearchTool, GlobTool, GrepTool, InvalidTool,
    ListTool, LspTool, QuestionTool, ReadTool, SkillTool, TaskTool, TodoReadTool, TodoWriteTool,
    WebFetchTool, WebSearchTool,
};

mod plan;
use plan::{PlanEnterTool, PlanExitTool};

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
    registry.register(Arc::new(PlanEnterTool));
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
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use harness_core::clock::RealClock;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::event::{ActorKind, EventActor};
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::ToolContext;

    pub(crate) fn tool_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        ToolContext {
            run_id: "run-harness-tools-tests".to_string(),
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

    pub(crate) fn read_spilled_artifact(context: &ToolContext, artifact_path: &str) -> String {
        let relative = artifact_path
            .strip_prefix("artifacts/")
            .expect("artifact path prefix");
        std::fs::read_to_string(context.artifacts_dir.join(relative))
            .expect("read spilled artifact")
    }

    pub(crate) fn write_workspace_file(
        workspace_root: &Path,
        relative_path: &str,
        contents: impl AsRef<[u8]>,
    ) -> PathBuf {
        let path = workspace_root.join(relative_path);
        std::fs::write(&path, contents).expect("write workspace fixture");
        path
    }
}

#[cfg(test)]
mod tests {
    use super::coordinator_registry;
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
    fn bash_description_warns_against_find_style_repo_exploration() {
        let registry = coordinator_registry(ShellAllowlist::default());
        let bash = registry.get("bash").expect("bash tool");
        let description = bash.description();

        assert!(description.contains("find"));
        assert!(description.contains("glob"));
        assert!(description.contains("grep"));
        assert!(description.contains("read"));
    }
}
