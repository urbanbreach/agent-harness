use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::{
    ActorKind, BackgroundTaskNotificationStatus, EventActor, EventEnvelopeV1, EventV1,
    TaskCancelledEvent, TaskCompletedEvent, TaskScheduleState, TaskTerminalScope, ToolCallStatus,
};
use crate::text::non_empty_trimmed;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRequestRef {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BackgroundToolCallCounts {
    pub requested: u64,
    pub succeeded: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRequestProjection {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler_task_id: Option<String>,
    pub status: String,
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<String>,
    pub tool_calls: BackgroundToolCallCounts,
    pub late_result: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackgroundRequestProjectionError {
    #[error("provide request_id, task_id, or session_id returned by a background task call")]
    MissingSelector,
    #[error("background request is not in the caller's task lineage")]
    Unauthorized,
    #[error("could not resolve background request `{0}`")]
    UnknownRequest(String),
    #[error("could not resolve background request for task_id/session_id `{0}`; pass the request_id returned by task(run_in_background=true)")]
    UnknownSelector(String),
    #[error("background request `{0}` has no projected events")]
    MissingProjection(String),
}

pub fn resolve_background_request_ref<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    actor: &EventActor,
    request_id: Option<&str>,
    selector_hint: Option<&str>,
) -> Result<BackgroundRequestRef, BackgroundRequestProjectionError> {
    let explicit_request_id = request_id.and_then(non_empty_trimmed);
    let selector_hint = selector_hint.and_then(non_empty_trimmed);

    if explicit_request_id.is_none() && selector_hint.is_none() {
        return Err(BackgroundRequestProjectionError::MissingSelector);
    }

    let mut latest_request_id = None;
    let mut parent_by_agent = BTreeMap::new();
    let mut saw_matching_unauthorized = false;
    let mut saw_explicit_request = false;

    for event in events {
        match &event.payload {
            EventV1::AgentSpawned(data) => {
                if let Some(parent_agent_id) = data.parent_agent_id.as_deref() {
                    parent_by_agent.insert(data.agent_id.clone(), parent_agent_id.to_string());
                }
            }
            EventV1::TaskScheduled(data) => {
                let event_request_id = event.correlation_id.as_deref();
                let matches_explicit_request = explicit_request_id == event_request_id;
                let matches_session = explicit_request_id.is_none()
                    && selector_hint.is_some_and(|selector| {
                        event.actor.agent_id.as_deref() == Some(selector)
                            || data.task_id == selector
                    });
                if !matches_explicit_request && !matches_session {
                    continue;
                }
                if matches_explicit_request {
                    saw_explicit_request = true;
                }
                if background_request_authorized(
                    actor,
                    &parent_by_agent,
                    event.actor.agent_id.as_deref(),
                ) {
                    latest_request_id = event.correlation_id.clone();
                } else {
                    saw_matching_unauthorized = true;
                }
            }
            _ => {}
        }
    }

    let request_id = match latest_request_id {
        Some(request_id) => request_id,
        None if saw_matching_unauthorized => {
            return Err(BackgroundRequestProjectionError::Unauthorized);
        }
        None if explicit_request_id.is_some() && !saw_explicit_request => {
            return Err(BackgroundRequestProjectionError::UnknownRequest(
                explicit_request_id
                    .expect("explicit request id checked")
                    .to_string(),
            ));
        }
        None => {
            return Err(BackgroundRequestProjectionError::UnknownSelector(
                selector_hint.expect("selector checked").to_string(),
            ));
        }
    };

    Ok(BackgroundRequestRef {
        request_id,
        session_id_hint: selector_hint.map(str::to_string),
    })
}

pub fn project_background_request<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    request_ref: &BackgroundRequestRef,
) -> Result<BackgroundRequestProjection, BackgroundRequestProjectionError> {
    let mut tool_calls = BackgroundToolCallCounts::default();
    let mut started_mono_ms = None;
    let mut session_id = request_ref.session_id_hint.clone();
    let mut scheduler_task_id = None;
    let mut latest_scheduled_state = None;
    let mut result_summary = None;
    let mut failure_summary = None;
    let mut duration_ms = None;
    let mut terminal_status = None;
    let mut late_result = false;
    let mut saw_event = false;

    for event in events {
        let matches_notification = matches!(
            &event.payload,
            EventV1::BackgroundTaskNotification(data)
                if data.child_request_id == request_ref.request_id
        );
        if event.correlation_id.as_deref() != Some(request_ref.request_id.as_str())
            && !matches_notification
        {
            continue;
        }
        saw_event = true;

        match &event.payload {
            EventV1::TaskScheduled(data) => {
                if data
                    .queue_key
                    .as_deref()
                    .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
                {
                    latest_scheduled_state = Some(data.state);
                    scheduler_task_id = Some(data.task_id.clone());
                    if let Some(agent_id) = event.actor.agent_id.as_ref() {
                        session_id = Some(agent_id.clone());
                    }
                    if data.state == TaskScheduleState::Started {
                        started_mono_ms = Some(event.mono_ms);
                    }
                }
            }
            EventV1::ToolCallRequested(_) => {
                tool_calls.requested += 1;
            }
            EventV1::ToolCallFinished(data) => match data.status {
                ToolCallStatus::Succeeded => {
                    tool_calls.succeeded += 1;
                }
                ToolCallStatus::Failed => {
                    tool_calls.failed += 1;
                }
            },
            EventV1::TaskCompleted(data) => {
                if is_background_agent_turn_completion(data, scheduler_task_id.as_deref()) {
                    terminal_status = Some("completed".to_string());
                    result_summary = Some(data.result_summary.clone());
                    duration_ms = data
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.timing.as_ref())
                        .and_then(|timing| timing.elapsed_ms)
                        .or_else(|| elapsed_ms_from_events(started_mono_ms, event.mono_ms));
                }
            }
            EventV1::TaskCancelled(data) => {
                if is_background_agent_turn_cancellation(data, scheduler_task_id.as_deref()) {
                    terminal_status = Some("cancelled".to_string());
                    failure_summary = Some(data.reason.clone());
                }
            }
            EventV1::TaskResultLate(_) => {
                late_result = true;
            }
            EventV1::BackgroundTaskNotification(data) => {
                terminal_status = Some(data.status.as_str().to_string());
                session_id = Some(data.child_session_id.clone());
                scheduler_task_id = Some(data.task_id.clone());
                match data.status {
                    BackgroundTaskNotificationStatus::Completed => {
                        result_summary = Some(data.summary.clone());
                    }
                    BackgroundTaskNotificationStatus::Cancelled
                    | BackgroundTaskNotificationStatus::Failed
                    | BackgroundTaskNotificationStatus::TimedOut => {
                        failure_summary = Some(data.summary.clone());
                    }
                }
            }
            _ => {}
        }
    }

    if !saw_event {
        return Err(BackgroundRequestProjectionError::MissingProjection(
            request_ref.request_id.clone(),
        ));
    }

    let status = terminal_status.unwrap_or_else(|| match latest_scheduled_state {
        Some(TaskScheduleState::Started) => "running".to_string(),
        Some(TaskScheduleState::Queued) => "queued".to_string(),
        None => "scheduled".to_string(),
    });
    let cancel_reason = (status == "cancelled")
        .then(|| failure_summary.clone())
        .flatten();

    Ok(BackgroundRequestProjection {
        request_id: request_ref.request_id.clone(),
        session_id,
        scheduler_task_id,
        terminal: matches!(
            status.as_str(),
            "completed" | "cancelled" | "failed" | "timed_out"
        ),
        duration_ms,
        result_summary,
        failure_summary,
        tool_calls,
        late_result,
        cancel_reason,
        status,
    })
}

