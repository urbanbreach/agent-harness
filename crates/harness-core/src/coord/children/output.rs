use super::*;

impl CoordinatorHandle {
    pub async fn child_task_output(
        &self,
        tool: impl Into<String>,
        selector: impl Into<String>,
    ) -> Result<ToolResult, CoordinatorError> {
        let (tool, selector) = (tool.into(), selector.into());
        self.call(move |s| {
            s.child_output(&tool, &selector, false)
                .map(|(output, _)| output)
        })
        .await
    }
    pub async fn child_task_output_with_history(
        &self,
        tool: impl Into<String>,
        selector: impl Into<String>,
    ) -> Result<(ToolResult, Vec<EventEnvelopeV1>), CoordinatorError> {
        let (tool, selector) = (tool.into(), selector.into());
        self.call(move |s| s.child_output(&tool, &selector, true))
            .await
    }
    pub async fn cancel_child_task(
        &self,
        tool: impl Into<String>,
        selector: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<ToolResult, CoordinatorError> {
        let (tool, selector, reason) = (tool.into(), selector.into(), reason.into());
        self.call(move |s| {
            if reason.len() > 1024 {
                return Err(CoordinatorError::Invalid(
                    "cancellation reason exceeds 1024 bytes".into(),
                ));
            }
            let (output, _) = s.child_output(&tool, &selector, false)?;
            if output
                .structured_json
                .as_ref()
                .is_some_and(|value| value["status"] != "running")
            {
                return Ok(output);
            }
            let session = s.owned_child(&tool, &selector)?;
            let child = &s.children[&session];
            let (request, result) = (
                child.request.clone(),
                child.result(&session, "cancelling", "Child cancellation requested."),
            );
            s.cancel(&request, &reason)?;
            Ok(result)
        })
        .await
    }
    pub async fn owned_child_tasks(
        &self,
        tool: impl Into<String>,
    ) -> Result<Vec<String>, CoordinatorError> {
        let tool = tool.into();
        self.call(move |s| {
            s.check_task(&tool)?;
            let parent = s.running[&tool].actor.agent_id.as_deref().ok_or_else(|| {
                CoordinatorError::PermissionDenied("child inspection requires an agent".into())
            })?;
            Ok(s.children
                .iter()
                .filter(|(_, child)| child.parent_agent == parent)
                .map(|(id, _)| id.clone())
                .collect())
        })
        .await
    }
}
impl Runtime {
    fn child_output(
        &self,
        tool: &str,
        selector: &str,
        history: bool,
    ) -> Result<(ToolResult, Vec<EventEnvelopeV1>), CoordinatorError> {
        self.check_task(tool)?;
        let request = if let Ok(session) = self.owned_child(tool, selector) {
            let child = &self.children[&session];
            if !child.complete && !history {
                return Ok((
                    child.result(&session, "running", "Child task is running."),
                    Vec::new(),
                ));
            }
            child.request.clone()
        } else {
            selector.into()
        };
        // Completed reports stay in the journal, not in every idle child context.
        let path = &self.info()?.events_path;
        let length = crate::store::open_private_file(path)?.metadata()?.len();
        if length > 64 * 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "session exceeds the 64 MiB background inspection limit".into(),
            ));
        }
        let events: Vec<_> =
            crate::store::JournalReader::open(path, length)?.collect::<Result<_, _>>()?;
        let failure = |error: crate::proj::BackgroundRequestProjectionError| {
            CoordinatorError::Invalid(error.to_string())
        };
        let reference = crate::proj::resolve_background_request_ref(
            &events,
            &self.running[tool].actor,
            Some(&request),
            None,
        )
        .map_err(failure)?;
        let session = reference.session_id_hint.as_deref().ok_or_else(|| {
            CoordinatorError::Invalid("child session is missing from its lineage".into())
        })?;
        self.owned_child(tool, session)?;
        let projected =
            crate::proj::project_background_request(&events, &reference).map_err(failure)?;
        let summary = projected
            .result_summary
            .as_deref()
            .or(projected.failure_summary.as_deref())
            .or(projected.cancel_reason.as_deref())
            .unwrap_or(&projected.status);
        let mut result = self.children[session].result(session, &projected.status, summary);
        if let Some(value) = result.structured_json.as_mut() {
            value["request_id"] = request.into();
            value["duration_ms"] = projected.duration_ms.into();
            value["tool_calls"] = serde_json::to_value(projected.tool_calls)?;
            value["late_result"] = projected.late_result.into();
        }
        let history = if history {
            crate::conversation_rewind::active_events(&events).iter()
                .filter(|event| event.actor.agent_id.as_deref() == Some(session)
                    || matches!(&event.payload, EventV1::AgentSpawned(e) if e.agent_id == session))
                .cloned().collect()
        } else {
            Vec::new()
        };
        Ok((result, history))
    }
    fn owned_child(&self, tool: &str, selector: &str) -> Result<String, CoordinatorError> {
        self.check_task(tool)?;
        let parent = self.running[tool]
            .actor
            .agent_id
            .as_deref()
            .ok_or_else(|| {
                CoordinatorError::PermissionDenied("child inspection requires an agent".into())
            })?;
        self.children
            .iter()
            .find(|(id, child)| {
                child.parent_agent == parent
                    && (id.as_str() == selector || child.request == selector)
            })
            .map(|(id, _)| id.clone())
            .ok_or_else(|| {
                CoordinatorError::PermissionDenied("no direct child matches this handle".into())
            })
    }
}
