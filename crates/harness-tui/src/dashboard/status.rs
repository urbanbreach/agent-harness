use std::collections::{BTreeMap, BTreeSet};

use harness_core::event::{
    ActorKind, BackgroundTaskNotificationStatus, EventEnvelopeV1, EventV1, TaskScheduleState,
    TaskScheduledEvent, TaskTerminalScope,
};
use harness_core::proj::RunStatus;
use harness_core::session::canonical_provider_fragment_payload;

use super::model::DashboardStatus;

pub(super) fn derive_status(
    run_id: &str,
    catalog_status: Option<RunStatus>,
    events: &[&EventEnvelopeV1],
) -> DashboardStatus {
    // Agent IDs are independent of run IDs. Materialized child journals declare
    // their own root agent, while the shared parent journal records child parents.
    let agents = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::AgentSpawned(agent) => {
                Some((agent.agent_id.as_str(), agent.parent_agent_id.is_none()))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let owns_agent = |agent: &str| agents.get(agent).copied().unwrap_or(agents.is_empty());
    let owns_actor = |event: &EventEnvelopeV1| {
        let agent = event
            .stream_key
            .as_deref()
            .and_then(|key| key.strip_prefix("agent:"))
            .or_else(|| {
                matches!(event.actor.kind, ActorKind::Worker | ActorKind::Supervisor)
                    .then_some(event.actor.agent_id.as_deref())
                    .flatten()
            });
        agent.is_none_or(owns_agent)
    };
    let mut requests = BTreeMap::new();
    let mut tasks = BTreeMap::new();
    let mut pending = BTreeSet::new();
    let mut status = match catalog_status {
        Some(RunStatus::Running) => DashboardStatus::Running,
        Some(RunStatus::Finished) => DashboardStatus::Completed,
        Some(RunStatus::Failed) => DashboardStatus::Failed,
        None => DashboardStatus::Stale,
    };
    for event in events {
        if event.run_id.as_str() != run_id {
            continue;
        }
        let owned = match &event.payload {
            EventV1::BackgroundTaskNotification(notification) => {
                notification.child_session_id.as_str() == run_id
            }
            EventV1::AgentStopped(agent) => owns_agent(&agent.agent_id),
            EventV1::TaskScheduled(task) => {
                let owned = owns_actor(event)
                    && task
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.lineage.as_ref())
                        .and_then(|lineage| lineage.child_session_id.as_deref())
                        .is_none_or(|child| child == run_id);
                tasks.insert(task.task_id.as_str(), owned);
                if let Some(request) = event.correlation_id.as_deref() {
                    requests.insert(request, owned);
                }
                owned
            }
            EventV1::TaskCompleted(task) => {
                owns_actor(event)
                    && tasks.get(task.task_id.as_str()).copied().unwrap_or(true)
                    && task
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.lineage.as_ref())
                        .and_then(|lineage| lineage.child_session_id.as_deref())
                        .is_none_or(|child| child == run_id)
            }
            EventV1::TaskCancelled(task) => {
                owns_actor(event) && tasks.get(task.task_id.as_str()).copied().unwrap_or(true)
            }
            EventV1::StaleDetected(task) => {
                owns_actor(event) && tasks.get(task.task_id.as_str()).copied().unwrap_or(true)
            }
            EventV1::ProviderRequestStarted(request) => {
                let owned = owns_actor(event)
                    && event
                        .correlation_id
                        .as_deref()
                        .and_then(|request| requests.get(request))
                        .copied()
                        .unwrap_or(true);
                requests.insert(request.request_id.as_str(), owned);
                owned
            }
            payload if canonical_provider_fragment_payload(payload).is_some() => {
                canonical_provider_fragment_payload(payload)
                    .and_then(|fragment| requests.get(fragment.request_id))
                    .copied()
                    .unwrap_or_else(|| owns_actor(event))
            }
            EventV1::RunStarted(_) | EventV1::RunFinished(_) | EventV1::RunFailed(_) => true,
            _ => {
                owns_actor(event)
                    && event
                        .correlation_id
                        .as_deref()
                        .and_then(|request| requests.get(request))
                        .copied()
                        .unwrap_or(true)
            }
        };
        if !owned {
            continue;
        }
        match &event.payload {
            EventV1::PermissionRequested(permission) => {
                pending.insert(permission.permission_id.clone());
            }
            EventV1::PermissionResolved(permission) => {
                pending.remove(&permission.permission_id);
            }
            _ => {}
        }
        status = match &event.payload {
            EventV1::RunStarted(_) if status == DashboardStatus::Completed => status,
            EventV1::RunStarted(_) => DashboardStatus::Running,
            EventV1::TaskScheduled(TaskScheduledEvent {
                state: TaskScheduleState::Queued,
                ..
            }) => DashboardStatus::Queued,
            EventV1::TaskScheduled(TaskScheduledEvent {
                state: TaskScheduleState::Started,
                ..
            }) => DashboardStatus::Running,
            EventV1::ProviderRequestStarted(_) => DashboardStatus::Streaming,
            payload if canonical_provider_fragment_payload(payload).is_some() => {
                DashboardStatus::Streaming
            }
            EventV1::TaskCompleted(data)
                if data
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_scope)
                    == Some(TaskTerminalScope::AgentTurn) =>
            {
                DashboardStatus::Completed
            }
            EventV1::RunFinished(_) => DashboardStatus::Completed,
            EventV1::RunFailed(_) => DashboardStatus::Failed,
            EventV1::TaskCancelled(_) | EventV1::AgentStopped(_) => DashboardStatus::Cancelled,
            EventV1::StaleDetected(_) => DashboardStatus::Stale,
            EventV1::BackgroundTaskNotification(notification) => match notification.status {
                BackgroundTaskNotificationStatus::Completed => DashboardStatus::Completed,
                BackgroundTaskNotificationStatus::Cancelled => DashboardStatus::Cancelled,
                BackgroundTaskNotificationStatus::Failed => DashboardStatus::Failed,
                BackgroundTaskNotificationStatus::TimedOut => DashboardStatus::Stale,
            },
            _ => status,
        };
    }
    if !pending.is_empty()
        && matches!(
            status,
            DashboardStatus::Running | DashboardStatus::Streaming | DashboardStatus::Queued
        )
    {
        DashboardStatus::AwaitingInput
    } else {
        status
    }
}
