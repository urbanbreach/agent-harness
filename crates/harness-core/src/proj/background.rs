use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRequestRef {
    pub request_id: crate::ids::RequestId,
    pub session_id_hint: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundToolCallCounts {
    pub requested: u64,
    pub succeeded: u64,
    pub failed: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRequestProjection {
    pub request_id: crate::ids::RequestId,
    pub session_id: Option<String>,
    pub scheduler_task_id: Option<String>,
    pub status: String,
    pub terminal: bool,
    pub duration_ms: Option<u64>,
    pub result_summary: Option<String>,
    pub failure_summary: Option<String>,
    pub tool_calls: BackgroundToolCallCounts,
    pub late_result: bool,
    pub cancel_reason: Option<String>,
}
#[derive(Debug, thiserror::Error)]
pub enum BackgroundRequestProjectionError {
    #[error("provide a request, task, or session ID")]
    MissingSelector,
    #[error("background request is not in the caller's task lineage")]
    Unauthorized,
    #[error("unknown background request: {0}")]
    UnknownRequest(String),
    #[error("unknown background task or session: {0}")]
    UnknownSelector(String),
    #[error("background request has no valid projected history: {0}")]
    MissingProjection(String),
}
pub fn project_background_request<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    reference: &BackgroundRequestRef,
) -> Result<BackgroundRequestProjection, BackgroundRequestProjectionError> {
    let events = checked_history(events).map_err(|_| {
        BackgroundRequestProjectionError::MissingProjection(reference.request_id.to_string())
    })?;
    let mut output = BackgroundRequestProjection {
        request_id: reference.request_id.clone(),
        session_id: reference.session_id_hint.clone(),
        scheduler_task_id: None,
        status: "queued".into(),
        terminal: false,
        duration_ms: None,
        result_summary: None,
        failure_summary: None,
        tool_calls: Default::default(),
        late_result: false,
        cancel_reason: None,
    };
    let (mut tools, mut finished_tools) = (BTreeSet::new(), BTreeSet::new());
    let (mut started, mut found) = (None, false);
    for event in crate::conversation_rewind::active_refs(events) {
        let metadata = lineage(&event.payload);
        let owns_event = event.correlation_id.as_deref() == Some(reference.request_id.as_str());
        let related = owns_event
            || metadata.and_then(|m| m.child_request_id.as_deref())
                == Some(reference.request_id.as_str());
        // A launch names a child; only that child's activity establishes its state.
        found |= owns_event;
        if let Some(session) = metadata
            .filter(|_| related)
            .and_then(|m| m.child_session_id.as_ref())
        {
            output.session_id = Some(session.clone());
        }
        match &event.payload {
            EventV1::TaskScheduled(e) if related => {
                found = true;
                output.scheduler_task_id = Some(e.task_id.to_string());
                if e.state == TaskScheduleState::Started && !output.terminal {
                    started.get_or_insert(event.mono_ms);
                    output.status = "running".into();
                }
            }
            EventV1::ProviderRequestStarted(e)
                if related
                    || e.metadata.as_ref().and_then(|m| m.turn_id.as_deref())
                        == Some(reference.request_id.as_str())
                    || e.request_id == reference.request_id =>
            {
                found = true;
                if !output.terminal {
                    started.get_or_insert(event.mono_ms);
                    output.status = "running".into();
                }
            }
            EventV1::ToolCallRequested(e)
                if related
                    && metadata.and_then(|m| m.parent_tool_call_id.as_deref())
                        != Some(e.tool_call_id.as_str()) =>
            {
                if tools.insert(e.tool_call_id.to_string()) {
                    output.tool_calls.requested += 1;
                }
            }
            EventV1::ToolCallFinished(e)
                if tools.contains(e.tool_call_id.as_str())
                    && finished_tools.insert(e.tool_call_id.to_string()) =>
            {
                match e.status {
                    ToolCallStatus::Succeeded => output.tool_calls.succeeded += 1,
                    ToolCallStatus::Failed => output.tool_calls.failed += 1,
                }
            }
            EventV1::TaskCompleted(e)
                if (related || output.scheduler_task_id.as_deref() == Some(e.task_id.as_str()))
                    && e.metadata.as_ref().and_then(|m| m.task_scope)
                        != Some(TaskTerminalScope::ToolCall)
                    && !tools.contains(e.task_id.as_str()) =>
            {
                found = true;
                output.result_summary = Some(e.result_summary.clone());
                output.finish("completed", started, event.mono_ms);
            }
            EventV1::TaskCancelled(e)
                if (related || output.scheduler_task_id.as_deref() == Some(e.task_id.as_str()))
                    && e.task_scope != Some(TaskTerminalScope::ToolCall)
                    && !tools.contains(e.task_id.as_str()) =>
            {
                output.cancel_reason = Some(e.reason.clone());
                if e.failure {
                    output.failure_summary = Some(e.reason.clone());
                }
                output.finish(
                    if e.failure { "failed" } else { "cancelled" },
                    started,
                    event.mono_ms,
                );
            }
            EventV1::TaskResultLate(e)
                if related || output.scheduler_task_id.as_deref() == Some(e.task_id.as_str()) =>
            {
                output.late_result = true;
                output.terminal = true;
                output.status = "late_result".into();
            }
            EventV1::BackgroundTaskNotification(e)
                if e.child_request_id == reference.request_id.as_str() =>
            {
                found = true;
                output.notification(e, started, event.mono_ms);
            }
            _ => {}
        }
    }
    if found {
        Ok(output)
    } else {
        Err(BackgroundRequestProjectionError::MissingProjection(
            reference.request_id.to_string(),
        ))
    }
}
impl BackgroundRequestProjection {
    fn notification(
        &mut self,
        event: &BackgroundTaskNotificationEvent,
        started: Option<u64>,
        time: u64,
    ) {
        self.session_id = Some(event.child_session_id.to_string());
        if !self.terminal {
            if event.status == BackgroundTaskNotificationStatus::Completed {
                self.result_summary = Some(event.summary.clone());
            } else {
                self.failure_summary = Some(event.summary.clone());
            }
            self.finish(event.status.as_str(), started, time);
        }
    }
    fn finish(&mut self, status: &str, started: Option<u64>, end: u64) {
        if self.terminal {
            self.late_result = true;
            self.status = "late_result".into();
        } else {
            self.status = status.into();
            self.terminal = true;
            self.duration_ms = started.map(|start| end.saturating_sub(start));
        }
    }
}
pub(super) fn lineage(payload: &EventV1) -> Option<&TaskLineageMetadata> {
    match payload {
        EventV1::TaskScheduled(e) => e.metadata.as_ref().and_then(|m| m.lineage.as_ref()),
        EventV1::TaskCompleted(e) => e.metadata.as_ref().and_then(|m| m.lineage.as_ref()),
        EventV1::ToolCallRequested(e) => e.metadata.as_ref().and_then(|m| m.lineage.as_ref()),
        EventV1::ToolCallFinished(e) => e.metadata.as_ref().and_then(|m| m.lineage.as_ref()),
        _ => None,
    }
}

struct Candidate<'a> {
    reference: BackgroundRequestRef,
    owner: Option<&'a str>,
    task: Option<&'a str>,
}
fn candidates<'a>(events: &[&'a EventEnvelopeV1]) -> BTreeMap<&'a str, Candidate<'a>> {
    let owners: BTreeMap<_, _> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ToolCallRequested(e) => {
                Some((e.tool_call_id.as_str(), event.actor.agent_id.as_deref()))
            }
            _ => None,
        })
        .collect();
    let mut candidates = BTreeMap::new();
    for event in events {
        let Some(lineage) = lineage(&event.payload) else {
            continue;
        };
        let Some(request) = lineage
            .child_request_id
            .as_deref()
            .or(event.correlation_id.as_deref())
        else {
            continue;
        };
        if lineage.child_session_id.is_none() || lineage.parent_tool_call_id.is_none() {
            continue;
        }
        let owner = lineage
            .parent_tool_call_id
            .as_deref()
            .and_then(|id| owners.get(id).copied())
            .flatten();
        let candidate = candidates.entry(request).or_insert_with(|| Candidate {
            reference: BackgroundRequestRef {
                request_id: request.into(),
                session_id_hint: lineage.child_session_id.clone(),
            },
            owner,
            task: None,
        });
        if let EventV1::TaskScheduled(e) = &event.payload {
            candidate.task = Some(e.task_id.as_str());
        }
    }
    candidates
}
fn parents<'a>(events: &[&'a EventEnvelopeV1]) -> BTreeMap<&'a str, Option<&'a str>> {
    events
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::AgentSpawned(a) => Some((a.agent_id.as_str(), a.parent_agent_id.as_deref())),
            _ => None,
        })
        .collect()
}
fn authorized(
    parents: &BTreeMap<&str, Option<&str>>,
    actor: &EventActor,
    owner: Option<&str>,
) -> bool {
    if matches!(
        actor.kind,
        ActorKind::System | ActorKind::User | ActorKind::Supervisor
    ) {
        return true;
    }
    let Some(caller) = actor.agent_id.as_deref() else {
        return false;
    };
    let mut current = owner;
    let mut visited = BTreeSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            return false;
        }
        if id == caller {
            return true;
        }
        current = parents.get(id).copied().flatten();
    }
    false
}
pub fn resolve_background_request_ref<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    actor: &EventActor,
    request_id: Option<&str>,
    selector_hint: Option<&str>,
) -> Result<BackgroundRequestRef, BackgroundRequestProjectionError> {
    if request_id.is_none() && selector_hint.is_none() {
        return Err(BackgroundRequestProjectionError::MissingSelector);
    }
    let events = checked_history(events).map_err(|_| {
        BackgroundRequestProjectionError::MissingProjection(request_id.unwrap_or_default().into())
    })?;
    let events = crate::conversation_rewind::active_refs(events);
    let candidates = candidates(&events);
    let candidate = candidates
        .values()
        .find(|candidate| {
            request_id.is_none_or(|id| id == candidate.reference.request_id.as_str())
                && selector_hint.is_none_or(|id| {
                    candidate.task == Some(id)
                        || candidate.reference.session_id_hint.as_deref() == Some(id)
                })
        })
        .ok_or_else(|| {
            request_id.map_or_else(
                || {
                    BackgroundRequestProjectionError::UnknownSelector(
                        selector_hint.unwrap_or_default().into(),
                    )
                },
                |id| BackgroundRequestProjectionError::UnknownRequest(id.into()),
            )
        })?;
    if !authorized(&parents(&events), actor, candidate.owner) {
        return Err(BackgroundRequestProjectionError::Unauthorized);
    }
    Ok(candidate.reference.clone())
}
pub fn resolve_all_background_request_refs<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    actor: &EventActor,
) -> Vec<BackgroundRequestRef> {
    let Ok(events) = checked_history(events) else {
        return Vec::new();
    };
    let events = crate::conversation_rewind::active_refs(events);
    let parents = parents(&events);
    candidates(&events)
        .into_values()
        .filter(|candidate| authorized(&parents, actor, candidate.owner))
        .map(|candidate| candidate.reference)
        .collect()
}
