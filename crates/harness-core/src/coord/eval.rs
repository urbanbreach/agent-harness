use super::{runtime::JobKind, *};
use crate::tool::ToolCapability;
mod completion;

impl CoordinatorHandle {
    /// Publish closures from an approved eval into only the defining agent's
    /// catalog. Children inherit descriptor snapshots when they are admitted.
    pub async fn register_eval_tools(
        &self,
        parent: String,
        tools: Vec<Arc<dyn crate::tool::Tool>>,
    ) -> Result<(), CoordinatorError> {
        self.call(move |s| {
            s.check_eval(&parent)?;
            let agent = s.running[&parent]
                .actor
                .agent_id
                .as_deref()
                .ok_or_else(|| {
                    CoordinatorError::Invalid("kernel tools require an agent session".into())
                })?;
            s.config
                .tool_registry
                .register_scoped(&s.tool_scope(Some(agent)).unwrap_or_default(), tools)
                .map_err(|error| CoordinatorError::Invalid(error.to_string()))
        })
        .await
    }

    /// Return the caller's current catalog without activating tools or performing I/O.
    pub async fn eval_tool_catalog(&self, parent: String) -> Result<Value, CoordinatorError> {
        self.call(move |s| {
            s.check_eval(&parent)?;
            let job = &s.running[&parent];
            let agent = job.actor.agent_id.as_ref().and_then(|id| s.agents.get(id));
            Ok(Value::Array(
                s.config
                    .tool_registry
                    .tool_ids_for(s.tool_scope(job.actor.agent_id.as_deref()).as_deref())
                    .into_iter()
                    .filter_map(|id| {
                        if id == "eval"
                            || agent
                                .is_some_and(|a| !s.config.tool_registry.allows(&a.profile, &id))
                        {
                            return None;
                        }
                        let tool = s
                            .config
                            .tool_registry
                            .get_for(&id, s.tool_scope(job.actor.agent_id.as_deref()).as_deref())?;
                        if tool.permission_requests(&serde_json::json!({})).iter().any(
                            |(name, _)| {
                                s.config.permission_policy.always_denies(name)
                                    || agent.is_some_and(|a| a.policy.always_denies(name))
                            },
                        ) {
                            return None;
                        }
                        Some(describe_tool(tool.as_ref()))
                    })
                    .collect::<Result<Vec<_>, CoordinatorError>>()?,
            ))
        })
        .await
    }

    pub async fn eval_progress(
        &self,
        parent: String,
        output: String,
        details: Value,
    ) -> Result<(), CoordinatorError> {
        if output.len() > 256 * 1024 || details.to_string().len() > 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "eval progress exceeds its limit".into(),
            ));
        }
        self.call(move |s| {
            s.check_eval(&parent)?;
            let actor = s.running[&parent].actor.clone();
            s.live(
                actor,
                parent.clone(),
                LiveEventV1::EvalProgress {
                    tool_call_id: parent.into(),
                    output,
                    details,
                },
            )
        })
        .await
    }

    /// Deliver the foreground receipt while keeping the approved job, children and cancellation owned here.
    pub async fn detach_eval(
        &self,
        parent: String,
        result: ToolResult,
    ) -> Result<(), CoordinatorError> {
        self.call(move |s| {
            s.check_eval(&parent)?;
            if s.detached_evals.contains(&parent) {
                return Ok(());
            }
            let job = &s.running[&parent];
            let (actor, lineage) = (job.actor.clone(), job.parent.clone());
            if let Some(agent) = &actor.agent_id {
                s.check_turn_capacity(agent)?;
            }
            let result = s.bound_tool_output(&parent, &actor, result, true)?;
            s.record_tool_result(
                &parent,
                &actor,
                lineage.as_deref(),
                &Ok(result.clone()),
                Vec::new(),
            )?;
            if let Some(agent) = &actor.agent_id {
                s.raw_tool_results
                    .insert(parent.clone(), (agent.clone(), Ok(result.clone())));
            }
            s.detached_evals.insert(parent.clone());
            if let JobKind::Tool { reply, .. } = &mut s
                .running
                .get_mut(&parent)
                .ok_or(CoordinatorError::UnknownTask(parent))?
                .kind
                && let Some(reply) = reply.take()
            {
                let _ = reply.send(Ok(result));
            }
            Ok(())
        })
        .await
    }

    pub async fn eval_has_steering(&self, parent: String) -> Result<bool, CoordinatorError> {
        self.call(move |s| {
            s.check_eval(&parent)?;
            Ok(s.running[&parent]
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| s.agents.get(id))
                .is_some_and(|a| !a.queue.is_empty()))
        })
        .await
    }

    pub async fn retain_eval_output(
        &self,
        parent: String,
        text: String,
    ) -> Result<crate::tool::ArtifactRef, CoordinatorError> {
        if text.len() > 8 * 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "eval artifact exceeds 8 MiB".into(),
            ));
        }
        self.call(move |s| {
            s.check_eval(&parent)?;
            let actor = s.running[&parent].actor.clone();
            s.write_artifact(&actor, Some(&parent), "txt", &text)
        })
        .await
    }
}

