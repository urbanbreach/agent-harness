use super::RunStatus;
use crate::event::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct InFlight<'a> {
    pub status: Option<RunStatus>,
    pub open: BTreeMap<(&'static str, &'a str), &'a EventEnvelopeV1>,
}
impl<'a> InFlight<'a> {
    pub fn stable(&self) -> bool {
        self.open.is_empty() && self.status != Some(RunStatus::Running)
    }
    pub fn apply(&mut self, event: &'a EventEnvelopeV1) {
        let update = match &event.payload {
            EventV1::RunStarted(_) => {
                self.status = Some(RunStatus::Running);
                None
            }
            EventV1::RunFinished(_) => {
                self.status = Some(RunStatus::Finished);
                None
            }
            EventV1::RunFailed(_) => {
                self.status = Some(RunStatus::Failed);
                None
            }
            EventV1::TaskScheduled(e) => Some(("task", e.task_id.as_str(), true)),
            EventV1::TaskCompleted(e) => Some(("task", e.task_id.as_str(), false)),
            EventV1::TaskCancelled(e) => Some(("task", e.task_id.as_str(), false)),
            EventV1::TaskResultLate(e) => Some(("task", e.task_id.as_str(), false)),
            EventV1::UserMessageSubmitted(e)
                if !e
                    .text
                    .trim_start()
                    .starts_with("<system-reminder>\n[BACKGROUND TASK ") =>
            {
                Some(("user", e.request_id.as_str(), true))
            }
            EventV1::ProviderRequestStarted(e) => {
                let turn = e
                    .metadata
                    .as_ref()
                    .and_then(|m| m.turn_id.as_deref())
                    .or(event.correlation_id.as_deref())
                    .unwrap_or(e.request_id.as_str());
                self.open.remove(&("user", turn));
                Some(("provider", e.request_id.as_str(), true))
            }
            EventV1::ProviderRequestFinished(e) => Some(("provider", e.request_id.as_str(), false)),
            EventV1::AssistantMessageFinished(e) => {
                for call in &e.parts {
                    if let crate::session::AssistantPart::ToolCall(call) = call {
                        self.open
                            .insert(("tool", call.tool_call_id.as_str()), event);
                    }
                }
                None
            }
            EventV1::ToolCallRequested(e) => Some(("tool", e.tool_call_id.as_str(), true)),
            EventV1::ToolCallStarted(e) => Some(("tool", e.tool_call_id.as_str(), true)),
            EventV1::ToolCallFinished(e)
                if e.output_json
                    .as_ref()
                    .is_some_and(|v| v["detached"] == true) =>
            {
                None
            }
            EventV1::ToolCallFinished(e) | EventV1::EvalCellFinished(e) => {
                Some(("tool", e.tool_call_id.as_str(), false))
            }
            EventV1::PermissionRequested(e) => Some(("permission", e.permission_id.as_str(), true)),
            EventV1::PermissionResolved(e) => Some(("permission", e.permission_id.as_str(), false)),
            EventV1::EditProposed(e) => Some(("edit", e.edit_id.as_str(), true)),
            EventV1::EditApplied(e) => {
                self.close_edit(&e.edit_id, &e.path);
                None
            }
            EventV1::EditRejected(e) => {
                self.close_edit(&e.edit_id, &e.path);
                None
            }
            EventV1::CompactionRequested(e) => Some(("compaction", e.checkpoint_id.as_str(), true)),
            EventV1::CompactionApplied(e) => Some(("compaction", e.checkpoint_id.as_str(), false)),
            EventV1::CompactionFailed(e) => e
                .checkpoint_id
                .as_deref()
                .map(|id| ("compaction", id, false)),
            _ => None,
        };
        if let Some((kind, id, start)) = update {
            if start {
                self.open.entry((kind, id)).or_insert(event);
            } else {
                self.open.remove(&(kind, id));
                if kind == "task" {
                    self.open.remove(&("user", id));
                }
            }
        }
    }
    fn close_edit(&mut self, id: &'a str, path: &str) {
        if self.open.remove(&("edit", id)).is_some() {
            return;
        }
        let key = self.open.iter().find_map(|(key, event)| {
            matches!(&event.payload,EventV1::EditProposed(e) if e.path==path).then_some(*key)
        });
        if let Some(key) = key {
            self.open.remove(&key);
        }
    }
    pub fn terminals(&self, reason: &str) -> Vec<EventEnvelopeV1> {
        self.open
            .iter()
            .filter_map(|((kind, id), source)| {
                let payload = match *kind {
                    "task" | "user" => {
                        if *kind == "user" && self.open.contains_key(&("task", *id)) {
                            return None;
                        }
                        EventV1::TaskCancelled(TaskCancelledEvent {
                            task_id: (*id).into(),
                            reason: reason.into(),
                            failure: true,
                            task_scope: Some(if self.open.contains_key(&("tool", *id)) {
                                TaskTerminalScope::ToolCall
                            } else {
                                TaskTerminalScope::AgentTurn
                            }),
                        })
                    }
                    "tool" => EventV1::ToolCallFinished(ToolCallFinishedEvent {
                        tool_call_id: (*id).into(),
                        status: ToolCallStatus::Failed,
                        output_summary: Some(reason.into()),
                        output_digest: None,
                        output_json: None,
                        metadata: None,
                    }),
                    "provider" => EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                        request_id: (*id).into(),
                        finish_reason: "interrupted".into(),
                        output_digest: None,
                        usage: None,
                        metadata: None,
                    }),
                    "permission" => EventV1::PermissionResolved(PermissionResolvedEvent {
                        permission_id: (*id).into(),
                        decision: PermissionDecision::Deny,
                        reason: Some(reason.into()),
                    }),
                    "edit" => match &source.payload {
                        EventV1::EditProposed(e) => EventV1::EditRejected(EditRejectedEvent {
                            edit_id: e.edit_id.clone(),
                            path: e.path.clone(),
                            reason: reason.into(),
                        }),
                        _ => return None,
                    },
                    "compaction" => match &source.payload {
                        EventV1::CompactionRequested(e) => {
                            EventV1::CompactionFailed(CompactionFailedEvent {
                                agent_id: e.agent_id.clone(),
                                trigger_reason: e.trigger_reason.clone(),
                                reason: reason.into(),
                                checkpoint_id: Some(e.checkpoint_id.clone()),
                                through_seq: Some(e.through_seq),
                                through_request_id: e.through_request_id.clone(),
                            })
                        }
                        _ => return None,
                    },
                    _ => return None,
                };
                Some(EventEnvelopeV1 {
                    payload,
                    ..(*source).clone()
                })
            })
            .collect()
    }
}
