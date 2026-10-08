use super::{
    runtime::Completion,
    tools::{Pending, PendingWork, ToolApproval, ToolWork},
    *,
};
use crate::perm::{PermissionAction, PermissionKind};

impl Runtime {
    pub fn start_tool(&mut self, mut work: ToolWork) -> Result<(), CoordinatorError> {
        if work.approval == ToolApproval::Repeated {
            self.last_tool = None;
        } else if work.approval == ToolApproval::Tool {
            let streak = self
                .last_tool
                .as_ref()
                .filter(|(digest, _)| digest == &work.permission_digest)
                .map_or(1, |(_, count)| count.saturating_add(1));
            self.last_tool = Some((work.permission_digest.clone(), streak));
            if streak >= 3 {
                let policy = work
                    .context
                    .actor
                    .agent_id
                    .as_ref()
                    .and_then(|id| self.agents.get(id))
                    .map(|a| &a.policy);
                let action =
                    self.config
                        .permission_policy
                        .check("doom_loop", work.tool.id(), policy);
                if action == PermissionAction::Deny {
                    return self.finished(Completion::Tool {
                        id: work.id,
                        result: Err(CoordinatorError::PermissionDenied("doom_loop".into())),
                        instructions: Vec::new(),
                    });
                }
                if action == PermissionAction::Ask
                    && !self
                        .grants
                        .iter()
                        .any(|g| g.kind == PermissionKind::DoomLoop && g.expires_at.is_none())
                {
                    work.yolo = false;
                    work.approval = ToolApproval::Repeated;
                    work.permission_digest = format!("doom_loop:{}", work.permission_digest);
                    let summary = format!(
                        "Repeated identical tool call: {} ({streak} consecutive calls)",
                        work.tool.id()
                    );
                    return self.ask_tool(work, "doom_loop".into(), summary);
                }
            }
        }
        if work.approval != ToolApproval::External {
            let policy = work
                .context
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| self.agents.get(id))
                .map(|a| &a.policy);
            let needs_approval =
                work.context
                    .external_directory_allow_prefixes
                    .iter()
                    .any(|path| {
                        self.config.permission_policy.check(
                            "external_directory",
                            &path.to_string_lossy(),
                            policy,
                        ) == PermissionAction::Ask
                            && !self.external_granted(&work, path)
                    });
            if needs_approval {
                work.approval = ToolApproval::External;
                work.yolo = false;
                let summary = format!(
                    "Outside workspace: {}",
                    work.context
                        .external_directory_allow_prefixes
                        .iter()
                        .map(|p| p.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return self.ask_tool(work, "external_directory".into(), summary);
            }
        }
        self.launch_tool(work);
        Ok(())
    }

    pub fn ask_tool(
        &mut self,
        work: ToolWork,
        kind: String,
        summary: String,
    ) -> Result<(), CoordinatorError> {
        let permission_id = self.id("permission")?;
        let timeout_ms = self.config.permission_policy.ask_timeout_ms();
        if let Err(error) = self.emit_hooked(
            work.context.actor.clone(),
            Some(work.id.clone()),
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: permission_id.clone(),
                kind,
                tool_call_id: Some(work.id.clone().into()),
                summary,
                request_digest: work.permission_digest.clone(),
                timeout_ms,
                default_decision: PermissionDecision::Deny,
            }),
        ) {
            let message = error.to_string();
            self.finished(Completion::Tool {
                id: work.id,
                result: Err(error),
                instructions: Vec::new(),
            })?;
            return Err(CoordinatorError::Invalid(message));
        }
        self.pending.insert(
            permission_id,
            Pending {
                id: work.id.clone(),
                deadline: (timeout_ms > 0).then(|| {
                    tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms)
                }),
                work: PendingWork::Tool(Box::new(work)),
            },
        );
        Ok(())
    }

    pub fn set_yolo_mode(&mut self, enabled: bool) -> Result<(), CoordinatorError> {
        self.accepting()?;
        self.emit_applied(
            super::handle::system(),
            None,
            EventV1::YoloModeChanged { enabled },
            |runtime, _| runtime.config.yolo_on_start = enabled,
        )?;
        if enabled {
            let pending: Vec<_> = self
                .pending
                .iter()
                .filter_map(|(id, pending)| {
                    let eligible = match &pending.work {
                        PendingWork::Tool(work) => work.yolo,
                        PendingWork::EditPaths { yolo, .. } => *yolo,
                        PendingWork::Question { .. } => false,
                    };
                    eligible.then(|| id.clone())
                })
                .collect();
            for id in pending {
                if let Err(error) = self.resolve_permission(
                    &id,
                    PermissionDecision::Allow,
                    Some("YOLO mode".into()),
                ) {
                    self.config.yolo_on_start = false;
                    self.emit(
                        super::handle::system(),
                        None,
                        EventV1::YoloModeChanged { enabled: false },
                    )?;
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}