fn background_request_authorized(
    actor: &EventActor,
    parent_by_agent: &BTreeMap<String, String>,
    request_agent_id: Option<&str>,
) -> bool {
    if actor.kind != ActorKind::Worker {
        return true;
    }
    let Some(caller_agent_id) = actor.agent_id.as_deref() else {
        return false;
    };
    let Some(mut candidate_agent_id) = request_agent_id else {
        return false;
    };

    if candidate_agent_id == caller_agent_id {
        return true;
    }

    let mut seen = BTreeSet::new();
    while seen.insert(candidate_agent_id.to_string()) {
        let Some(parent_agent_id) = parent_by_agent.get(candidate_agent_id) else {
            return false;
        };
        if parent_agent_id == caller_agent_id {
            return true;
        }
        candidate_agent_id = parent_agent_id;
    }

    false
}

fn is_background_agent_turn_completion(
    event: &TaskCompletedEvent,
    scheduler_task_id: Option<&str>,
) -> bool {
    event
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
        == Some(TaskTerminalScope::AgentTurn)
        || scheduler_task_id == Some(event.task_id.as_str())
}

fn is_background_agent_turn_cancellation(
    event: &TaskCancelledEvent,
    scheduler_task_id: Option<&str>,
) -> bool {
    event.task_scope == Some(TaskTerminalScope::AgentTurn)
        || scheduler_task_id == Some(event.task_id.as_str())
}

fn elapsed_ms_from_events(started_mono_ms: Option<u64>, finished_mono_ms: u64) -> Option<u64> {
    started_mono_ms.map(|started| finished_mono_ms.saturating_sub(started))
}
