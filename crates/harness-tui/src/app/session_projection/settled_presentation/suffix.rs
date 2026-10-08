use super::*;

impl SessionProjection {
    /// Reuse only a closed prefix. Other histories use the same converter from zero.
    pub(in crate::app::session_projection) fn settled_suffix_start(&self) -> Option<usize> {
        // ponytail: complex turns still convert in full. Widen reuse only after
        // their cross-turn presentation dependencies have behavioral coverage.
        if self.canonical_projection_error.is_some()
            || self.events_trimmed_count > 0
            || self.transcript_trimmed_count > 0
            || !self.pending_permissions.is_empty()
            || !self.transient_assistants.is_empty()
        {
            return None;
        }
        let canonical = self.canonical_projection.as_ref()?;
        if canonical.run_summary.status != harness_core::proj::RunStatus::Running {
            return None;
        }
        let events = canonical.source_events();
        let last_seq = events.last()?.seq;
        let from = self.activities.iter().rposition(|activity| {
            !activity.request_id.is_empty() && activity.first_seq <= last_seq
        })?;
        if from == 0 {
            return None;
        }
        let cutoff = self.activities[from].first_seq;
        let requests = self
            .activities
            .iter()
            .skip(from)
            .filter(|activity| !activity.request_id.is_empty())
            .map(|activity| activity.request_id.as_str())
            .collect::<BTreeSet<_>>();
        if self.activities.iter().take(from).any(|activity| {
            activity.request_id.is_empty()
                || activity.first_seq >= cutoff
                || requests.contains(activity.request_id.as_str())
        }) {
            return None;
        }
        let messages = &canonical.transcript.messages;
        let at = messages.partition_point(|message| message.provenance.first_seq < cutoff);
        if messages[..at].iter().any(|message| {
            message
                .request_id
                .as_ref()
                .is_some_and(|id| requests.contains(id.as_str()))
        }) || messages[at..].iter().any(|message| {
            !message
                .request_id
                .as_ref()
                .is_some_and(|id| requests.contains(id.as_str()))
                || message.parts.iter().any(|part| match part {
                    ProjectedPart::Text(_) | ProjectedPart::Reasoning(_) => false,
                    ProjectedPart::Task(task) => task.lineage.is_some(),
                    _ => true,
                })
        }) {
            return None;
        }
        // Reopening a retained terminal task can restore the previously sixth
        // terminal row. The prefix display map no longer contains that row.
        if self.unsettled_durable_events.iter().any(|event| {
            let EventV1::TaskScheduled(scheduled) = &event.payload else {
                return false;
            };
            messages[at..].iter().flat_map(|message| &message.parts).any(|part| {
                matches!(part, ProjectedPart::Task(task)
                    if task.task_id == scheduled.task_id
                        && !matches!(task.state, ProjectedTaskState::Queued | ProjectedTaskState::Started))
            })
        }) {
            return None;
        }

        let at = events.partition_point(|event| event.seq < cutoff);
        let mut providers = BTreeSet::new();
        let mut tasks = BTreeSet::new();
        // Historical suffix events matter too: some presentation events have no
        // canonical message, and their effects can depend on the earlier prefix.
        for event in events[at..].iter().chain(&self.unsettled_durable_events) {
            let (request, provider, task) = suffix_event_owner(event)?;
            if !requests.contains(request) {
                return None;
            }
            providers.extend(provider);
            tasks.extend(task);
        }
        if self.orchestration_tasks.values().any(|task| {
            task.first_seq < cutoff
                && [task.request_id.as_deref(), task.child_request_id.as_deref()]
                    .into_iter()
                    .flatten()
                    .any(|id| requests.contains(id) || providers.contains(id))
        }) {
            return None;
        }
        if self.activities.iter().take(from).any(|activity| {
            providers.contains(activity.request_id.as_str())
                || activity.request_data.as_ref().is_some_and(|provider| {
                    requests.contains(provider.request_id.as_str())
                        || providers.contains(provider.request_id.as_str())
                })
        }) {
            return None;
        }
        // Provider IDs can alias an older turn even with a new correlation ID.
        // Completed tasks may already have left the five-row display retention.
        for event in &events[..at] {
            let (provider, task) = match &event.payload {
                EventV1::ConversationRewound(_) => return None,
                EventV1::ProviderRequestStarted(data) => (Some(data.request_id.as_str()), None),
                EventV1::ProviderRequestFinished(data) => (Some(data.request_id.as_str()), None),
                EventV1::AssistantMessageFinished(data) => (Some(data.request_id.as_str()), None),
                EventV1::ProviderStreamDelta(data) | EventV1::ProviderReasoningDelta(data) => {
                    (Some(data.request_id.as_str()), None)
                }
                EventV1::TaskScheduled(data) => (None, Some(data.task_id.as_str())),
                EventV1::TaskCompleted(data) => (None, Some(data.task_id.as_str())),
                EventV1::TaskCancelled(data) => (None, Some(data.task_id.as_str())),
                EventV1::TaskResultLate(data) => (None, Some(data.task_id.as_str())),
                EventV1::BackgroundTaskNotification(data) => {
                    let request = data
                        .delivered_turn_request_id
                        .as_deref()
                        .unwrap_or(data.child_request_id.as_str());
                    if requests.contains(request) || providers.contains(request) {
                        return None;
                    }
                    (None, Some(data.task_id.as_str()))
                }
                EventV1::StaleDetected(data) => (None, Some(data.task_id.as_str())),
                _ => (None, None),
            };
            if provider.is_some_and(|id| providers.contains(id) || requests.contains(id))
                || task.is_some_and(|id| tasks.contains(id))
            {
                return None;
            }
        }
        Some(from)
    }
}

fn suffix_event_owner(event: &EventEnvelopeV1) -> Option<(&str, Option<&str>, Option<&str>)> {
    let provider = match &event.payload {
        EventV1::UserMessageSubmitted(data) => {
            return Some((
                event
                    .correlation_id
                    .as_deref()
                    .unwrap_or(data.request_id.as_str()),
                None,
                None,
            ));
        }
        EventV1::PromptAttachmentsSubmitted(data) => {
            return Some((data.request_id.as_str(), None, None));
        }
        EventV1::ProviderRequestStarted(data) => data.request_id.as_str(),
        EventV1::ProviderRequestFinished(data) => data.request_id.as_str(),
        EventV1::AssistantMessageFinished(data)
            if data.parts.iter().all(|part| {
                matches!(
                    part,
                    AssistantPart::Text { .. } | AssistantPart::Reasoning { .. }
                )
            }) =>
        {
            data.request_id.as_str()
        }
        EventV1::TaskScheduled(data)
            if data
                .metadata
                .as_ref()
                .is_none_or(|metadata| metadata.lineage.is_none()) =>
        {
            return Some((
                event.correlation_id.as_deref()?,
                None,
                Some(data.task_id.as_str()),
            ));
        }
        EventV1::TaskCompleted(data)
            if data
                .metadata
                .as_ref()
                .is_none_or(|metadata| metadata.lineage.is_none()) =>
        {
            return Some((
                event.correlation_id.as_deref()?,
                None,
                Some(data.task_id.as_str()),
            ));
        }
        _ => return None,
    };
    Some((
        event.correlation_id.as_deref().unwrap_or(provider),
        Some(provider),
        None,
    ))
}
