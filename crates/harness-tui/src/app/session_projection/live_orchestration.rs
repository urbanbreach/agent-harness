use super::*;

impl SessionProjection {
    pub(super) fn update_live_orchestration_event(&mut self, event: &EventEnvelopeV1) -> bool {
        match &event.payload {
            EventV1::TaskScheduled(data) => {
                if let Some(request_id) = event.correlation_id.as_deref() {
                    self.note_child_agent_request(event, request_id);
                }
                if data.state == harness_core::event::TaskScheduleState::Queued {
                    if let Some(request_id) = event.correlation_id.as_deref() {
                        if let Some(index) =
                            self.activity_index_or_local_echo(request_id, event.seq)
                        {
                            if let Some(entry) = self.activities.get_mut(index) {
                                if !matches!(
                                    entry.status,
                                    ActivityStatus::Done | ActivityStatus::Error
                                ) {
                                    entry.status = ActivityStatus::Queued;
                                    mark_activity_event(entry, event.seq, event.mono_ms);
                                }
                            }
                        }
                    }
                }
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    merge_orchestration_task_lineage(
                        row,
                        data.metadata
                            .as_ref()
                            .and_then(|metadata| metadata.lineage.as_ref()),
                    );
                    if let Some(queue_key) = data.queue_key.as_ref() {
                        row.queue_key = Some(queue_key.clone());
                    }
                    row.warning = None;
                    if row.child_request_id.is_none() {
                        row.child_request_id = event.correlation_id.clone();
                    }
                    row.state = match data.state {
                        harness_core::event::TaskScheduleState::Queued => {
                            OrchestrationTaskState::Queued
                        }
                        harness_core::event::TaskScheduleState::Started => {
                            OrchestrationTaskState::Running
                        }
                    };
                });
            }
            EventV1::BackgroundTaskNotification(data) => {
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    row.child_session_id = Some(data.child_session_id.to_string());
                    row.child_request_id = Some(data.child_request_id.clone());
                    row.result_summary = non_empty_preserved_string(&data.summary);
                    row.state = match data.status {
                        harness_core::event::BackgroundTaskNotificationStatus::Completed => {
                            OrchestrationTaskState::Completed
                        }
                        harness_core::event::BackgroundTaskNotificationStatus::Cancelled => {
                            OrchestrationTaskState::Cancelled
                        }
                        harness_core::event::BackgroundTaskNotificationStatus::Failed => {
                            OrchestrationTaskState::Failed
                        }
                        harness_core::event::BackgroundTaskNotificationStatus::TimedOut => {
                            OrchestrationTaskState::TimedOut
                        }
                    };
                    row.warning = match data.status {
                        harness_core::event::BackgroundTaskNotificationStatus::Completed => None,
                        _ => Some(data.status.as_str().replace('_', " ")),
                    };
                });
                self.ensure_background_notification_activity(event, data);
            }
            EventV1::StaleDetected(data) => {
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    row.state = OrchestrationTaskState::Stale;
                    row.warning = Some(format!("stale for {} ms", data.stale_for_ms));
                });
            }
            _ => return false,
        }
        true
    }
}
