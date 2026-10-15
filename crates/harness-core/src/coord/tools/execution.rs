//! Tool execution and durable result recording.
use super::*;
use crate::tool::ToolCapability;

impl Runtime {
    pub(in crate::coord) fn launch_tool(&mut self, work: ToolWork) {
        let semaphore = Arc::clone(&self.tools);
        let id = work.id.clone();
        let task = self.jobs.spawn(async move {
            let token = work.context.cancellation.clone();
            let handle = work.context.coordinator.clone();
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
            let instructions = if result.as_ref().is_ok_and(|output| !output.is_error()) {
                let id = work.id.clone();
                let discovery = handle.call(move |s| {
                    Ok(s.running.get(&id).and_then(|job| s.directory_instruction_discovery(job)))
                }).await.ok().flatten();
                if let Some(discovery) = discovery {
                    tokio::select! {
                        biased;
                        () = token.cancelled() => Vec::new(),
                        instructions = tokio::task::spawn_blocking(move || discovery.read()) => instructions.unwrap_or_default(),
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            Completion::Tool { id: work.id, result, instructions }
        });
        if let Some(job) = self.running.get_mut(&id) {
            job.join_id = Some(task.id());
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
                    useless: result.as_ref().is_ok_and(|r| r.useless && !r.is_error()),
                    source_artifact: result.as_ref().is_ok_and(|r| r.source_artifact),
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