fn describe_tool(tool: &dyn crate::tool::Tool) -> Result<Value, CoordinatorError> {
    let prelude = tool.kernel_prelude();
    if let Some(prelude) = &prelude
        && (prelude.exports.is_empty()
            || prelude.exports.iter().any(|name| {
                name.starts_with("__")
                    || matches!(
                        name.as_str(),
                        "agent"
                            | "completion"
                            | "display"
                            | "env"
                            | "log"
                            | "output"
                            | "parallel"
                            | "phase"
                            | "pipeline"
                            | "print"
                            | "read"
                            | "tool"
                            | "tool_schema"
                            | "tools"
                            | "workpool"
                            | "write"
                    )
            }))
    {
        return Err(CoordinatorError::Invalid(format!(
            "tool {} kernel prelude must export names without shadowing built-in eval helpers",
            tool.id()
        )));
    }
    let description = prelude.as_ref().map_or_else(
        || tool.description().to_owned(),
        |prelude| format!("{}\n{}", tool.description(), prelude.documentation),
    );
    Ok(
        serde_json::json!({"name":tool.id(),"description":description,"parameters":tool.parameters_json_schema(),"kernelPrelude":prelude}),
    )
}

impl Runtime {
    pub(super) fn tool_scope(&self, agent: Option<&str>) -> Option<String> {
        Some(format!("{}:{}", self.info.as_ref()?.run_id, agent?))
    }
    pub(super) fn check_eval(&self, parent: &str) -> Result<(), CoordinatorError> {
        self.check_task(parent)?;
        let job = &self.running[parent];
        if job.join_id.is_none()
            || !matches!(&job.kind, JobKind::Tool { tool_id, capability: ToolCapability::SpawnAgent, .. } if tool_id == "eval" || self.tool_scope(job.actor.agent_id.as_deref()).is_some_and(|scope| self.config.tool_registry.scoped_tools(&scope).iter().any(|tool| tool.id() == tool_id)))
        {
            return Err(CoordinatorError::PermissionDenied(
                "operation requires an approved eval call".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn finish_detached_eval(
        &mut self,
        id: &str,
        actor: &EventActor,
        result: &Result<ToolResult, CoordinatorError>,
    ) -> Result<(), CoordinatorError> {
        let (text, json, success) = match result {
            Ok(result) => (
                result.display_text.clone(),
                result.structured_json.clone(),
                !result.is_error(),
            ),
            Err(error) => (error.to_string(), None, false),
        };
        self.emit(
            actor.clone(),
            Some(id.into()),
            EventV1::EvalCellFinished(ToolCallFinishedEvent {
                tool_call_id: id.into(),
                status: if success {
                    ToolCallStatus::Succeeded
                } else {
                    ToolCallStatus::Failed
                },
                output_digest: Some(runtime::digest(&text)),
                output_summary: Some(text.clone()),
                output_json: json,
                metadata: Some(ToolCallMetadata {
                    attachments: result
                        .as_ref()
                        .map(|r| r.attachments.clone())
                        .unwrap_or_default(),
                    artifact_refs: result
                        .as_ref()
                        .map(|r| {
                            r.artifacts
                                .iter()
                                .map(|a| EventArtifactRef {
                                    path: a.path.clone(),
                                    digest: Some(a.digest.clone()),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    ..Default::default()
                }),
            }),
        )?;
        if self.stopping.is_some() || self.fault.is_some() {
            return Ok(());
        }
        let Some(agent) = actor
            .agent_id
            .as_ref()
            .filter(|id| !self.killed_agents.contains(*id) && !self.stopped_sessions.contains(*id))
        else {
            return Ok(());
        };
        let prompt = super::prompt::Prompt {
            text: format!(
                "Eval cell {id} {}.\n{text}",
                if success {
                    "completed"
                } else {
                    "failed or was cancelled"
                }
            ),
            attachments: result
                .as_ref()
                .map(|r| r.attachments.clone())
                .unwrap_or_default(),
            ..Default::default()
        };
        // Detachment reserves this queue slot until the cell settles.
        self.queue_turn(actor.clone(), agent, prompt, None, None, None)?;
        Ok(())
    }
}
