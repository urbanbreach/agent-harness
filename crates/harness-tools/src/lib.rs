use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::config::ShellAllowlist;
use harness_core::event::ActorKind;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use serde::Deserialize;
use serde_json::json;

mod hashline_apply;
use hashline_apply::HashlineApplyTool;

pub fn coordinator_registry(shell_allowlist: ShellAllowlist) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(FsReadTool));
    registry.register(Arc::new(HashlineApplyTool));
    registry.register(Arc::new(ShellRunTool::new(shell_allowlist)));
    registry
}

pub fn worker_registry(shell_allowlist: ShellAllowlist) -> ToolRegistry {
    let coordinator = coordinator_registry(shell_allowlist);
    let mut worker = ToolRegistry::new();
    for tool in coordinator.filter_for_actor(ActorKind::Worker) {
        if tool.capability() != ToolCapability::SpawnAgent {
            worker.register(tool);
        }
    }
    worker
}

struct FsReadTool;

#[derive(Deserialize)]
struct FsReadArgs {
    path: String,
}

#[async_trait]
impl Tool for FsReadTool {
    fn id(&self) -> &str {
        "fs.read"
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

        let resolved = ctx.resolve_workspace_path(path)?;
        let text = std::fs::read_to_string(&resolved)
            .map_err(|err| ToolError::Execution(format!("failed to read file: {err}")))?;

        Ok(ToolResult {
            display_text: text.clone(),
            structured_json: Some(json!({
                "path": args.path,
                "resolved_path": resolved.display().to_string(),
                "bytes": text.len(),
            })),
            artifacts: Vec::new(),
        })
    }
}

struct ShellRunTool {
    allowlist: ShellAllowlist,
}

impl ShellRunTool {
    fn new(allowlist: ShellAllowlist) -> Self {
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

#[derive(Deserialize)]
struct ShellRunArgs {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[async_trait]
impl Tool for ShellRunTool {
    fn id(&self) -> &str {
        "shell.run"
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

        if !self.is_executable_allowed(&args.cmd) {
            return Err(ToolError::CommandBlocked(args.cmd));
        }

        let cwd = self.resolve_cwd(&ctx, args.cwd.as_deref())?;

        let output = Command::new(&args.cmd)
            .args(&args.args)
            .current_dir(cwd)
            .output()
            .map_err(|err| ToolError::Execution(format!("failed to execute command: {err}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let status = output.status.code().unwrap_or(-1);

        Ok(ToolResult {
            display_text: stdout.clone(),
            structured_json: Some(json!({
                "status": status,
                "stdout": stdout,
                "stderr": stderr,
                "success": output.status.success(),
            })),
            artifacts: Vec::new(),
        })
    }
}
