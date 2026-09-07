use harness_core::event::{
    BackgroundTaskNotificationStatus, EventEnvelopeV1, EventV1, TaskScheduleState,
    TaskScheduledEvent,
};
use harness_core::proj::RunStatus;
use harness_core::session::canonical_provider_fragment_payload;

use super::model::DashboardStatus;

pub(super) fn derive_status(
    catalog_status: Option<RunStatus>,
    events: &[&EventEnvelopeV1],
) -> DashboardStatus {
    let mut pending = std::collections::BTreeSet::new();
    let mut status = match catalog_status {
        Some(RunStatus::Running) => DashboardStatus::Running,
        Some(RunStatus::Finished) => DashboardStatus::Completed,
        Some(RunStatus::Failed) => DashboardStatus::Failed,
        None => DashboardStatus::Stale,
    };
    for event in events {
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
