use super::{
    handle::system,
    tools::{PendingWork, ToolApproval, ToolWork},
    *,
};
use crate::{perm::*, store, tool::ToolCapability};
use std::path::Path;
pub(super) const GRANTS_FILE: &str = ".agent-harness/permission-grants.json";
const MAX_GRANTS: usize = 4096;

pub(super) fn can_auto_approve(permission: &str, path: &Path) -> bool {
    !matches!(permission, "question" | "external_directory" | "doom_loop")
        && (!matches!(permission, "read" | "lsp")
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.ends_with(".env")
                        || name.contains(".env.") && !name.ends_with(".env.example")
                }))
}

impl CoordinatorHandle {
    pub async fn resolve_permission_with_grant_scope(
        &self,
        id: impl Into<String>,
        decision: PermissionDecision,
        reason: Option<String>,
        scope: Option<PermissionGrantScope>,
    ) -> Result<(), CoordinatorError> {
        let id = id.into();
        self.call(move |s| s.resolve_permission_scoped(&id, decision, reason, scope))
            .await
    }
}
impl Runtime {
    fn tool_matcher(&self, work: &ToolWork) -> PermissionGrantMatcher {
        let request_digest = work.permission_digest.clone();
        let matcher = match work.tool.capability() {
            ToolCapability::Shell => work
                .args
                .get("command")
                .and_then(serde_json::Value::as_str)
                .map(|command| PermissionGrantMatcher::ShellCommand {
                    command_digest: super::runtime::digest(command),
                    request_digest: request_digest.clone(),
                    patterns: Vec::new(),
                    always_patterns: work.tool.permission_always_patterns(&work.args),
                }),
            ToolCapability::ReadFs | ToolCapability::EditFs => {
                match work.context.approved_paths.as_slice() {
                    [(original, resolved)]
                        if original == resolved
                            && resolved.starts_with(&work.context.workspace_root) =>
                    {
                        Some(PermissionGrantMatcher::WorkspacePath {
                            path: resolved.to_string_lossy().into(),
                            request_digest: request_digest.clone(),
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        matcher
            .filter(|matcher| {
                serde_json::to_string(matcher)
                    .is_ok_and(|s| s.len() <= 16 * 1024 && self.redactor.redact_text(&s) == s)
            })
            .unwrap_or(PermissionGrantMatcher::RequestDigest { request_digest })
    }
    pub fn tool_granted(&self, work: &ToolWork) -> bool {
        if self.grants.is_empty() {
            return false;
        }
        let matcher = self.tool_matcher(work);
        self.grants.iter().any(|g| {
            g.expires_at.is_none()
                && g.tool.effective_tool_id == work.tool.id()
                && g.kind == capability_kind(work.tool.capability(), work.tool.id())
                && g.matcher.matches(&matcher)
        })
    }
    pub fn external_granted(&self, work: &ToolWork, path: &Path) -> bool {
        let matcher = PermissionGrantMatcher::ExternalPath {
            path_prefix: path.to_string_lossy().into(),
            request_digest: work.permission_digest.clone(),
        };
        self.grants.iter().any(|g| {
            g.expires_at.is_none()
                && g.tool.effective_tool_id == work.tool.id()
                && g.kind == PermissionKind::ExternalDirectory
                && g.matcher.matches(&matcher)
        })
    }
    pub fn has_grant(&self, tool: &str, digest: &str) -> bool {
        self.grants.iter().any(|g| g.expires_at.is_none() && g.tool.effective_tool_id == tool
            && matches!(&g.matcher, PermissionGrantMatcher::RequestDigest {request_digest} if request_digest == digest))
    }
    pub(super) fn record_grant(
        &mut self,
        permission: &str,
        scope: PermissionGrantScope,
    ) -> Result<(), CoordinatorError> {
        let pending = self
            .pending
            .get(permission)
            .ok_or_else(|| CoordinatorError::UnknownPermission(permission.into()))?;
        self.check_task(&pending.id)?;
        let (capability, tool_id, permission_digest) = match &pending.work {
            PendingWork::Tool(work) => (
                work.tool.capability(),
                work.tool.id(),
                &work.permission_digest,
            ),
            PendingWork::EditPaths {
                permission_digest,
                editing,
                ..
            } => {
                let super::runtime::JobKind::Tool { tool_id, .. } = &self.running[&pending.id].kind
                else {
                    return Err(CoordinatorError::Invalid(
                        "edit approval requires a tool".into(),
                    ));
                };
                (
                    if *editing {
                        ToolCapability::EditFs
                    } else {
                        ToolCapability::ReadFs
                    },
                    tool_id.as_str(),
                    permission_digest,
                )
            }
            PendingWork::Question { .. } => {
                return Err(CoordinatorError::Invalid(
                    "question answers cannot become permission grants".into(),
                ))
            }
        };
        let kind = match &pending.work {
            PendingWork::Tool(work) if work.approval == ToolApproval::Repeated => {
                PermissionKind::DoomLoop
            }
            PendingWork::Tool(work) if work.approval == ToolApproval::External => {
                PermissionKind::ExternalDirectory
            }
            _ => capability_kind(capability, tool_id),
        };
        let tool = PermissionToolSelector {
            effective_tool_id: tool_id.into(),
            canonical_tool_id: Some(tool_id.into()),
        };
        let matchers = match &pending.work {
            PendingWork::Tool(work) if work.approval == ToolApproval::External => work
                .context
                .external_directory_allow_prefixes
                .iter()
                .map(|path| {
                    let prefix = path
                        .parent()
                        .filter(|p| p.parent().is_some())
                        .unwrap_or(path);
                    if prefix.parent().is_none() {
                        return PermissionGrantMatcher::RequestDigest {
                            request_digest: permission_digest.clone(),
                        };
                    }
                    PermissionGrantMatcher::ExternalPath {
                        path_prefix: prefix.to_string_lossy().into(),
                        request_digest: permission_digest.clone(),
                    }
                })
                .collect(),
            PendingWork::Tool(work) if work.approval == ToolApproval::Tool => {
                vec![self.tool_matcher(work)]
            }
            _ => vec![PermissionGrantMatcher::RequestDigest {
                request_digest: permission_digest.clone(),
            }],
        };
        let task = pending.id.clone();
        if self.grants.len().saturating_add(matchers.len()) > MAX_GRANTS {
            return Err(CoordinatorError::Invalid(
                "permission grant limit reached".into(),
            ));
        }
        for mut matcher in matchers {
            let encoded = serde_json::to_string(&matcher)?;
            if self.redactor.redact_text(&encoded) != encoded {
                matcher = PermissionGrantMatcher::RequestDigest {
                    request_digest: matcher.digest().into(),
                };
            }
            let grant = PermissionGrant {
                grant_id: self.id("grant")?,
                permission_id: permission.into(),
                scope,
                expires_at: None,
                kind,
                tool: tool.clone(),
                matcher,
            };
            self.emit(
                system(),
                Some(task.clone()),
                EventV1::PermissionGrantRecorded(PermissionGrantRecordedEvent {
                    grant: grant.clone(),
                }),
            )?;
            if scope == PermissionGrantScope::Workspace {
                let path = self.info()?.workspace_root.join(GRANTS_FILE);
                let _lock = store::lock_private_parent(&path)?;
                let mut grants = workspace_grants(&self.info()?.workspace_root)?;
                if !grants
                    .iter()
                    .any(|g| g.tool == grant.tool && g.matcher == grant.matcher)
                {
                    grants.push(grant.clone());
                }
                let bytes = serde_json::to_vec(&grants)?;
                if grants.len() > MAX_GRANTS || bytes.len() > 1024 * 1024 {
                    return Err(CoordinatorError::Invalid(
                        "workspace permission grant limit reached".into(),
                    ));
                }
                store::write_private_atomic(&path, &bytes)?;
            }
            self.grants.push(grant);
        }
        Ok(())
    }
}
fn capability_kind(capability: ToolCapability, tool_id: &str) -> PermissionKind {
    if tool_id == "eval" {
        return PermissionKind::Eval;
    }
    match capability {
        ToolCapability::ReadFs => PermissionKind::Read,
        ToolCapability::EditFs => PermissionKind::EditFs,
        ToolCapability::Shell => PermissionKind::Shell,
        ToolCapability::Network => PermissionKind::Network,
        ToolCapability::SpawnAgent => PermissionKind::Task,
    }
}
fn workspace_grants(root: &Path) -> Result<Vec<PermissionGrant>, CoordinatorError> {
    let Some(bytes) = store::read_private_bytes(&root.join(GRANTS_FILE), 1024 * 1024)? else {
        return Ok(Vec::new());
    };
    let grants: Vec<PermissionGrant> = serde_json::from_slice(&bytes)
        .map_err(|_| CoordinatorError::Invalid("invalid workspace permission grants".into()))?;
    if grants.len() > MAX_GRANTS
        || grants
            .iter()
            .any(|g| g.scope != PermissionGrantScope::Workspace)
    {
        return Err(CoordinatorError::Invalid(
            "invalid workspace permission grant scope or count".into(),
        ));
    }
    Ok(grants)
}
pub(super) fn load_grants(
    root: &Path,
    events: &[EventEnvelopeV1],
) -> Result<Vec<PermissionGrant>, CoordinatorError> {
    let mut grants = workspace_grants(root)?;
    for event in events {
        if let EventV1::PermissionGrantRecorded(e) = &event.payload {
            if e.grant.scope == PermissionGrantScope::Session {
                if grants.len() >= MAX_GRANTS {
                    return Err(CoordinatorError::Invalid(
                        "session permission grant limit reached".into(),
                    ));
                }
                grants.push(e.grant.clone());
            }
        }
    }
    for grant in &mut grants {
        if let PermissionGrantMatcher::WorkspacePath { path, .. } = &mut grant.matcher {
            *path = root.join(&*path).to_string_lossy().into_owned();
        }
    }
    Ok(grants)
}
