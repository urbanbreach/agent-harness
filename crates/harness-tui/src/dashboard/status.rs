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
    let mut status = match catalog_status {
        Some(RunStatus::Running) => DashboardStatus::Running,
        Some(RunStatus::Finished) => DashboardStatus::Completed,
        Some(RunStatus::Failed) => DashboardStatus::Failed,
        None => DashboardStatus::Stale,
    };
    for event in events {
        status = if matches!(event.payload, EventV1::RunStarted(_)) {
            DashboardStatus::Running
        } else if matches!(
            event.payload,
            EventV1::TaskScheduled(TaskScheduledEvent {
                state: TaskScheduleState::Queued,
                ..
            })
        ) {
            DashboardStatus::Queued
        } else if matches!(
            event.payload,
            EventV1::TaskScheduled(TaskScheduledEvent {
                state: TaskScheduleState::Started,
                ..
            })
        ) {
            DashboardStatus::Running
        } else if matches!(event.payload, EventV1::ProviderRequestStarted(_))
            || canonical_provider_fragment_payload(&event.payload).is_some()
        {
            DashboardStatus::Streaming
        } else if matches!(event.payload, EventV1::RunFinished(_)) {
            DashboardStatus::Completed
        } else if matches!(event.payload, EventV1::RunFailed(_)) {
            DashboardStatus::Failed
        } else if matches!(
            event.payload,
            EventV1::TaskCancelled(_) | EventV1::AgentStopped(_)
        ) {
            DashboardStatus::Cancelled
        } else if matches!(event.payload, EventV1::StaleDetected(_)) {
            DashboardStatus::Stale
        } else if let EventV1::BackgroundTaskNotification(notification) = &event.payload {
            match notification.status {
                BackgroundTaskNotificationStatus::Completed => DashboardStatus::Completed,
                BackgroundTaskNotificationStatus::Cancelled => DashboardStatus::Cancelled,
                BackgroundTaskNotificationStatus::Failed => DashboardStatus::Failed,
                BackgroundTaskNotificationStatus::TimedOut => DashboardStatus::Stale,
            }
        } else {
            status
        };
    }
    status
}
