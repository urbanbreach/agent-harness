//! Built-in tool registry and tool implementations for filesystem, shell, and
//! hashline edit operations.
//!
//! Runtime policy lives in `harness-core`; this crate should focus on tool
//! argument validation, execution, and stable schema exposure.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::config::ShellAllowlist;
use harness_core::event::ActorKind;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

mod hashline_apply;
use hashline_apply::HashlineApplyTool;

mod hashline_scan;
use hashline_scan::HashlineScanTool;

mod fs_glob;
use fs_glob::FsGlobTool;

mod fs_ls;
use fs_ls::FsLsTool;

mod fs_grep;
use fs_grep::FsGrepTool;

mod workspace_edit;
use workspace_edit::{FsWriteTool, WorkspaceEditExecutor};

mod network;
use network::{NetworkExecutor, SearchCodeTool, SearchWebTool, WebFetchTool};

mod control_plane;
use control_plane::{
    ControlPlaneExecutor, InvalidTool, PlanExitTool, SkillLoadTool, TodoReadTool, TodoWriteTool,
    UserQuestionTool,
};

mod agent_ops;
use agent_ops::{AgentOpsExecutor, AgentSpawnTool, ToolBatchTool};

mod code_lsp;
use code_lsp::{CodeLspExecutor, CodeLspTool};

mod github;
use github::{GitHubExecutor, GitHubIssueTool, GitHubPullRequestTool};

mod lsp_support;

mod opencode_compat;
use opencode_compat::{
    ApplyPatchCompatTool, BashCompatTool, BatchCompatTool, CodeSearchCompatTool, GlobCompatTool,
    GrepCompatTool, InvalidCompatTool, ListCompatTool, LspCompatTool, QuestionCompatTool,
    ReadCompatTool, SkillCompatTool, TaskCompatTool, TodoReadCompatTool, TodoWriteCompatTool,
    WebFetchCompatTool, WebSearchCompatTool, WriteCompatTool,
};

pub use harness_core::tool::{
    canonical_tool_id_for, native_and_alias_tool_ids, native_tool_parity_matrix,
    NativeToolMigrationStatus, NativeToolParityEntry, NativeToolPermissionClass,
    NativeToolProviderExposure,
};

