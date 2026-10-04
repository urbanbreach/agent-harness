use super::{
    runtime::{digest, JobKind},
    tools::{Pending, PendingWork},
    *,
};
use crate::{perm::PermissionAction, tool::ToolCapability};
use std::path::Path;
use tokio::time::{Duration, Instant};

impl CoordinatorHandle {
    /// Refactoring tools discover targets after starting. Approve the complete set before writing.
    pub async fn authorize_tool_edit_paths(
        &self,
        task: String,
        paths: Vec<PathBuf>,
    ) -> Result<Vec<PathBuf>, CoordinatorError> {
        self.authorize_tool_paths(task, paths, &["edit"]).await
    }
    /// Approve paths discovered by a running refactoring tool before reading or editing them.
    pub async fn authorize_tool_paths(
        &self,
        task: String,
        paths: Vec<PathBuf>,
        permissions: &[&str],
    ) -> Result<Vec<PathBuf>, CoordinatorError> {
        let permissions: Vec<String> = permissions.iter().map(|p| (*p).into()).collect();
        let (reply, result) = oneshot::channel();
        self.call(move |s| s.request_edit_paths(&task, paths, permissions, reply))
            .await?;
        result.await.map_err(|_| CoordinatorError::Closed)?
    }
}
impl Runtime {
    pub(super) fn validate_edit_path(&self, path: &Path) -> Result<(), CoordinatorError> {
        if path.starts_with(self.config.session_dir.canonicalize()?)
            || path
                == crate::tool::resolve_file_path(
                    &self.info()?.workspace_root,
                    Path::new(super::grants::GRANTS_FILE),
                )
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?
        {
            return Err(CoordinatorError::PermissionDenied(
                "managed session and permission files cannot be edited by tools".into(),
            ));
        }
        Ok(())
    }
    fn request_edit_paths(
        &mut self,
        task: &str,
        paths: Vec<PathBuf>,
        permissions: Vec<String>,
        reply: Reply<Vec<PathBuf>>,
    ) -> Result<(), CoordinatorError> {
        self.check_task(task)?;
        if permissions.is_empty()
            || permissions.len() > 3
            || permissions
                .iter()
                .any(|p| !matches!(p.as_str(), "read" | "lsp" | "edit"))
        {
            return Err(CoordinatorError::Invalid(
                "invalid refactoring permissions".into(),
            ));
        }
        let editing = permissions.iter().any(|p| p == "edit");
        let job = &self.running[task];
        let JobKind::Tool {
            tool_id,
            capability: ToolCapability::EditFs,
            paths: approved,
            ..
        } = &job.kind
        else {
            return Err(CoordinatorError::PermissionDenied(
                "additional edit paths require an editing tool".into(),
            ));
        };
        if job.join_id.is_none() || paths.len() > 256 || approved.len() + paths.len() > 1024 {
            return Err(CoordinatorError::Invalid(
                "edit tool is not running or its path limit was exceeded".into(),
            ));
        }
        let root = &self.info()?.workspace_root;
        let cwd = self.execution_cwd(&job.actor)?;
        let policy = job
            .actor
            .agent_id
            .as_ref()
            .and_then(|id| self.agents.get(id))
            .map(|agent| &agent.policy);
        let mut ask = false;
        let mut yolo = true;
        let mut resolved = Vec::with_capacity(paths.len());
        for input in paths {
            let path = crate::tool::resolve_file_path(&cwd, &input)
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
            yolo &= path.starts_with(root);
            if editing {
                self.validate_edit_path(&path)?;
            }
            if !path.starts_with(root) && !approved.contains(&path) {
                return Err(CoordinatorError::PermissionDenied(
                    "additional edit paths must stay within the workspace".into(),
                ));
            }
            for (value, permission) in [&input, &path].into_iter().flat_map(|value| {
                permissions
                    .iter()
                    .map(move |permission| (value, permission))
            }) {
                yolo &= super::grants::can_auto_approve(permission, value);
                let selector = value.strip_prefix(root).unwrap_or(value).to_string_lossy();
                match self
                    .config
                    .permission_policy
                    .check(permission, &selector, policy)
                {
                    PermissionAction::Deny => {
                        return Err(CoordinatorError::PermissionDenied(format!(
                            "{permission} {selector}"
                        )))
                    }
                    PermissionAction::Ask if permission != "edit" || !approved.contains(&path) => {
                        ask = true
                    }
                    _ => {}
                }
            }
            resolved.push(path);
        }
        resolved.sort();
        resolved.dedup();
        let tool_id = tool_id.clone();
        let actor = job.actor.clone();
        let summary = serde_json::to_string(
            &serde_json::json!({"paths":resolved,"permissions":permissions}),
        )?;
        let permission_digest = digest(&format!("edit-paths:{tool_id}:{summary}"));
        if !ask
            || (self.config.yolo_on_start && yolo)
            || self.has_grant(&tool_id, &permission_digest)
        {
            let _ = reply.send(self.approve_edit_paths(task, resolved, editing));
            return Ok(());
        }
        let permission = self.id("permission")?;
        let timeout_ms = self.config.permission_policy.ask_timeout_ms();
        self.emit_hooked(
            actor,
            Some(task.into()),
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: permission.clone(),
                kind: tool_id,
                tool_call_id: Some(task.into()),
                summary,
                request_digest: permission_digest.clone(),
                timeout_ms,
                default_decision: PermissionDecision::Deny,
            }),
        )?;
        self.pending.insert(
            permission,
            Pending {
                id: task.into(),
                deadline: (timeout_ms > 0)
                    .then(|| Instant::now() + Duration::from_millis(timeout_ms)),
                work: PendingWork::EditPaths {
                    paths: resolved,
                    editing,
                    yolo,
                    permission_digest,
                    reply,
                },
            },
        );
        Ok(())
    }
    pub(super) fn approve_edit_paths(
        &mut self,
        task: &str,
        paths: Vec<PathBuf>,
        editing: bool,
    ) -> Result<Vec<PathBuf>, CoordinatorError> {
        self.check_task(task)?;
        for path in &paths {
            if editing {
                self.validate_edit_path(path)?;
            }
            if crate::tool::resolve_file_path(&self.info()?.workspace_root, path)
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?
                != *path
            {
                return Err(CoordinatorError::Invalid(
                    "edit target changed during approval".into(),
                ));
            }
        }
        if let Some(runtime::Job {
            kind: JobKind::Tool {
                paths: approved, ..
            },
            ..
        }) = self.running.get_mut(task)
        {
            if editing {
                approved.extend(paths.iter().cloned());
                approved.sort();
                approved.dedup();
            }
        }
        Ok(paths)
    }
}
