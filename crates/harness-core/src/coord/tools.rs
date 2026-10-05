use super::{runtime::*, *};
use crate::{
    perm::PermissionAction,
    tool::{Tool, ToolCapability, ToolContext},
};
use serde_json::Value;
use tokio::time::{Duration, Instant};

pub(super) struct Pending {
    pub id: String,
    pub deadline: Option<Instant>,
    pub work: PendingWork,
}
pub(super) enum PendingWork {
    Tool(Box<ToolWork>),
    Question {
        request: crate::tool::QuestionRequest,
        reply: Reply<Value>,
    },
    EditPaths {
        paths: Vec<PathBuf>,
        editing: bool,
        yolo: bool,
        permission_digest: String,
        reply: Reply<Vec<PathBuf>>,
    },
}
pub(super) struct ToolWork {
    pub id: String,
    pub tool: Arc<dyn Tool>,
    pub args: Value,
    pub permission_digest: String,
    pub yolo: bool,
    pub approval: ToolApproval,
    pub context: ToolContext,
}
#[derive(PartialEq, Eq)]
pub(super) enum ToolApproval {
    Tool,
    Repeated,
    External,
}

impl Runtime {
    pub fn request_tool(
        &mut self,
        actor: EventActor,
        parent: Option<String>,
        id: Option<String>,
        tool_id: String,
        mut args: Value,
        reply: Option<Reply<ToolResult>>,
    ) -> Result<String, CoordinatorError> {
        self.accepting()?;
        // Keep permission digests independent of dependency-selected JSON map ordering.
        args.sort_all_objects();
        if self.stopped_sessions.contains(self.info()?.run_id.as_str())
            || actor.agent_id.as_ref().is_some_and(|id| {
                self.stopped_sessions.contains(id) || self.killed_agents.contains(id)
            })
        {
            return Err(CoordinatorError::Stopping);
        }
        if let Some(parent) = &parent {
            self.check_task(parent)?;
        }
        if self
            .running
            .values()
            .filter(|job| matches!(job.kind, JobKind::Tool { .. }))
            .count()
            >= self
                .config
                .command_buffer
                .saturating_add(self.config.tool_concurrency)
        {
            return Err(CoordinatorError::Invalid("tool queue is full".into()));
        }
        if !args.is_object() || args.to_string().len() > 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "tool arguments must be an object of at most 1 MiB".into(),
            ));
        }
        let tool = self
            .config
            .tool_registry
            .get_for(
                &tool_id,
                self.tool_scope(actor.agent_id.as_deref()).as_deref(),
            )
            .or_else(|| {
                (tool_id == "question")
                    .then(|| Arc::new(crate::tool::QuestionTool) as Arc<dyn Tool>)
            })
            .ok_or_else(|| CoordinatorError::UnknownTool(tool_id.clone()))?;
        if tool_id == "skill"
            && let Some(agent) = actor.agent_id.as_deref()
        {
            self.ensure_skill_startup(agent)?;
        }
        let profile = actor
            .agent_id
            .as_ref()
            .map(|id| {
                self.agents
                    .get(id)
                    .ok_or_else(|| CoordinatorError::UnknownAgent(id.clone()))
            })
            .transpose()?;
        if profile.is_some_and(|agent| !self.config.tool_registry.allows(&agent.profile, &tool_id))
        {
            return Err(CoordinatorError::PermissionDenied(format!(
                "tool {tool_id} is not enabled for this agent"
            )));
        }
        let mut requirements = tool.permission_requests(&args);
        if matches!(tool_id.as_str(), "spawn_subagent" | "task") {
            let selector = self.native_spawn_selector(&actor, &args)?;
            requirements.retain(|(permission, _)| permission != "task");
            requirements.push(("task".into(), selector));
        }
        let mut external = Vec::new();
        let cwd = self.execution_cwd(&actor)?;
        let policy_root = self.info()?.workspace_root.clone();
        let tool_state = profile
            .map(|a| a.tool_state.clone())
            .unwrap_or_else(|| self.tool_state.clone());
        let mut paths = Vec::new();
        let mut approved_paths = Vec::new();
        let mut yolo = requirements.iter().all(|(permission, value)| {
            super::grants::can_auto_approve(permission, PathBuf::from(value).as_path())
        });
        for input in tool
            .filesystem_paths(&args)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?
        {
            let path = crate::tool::resolve_file_path(&cwd, &input)
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
            approved_paths.push((cwd.join(&input), path.clone()));
            yolo &= tool.capability() != ToolCapability::ReadFs
                || [&input, &path]
                    .into_iter()
                    .all(|p| super::grants::can_auto_approve("read", p));
            if matches!(
                tool.capability(),
                ToolCapability::EditFs | ToolCapability::Shell
            ) {
                self.validate_edit_path(&path)?;
            }
            paths.push(path.clone());
            let selector = path.strip_prefix(&policy_root).unwrap_or(&path);
            let selector = if selector.as_os_str().is_empty() {
                ".".into()
            } else {
                selector.to_string_lossy().into_owned()
            };
            let canonical: Vec<_> = requirements
                .iter()
                .filter(|(_, value)| {
                    value == input.to_string_lossy().as_ref() && value != &selector
                })
                .map(|(permission, _)| (permission.clone(), selector.clone()))
                .collect();
            requirements.extend(canonical);
            if !path.starts_with(&policy_root) {
                requirements.push((
                    "external_directory".into(),
                    path.to_string_lossy().into_owned(),
                ));
                external.push(path);
            }
        }
        let action = requirements
            .iter()
            .map(|(permission, value)| {
                let action = self.config.permission_policy.check(
                    permission,
                    value,
                    profile.map(|agent| &agent.policy),
                );
                if permission == "external_directory" && action == PermissionAction::Ask {
                    PermissionAction::Allow
                } else {
                    action
                }
            })
            .fold(
                self.skill_permission(&actor, &tool_id, &args)?,
                |a, b| match (a, b) {
                    (PermissionAction::Deny, _) | (_, PermissionAction::Deny) => {
                        PermissionAction::Deny
                    }
                    (PermissionAction::Ask, _) | (_, PermissionAction::Ask) => {
                        PermissionAction::Ask
                    }
                    _ => PermissionAction::Allow,
                },
            );
        let profile_name = profile.map(|a| a.profile.name.clone());
        let model = profile.map(|a| a.info.model_ref.clone());
        let settings = profile.map(|a| a.settings.clone());
        let skill_startup = profile.and_then(|agent| agent.skill_startup.clone());
        let id = match id {
            Some(id) => id,
            None => self.id("tool")?,
        };
        if self.running.contains_key(&id) {
            return Err(CoordinatorError::Invalid("duplicate tool call".into()));
        }
        let args_summary = crate::redact::redact_value(self.redactor.as_ref(), &args).to_string();
        let args_digest = digest(&args.to_string());
        let permission_digest = digest(&serde_json::to_string(&(
            &tool_id,
            &args,
            &requirements,
            &paths,
        ))?);
        self.emit(
            actor.clone(),
            parent.clone(),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: id.clone().into(),
                tool_id: tool_id.clone(),
                args_summary: args_summary.clone(),
                args_digest: args_digest.clone(),
                metadata: self
                    .tool_lineage(parent.as_deref())
                    .map(|lineage| ToolCallMetadata {
                        lineage: Some(lineage),
                        ..Default::default()
                    }),
            }),
        )?;
        let cancellation = parent
            .as_ref()
            .and_then(|id| self.running.get(id))
            .map_or_else(CancellationToken::new, |job| job.cancellation.child_token());
        let context = ToolContext {
            run_id: self.info()?.run_id.to_string(),
            interactive: self.config.interactive,
            workspace_root: cwd,
            policy_roots: vec![policy_root],
            artifacts_dir: self.info()?.artifacts_dir.clone(),
            actor: actor.clone(),
            profile: profile_name,
            tool_call_id: id.clone().into(),
            current_model_ref: model,
            current_model_settings: settings,
            skill_startup,
            coordinator: self.handle()?,
            cancellation: cancellation.clone(),
            external_directory_allow_prefixes: external,
            approved_paths,
            tool_state,
            formatter: Arc::clone(&self.config.formatter),
            redactor: Arc::clone(&self.redactor),
        };
        self.running.insert(
            id.clone(),
            Job {
                join_id: None,
                actor: actor.clone(),
                kind: JobKind::Tool {
                    tool_id: tool_id.clone(),
                    reply,
                    capability: tool.capability(),
                    paths,
                },
                parent,
                cancellation,
                reason: None,
                hooks: Vec::new(),
            },
        );
        let work = ToolWork {
            id: id.clone(),
            tool,
            args,
            permission_digest: permission_digest.clone(),
            yolo,
            approval: ToolApproval::Tool,
            context,
        };
        match action {
            PermissionAction::Deny => {
                self.finished(Completion::Tool {
                    id: id.clone(),
                    result: Err(CoordinatorError::PermissionDenied(tool_id)),
                })?;
            }
            PermissionAction::Ask
                if !(self.config.yolo_on_start && yolo || self.tool_granted(&work))
                    && tool_id != "question" =>
            {
                self.ask_tool(work, tool_id, args_summary)?;
            }
            _ => self.start_tool(work)?,
        }
        Ok(id)
    }
    pub(super) fn launch_tool(&mut self, work: ToolWork) {
        let semaphore = Arc::clone(&self.tools);
        let id = work.id.clone();
        let task = self.jobs.spawn(async move {
            let token = work.context.cancellation.clone();
            let result = async {
                // A parent waiting for a child must not hold the child's I/O capacity.
                let permit = if work.tool.capability() == ToolCapability::SpawnAgent || work.tool.id() == "question" { None } else {
                    Some(tokio::select! {
                        biased;
                        () = token.cancelled() => return Err(CoordinatorError::Cancelled(work.id.clone())),
                        permit = semaphore.acquire_owned() => permit.map_err(|_| CoordinatorError::Closed)?,
                    })
                };
                let id = work.id.clone();
                work.context.coordinator.call(move |s| {
                    s.check_task(&id)?;
                    let actor = s.running.get(&id).ok_or_else(|| CoordinatorError::UnknownTask(id.clone()))?.actor.clone();
                    s.emit_hooked(actor, Some(id.clone()), EventV1::ToolCallStarted(ToolCallStartedEvent { tool_call_id: id.into() })).map(|_| ())
                }).await?;
                // Tools observe cancellation and finish cleanup before this future returns.
                let result = work.tool.call(work.context, work.args).await.map_err(CoordinatorError::from);
                drop(permit);
                result
            }.await;
            Completion::Tool { id: work.id, result }
        });
        if let Some(job) = self.running.get_mut(&id) {
            job.join_id = Some(task.id());
        }
    }
    pub fn resolve_permission(
        &mut self,
        id: &str,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> Result<(), CoordinatorError> {
        self.resolve_permission_scoped(id, decision, reason, None)
    }
    pub fn resolve_permission_scoped(
        &mut self,
        id: &str,
        decision: PermissionDecision,
        reason: Option<String>,
        scope: Option<crate::perm::PermissionGrantScope>,
    ) -> Result<(), CoordinatorError> {
        let pending = self
            .pending
            .get(id)
            .ok_or_else(|| CoordinatorError::UnknownPermission(id.into()))?;
        let answers = match &pending.work {
            PendingWork::Question { request, .. } if decision == PermissionDecision::Allow => {
                Some(request.answers(reason.as_deref())?)
            }
            _ => None,
        };
        let job = self
            .running
            .get(&pending.id)
            .ok_or_else(|| CoordinatorError::UnknownTask(pending.id.clone()))?;
        let actor = job.actor.clone();
        let task = pending.id.clone();
        let cancelled = job.cancellation.is_cancelled();
        let hook_error = self.hook(crate::config::HookLifecycleEvent::PermissionResolved, &actor, Some(&task),
            serde_json::json!({"permission_id":id,"outcome":if decision == PermissionDecision::Allow {"allow"} else {"deny"},"failure_reason":reason})).err();
        let allowed = decision == PermissionDecision::Allow && !cancelled && hook_error.is_none();
        let (decision, reason) = if let Some(error) = hook_error {
            (PermissionDecision::Deny, Some(error.to_string()))
        } else {
            (decision, reason)
        };
        if let Some(scope) = scope.filter(|_| allowed) {
            self.record_grant(id, scope)?;
        }
        self.emit(
            actor,
            Some(task),
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: id.into(),
                decision,
                reason,
            }),
        )?;
        if let Some(pending) = self.pending.remove(id) {
            let denied =
                || CoordinatorError::PermissionDenied("approval declined or expired".into());
            match pending.work {
                PendingWork::Tool(work) if allowed => self.start_tool(*work)?,
                PendingWork::Tool(_) => self.finished(Completion::Tool {
                    id: pending.id,
                    result: Err(denied()),
                })?,
                PendingWork::Question { reply, .. } => {
                    let _ = reply.send(answers.filter(|_| allowed).ok_or_else(denied));
                }
                PendingWork::EditPaths {
                    paths,
                    editing,
                    reply,
                    ..
                } => {
                    let result = if allowed {
                        self.approve_edit_paths(&pending.id, paths, editing)
                    } else {
                        Err(denied())
                    };
                    let _ = reply.send(result);
                }
            }
        }
        Ok(())
    }
    pub fn expire_permissions(&mut self) {
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| p.deadline.is_some_and(|d| d <= Instant::now()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            let _ = self.resolve_permission(
                &id,
                PermissionDecision::Deny,
                Some("approval timed out".into()),
            );
        }
    }
    pub fn record_tool_result(
        &mut self,
        id: &str,
        actor: &EventActor,
        parent: Option<&str>,
        result: &Result<ToolResult, CoordinatorError>,
        hook_executions: Vec<HookExecutionMetadata>,
    ) -> Result<(), CoordinatorError> {
        let (status, text, json, artifacts) = match result {
            Ok(result) => (
                if result.is_error() {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Succeeded
                },
                result.display_text.clone(),
                result.structured_json.clone(),
                result
                    .artifacts
                    .iter()
                    .map(|a| EventArtifactRef {
                        path: a.path.clone(),
                        digest: Some(a.digest.clone()),
                    })
                    .collect(),
            ),
            Err(error) => (ToolCallStatus::Failed, error.to_string(), None, Vec::new()),
        };
        self.emit(
            actor.clone(),
            Some(id.into()),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: id.into(),
                status,
                output_digest: Some(digest(&text)),
                output_summary: Some(text),
                output_json: json,
                metadata: Some(ToolCallMetadata {
                    artifact_refs: artifacts,
                    hook_executions,
                    attachments: result
                        .as_ref()
                        .map(|r| r.attachments.clone())
                        .unwrap_or_default(),
                    lineage: self.tool_lineage(parent),
                    ..Default::default()
                }),
            }),
        )?;
        Ok(())
    }
}