pub fn coordinator_registry(shell_allowlist: ShellAllowlist) -> ToolRegistry {
    let agent_ops_executor = Arc::new(AgentOpsExecutor::new());
    let control_plane_executor = Arc::new(ControlPlaneExecutor::new());
    let network_executor = Arc::new(NetworkExecutor::new());
    let code_lsp_executor = Arc::new(CodeLspExecutor::new());
    let github_executor = Arc::new(GitHubExecutor::new());
    let workspace_edit_executor = Arc::new(WorkspaceEditExecutor::new());

    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FsReadTool));
    registry.register(Arc::new(FsGlobTool));
    registry.register(Arc::new(FsLsTool));
    registry.register(Arc::new(FsGrepTool));
    registry.register(Arc::new(ReadCompatTool));
    registry.register(Arc::new(ListCompatTool));
    registry.register(Arc::new(GlobCompatTool));
    registry.register(Arc::new(GrepCompatTool));
    registry.register(Arc::new(AgentSpawnTool::new(agent_ops_executor.clone())));
    registry.register(Arc::new(TaskCompatTool::new(agent_ops_executor.clone())));
    registry.register(Arc::new(HashlineScanTool));
    registry.register(Arc::new(HashlineApplyTool));
    registry.register(Arc::new(FsWriteTool::new(workspace_edit_executor.clone())));
    registry.register(Arc::new(WriteCompatTool::new(
        workspace_edit_executor.clone(),
    )));
    registry.register(Arc::new(ApplyPatchCompatTool::new(
        workspace_edit_executor.clone(),
    )));
    registry.register(Arc::new(ShellRunTool::new(shell_allowlist.clone())));
    registry.register(Arc::new(BashCompatTool::new(shell_allowlist)));
    registry.register(Arc::new(WebFetchTool::new(network_executor.clone())));
    registry.register(Arc::new(WebFetchCompatTool::new(network_executor.clone())));
    registry.register(Arc::new(SearchWebTool::new(network_executor.clone())));
    registry.register(Arc::new(WebSearchCompatTool::new(network_executor.clone())));
    registry.register(Arc::new(SearchCodeTool::new(network_executor.clone())));
    registry.register(Arc::new(CodeSearchCompatTool::new(
        network_executor.clone(),
    )));
    registry.register(Arc::new(GitHubIssueTool::new(github_executor.clone())));
    registry.register(Arc::new(GitHubPullRequestTool::new(github_executor)));
    registry.register(Arc::new(TodoWriteTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(TodoWriteCompatTool::new(
        control_plane_executor.clone(),
    )));
    registry.register(Arc::new(TodoReadTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(TodoReadCompatTool::new(
        control_plane_executor.clone(),
    )));
    registry.register(Arc::new(SkillLoadTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(SkillCompatTool::new(
        control_plane_executor.clone(),
    )));
    registry.register(Arc::new(ToolBatchTool::new(agent_ops_executor.clone())));
    registry.register(Arc::new(BatchCompatTool::new(agent_ops_executor.clone())));
    registry.register(Arc::new(UserQuestionTool::new(
        control_plane_executor.clone(),
    )));
    registry.register(Arc::new(QuestionCompatTool::new(
        control_plane_executor.clone(),
    )));
    registry.register(Arc::new(PlanExitTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(opencode_compat::PlanExitCompatTool::new(
        control_plane_executor.clone(),
    )));
    registry.register(Arc::new(CodeLspTool::new(code_lsp_executor.clone())));
    registry.register(Arc::new(LspCompatTool::new(code_lsp_executor)));
    registry.register(Arc::new(InvalidTool::new(control_plane_executor.clone())));
    registry.register(Arc::new(InvalidCompatTool::new(control_plane_executor)));
    registry
}

pub fn worker_registry(shell_allowlist: ShellAllowlist) -> ToolRegistry {
    let coordinator = coordinator_registry(shell_allowlist);
    let mut worker = ToolRegistry::new();
    for tool in coordinator.filter_for_actor(ActorKind::Worker) {
        worker.register(tool);
    }
    worker
}

pub(crate) struct FsReadTool;

const FS_READ_DEFAULT_OFFSET: u32 = 1;
const FS_READ_DEFAULT_LIMIT: u32 = 2000;

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

#[async_trait]
impl Tool for FsReadTool {
    fn id(&self) -> &str {
        "fs.read"
    }

    fn description(&self) -> &str {
        "Reads UTF-8 text from a workspace file with optional offset/limit and line numbers."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        json_schema_for::<FsReadArgs>()
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

        let path = Path::new(&args.path);
        if path.is_absolute() {
            return Err(ToolError::InvalidArguments(
                "path must be relative to workspace root".to_string(),
            ));
        }

        if args.offset == 0 {
            return Err(ToolError::InvalidArguments(
                "offset must be >= 1".to_string(),
            ));
        }

        if args.limit == 0 {
            return Err(ToolError::InvalidArguments(
                "limit must be >= 1".to_string(),
            ));
        }

        let resolved = ctx.resolve_workspace_path(path)?;
        let bytes = std::fs::read(&resolved)
            .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| ToolError::Execution("binary file not supported".to_string()))?;

        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();
        let start_line_index = (args.offset - 1) as usize;

        let available_lines: &[&str] = if start_line_index < total_lines {
            &lines[start_line_index..]
        } else {
            &[]
        };

        let line_limit = args.limit as usize;
        let shown_count = available_lines.len().min(line_limit);
        let truncated = available_lines.len() > line_limit;
        let shown_lines = &available_lines[..shown_count];

        let mut display_text =
            format_fs_read_lines(shown_lines, start_line_index, args.line_numbers);
        let mut artifacts = Vec::new();

        if truncated {
            let full_output =
                format_fs_read_lines(available_lines, start_line_index, args.line_numbers);
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

        Ok(ToolResult {
            display_text,
            structured_json: Some(json!({
                "path": args.path,
                "resolved_path": resolved.display().to_string(),
                "offset": args.offset,
                "limit": args.limit,
                "total_lines": total_lines,
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
        "Runs an allowlisted shell command inside the workspace and returns stdout/stderr."
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
        let (display_text, structured_json) = if let Some(cmd) = args.cmd {
            if !self.is_executable_allowed(&cmd) {
                return Err(ToolError::CommandBlocked(cmd));
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
            let display_text = if stderr.is_empty() {
                stdout.clone()
            } else if stdout.is_empty() {
                stderr.clone()
            } else {
                format!("{stdout}\n{stderr}")
            };
            let structured_json = json!({
                "cmd": cmd,
                "args": args.args,
                "cwd": args.cwd,
                "status": status,
                "stdout": stdout,
                "stderr": stderr,
                "success": output.status.success(),
            });

            (display_text, structured_json)
        } else {
            let command = args.command.expect("validated shell command presence");
            let workdir = args.workdir.or(args.cwd);
            let cwd = self.resolve_cwd(&ctx, workdir.as_deref())?;
            crate::opencode_compat::validate_bash_command(
                &command,
                &cwd,
                &ctx.workspace_root,
                &self.allowlist,
            )?;
            let shell = std::env::var("SHELL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "/bin/bash".to_string());
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
            let display_text = if stderr.is_empty() {
                stdout.clone()
            } else if stdout.is_empty() {
                stderr.clone()
            } else {
                format!("{stdout}\n{stderr}")
            };
            let structured_json = json!({
                "description": args.description,
                "command": command,
                "workdir": workdir,
                "status": status,
                "stdout": stdout,
                "stderr": stderr,
                "success": output.status.success(),
            });

            (display_text, structured_json)
        };

        Ok(ToolResult {
            display_text,
            structured_json: Some(structured_json),
            artifacts: Vec::new(),
        })
    }
}

fn json_schema_for<T: JsonSchema>() -> serde_json::Value {
    match serde_json::to_value(schemars::schema_for!(T).schema) {
        Ok(schema) => schema,
        Err(_) => json!({"type": "object"}),
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
            plan_mode: false,
            plan_exit_target_profile: None,
            tool_call_id: tool_call_id.to_string(),
            coordinator,
        }
    }

    #[test]
    fn tool_parameter_schemas_are_json_objects() {
        let registry = coordinator_registry(ShellAllowlist::default());

        for tool_id in [
            "agent.spawn",
            "fs.read",
            "fs.glob",
            "fs.ls",
            "fs.grep",
            "github.issue",
            "github.pull_request",
            "shell.run",
            "edit.hashline_apply",
            "edit.hashline_scan",
        ] {
            let tool = registry
                .get(tool_id)
                .unwrap_or_else(|| panic!("missing tool {tool_id}"));
            assert!(
                tool.parameters_json_schema().is_object(),
                "schema for {tool_id} must be an object"
            );
        }
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

    #[tokio::test]
    async fn fs_read_supports_offset_and_limit_with_line_numbers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "line one\nline two\nline three\n").expect("write fixture");

        let tool = FsReadTool;
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
    async fn fs_read_adds_truncation_marker_and_spills_full_output_artifact() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("fixture.txt");
        std::fs::write(&source, "alpha\nbeta\ngamma\ndelta\n").expect("write fixture");

        let context = fs_read_context(temp.path(), "toolcall-truncated");
        let tool = FsReadTool;
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

        let tool = FsReadTool;
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
}
