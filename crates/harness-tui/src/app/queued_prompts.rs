use harness_core::event::{TaskScheduleState, TaskTerminalScope};

use super::prompt_editor::{
    move_prompt_management_selection, PendingQueuedPromptCancellation, QueuedPromptEntry,
};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum QueuedPromptRuntimeEvent {
    UserMessage {
        request_id: String,
        text: String,
    },
    Queued {
        request_id: String,
        task_id: String,
    },
    Started {
        request_id: Option<String>,
        task_id: String,
    },
    Cancelled {
        request_id: Option<String>,
        task_id: String,
    },
}

pub(in crate::app) fn queued_prompt_runtime_event(
    event: &EventEnvelopeV1,
) -> Option<QueuedPromptRuntimeEvent> {
    match &event.payload {
        EventV1::UserMessageSubmitted(data) => Some(QueuedPromptRuntimeEvent::UserMessage {
            request_id: data.request_id.clone(),
            text: data.text.clone(),
        }),
        EventV1::TaskScheduled(data) => {
            let request_id = event.correlation_id.clone();
            match (data.state, request_id) {
                (TaskScheduleState::Queued, Some(request_id)) => {
                    Some(QueuedPromptRuntimeEvent::Queued {
                        request_id,
                        task_id: data.task_id.clone(),
                    })
                }
                (TaskScheduleState::Started, request_id) => {
                    Some(QueuedPromptRuntimeEvent::Started {
                        request_id,
                        task_id: data.task_id.clone(),
                    })
                }
                (TaskScheduleState::Queued, None) => None,
            }
        }
        EventV1::TaskCancelled(data) if data.task_scope == Some(TaskTerminalScope::AgentTurn) => {
            Some(QueuedPromptRuntimeEvent::Cancelled {
                request_id: event.correlation_id.clone(),
                task_id: data.task_id.clone(),
            })
        }
        _ => None,
    }
}

impl AppState {
    pub(in crate::app) fn apply_queued_prompt_runtime_event(
        &mut self,
        event: QueuedPromptRuntimeEvent,
    ) {
        match event {
            QueuedPromptRuntimeEvent::UserMessage { request_id, text } => {
                self.attach_queued_prompt_request_id(&text, &request_id);
            }
            QueuedPromptRuntimeEvent::Queued {
                request_id,
                task_id,
            } => self.attach_or_cancel_queued_prompt_task(&request_id, &task_id),
            QueuedPromptRuntimeEvent::Started {
                request_id,
                task_id,
            }
            | QueuedPromptRuntimeEvent::Cancelled {
                request_id,
                task_id,
            } => self.remove_queued_prompt_by_request_or_task(request_id.as_deref(), &task_id),
        }
    }

    pub(in crate::app) fn show_queued_prompts(&mut self) {
        self.close_palette();
        self.clear_slash_menu();
        self.clear_file_mention_menu();
        self.composer.queued_prompt_dialog_visible = true;
        self.composer.stash_dialog_visible = false;
    }

    pub(in crate::app) fn close_queued_prompts(&mut self) {
        self.composer.queued_prompt_dialog_visible = false;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn move_queued_prompt_selection(&mut self, delta: isize) {
        self.composer.queued_prompt_selected = move_prompt_management_selection(
            self.composer.queued_prompt_selected,
            delta,
            self.composer.queued_prompts.len(),
        );
    }

    pub(in crate::app) fn add_queued_prompt_preview(&mut self, text: String) {
        self.composer
            .queued_prompts
            .push(QueuedPromptEntry::new(text));
        self.sync_queued_prompt_count();
        self.composer.queued_prompt_selected = self.composer.queued_prompts.len().saturating_sub(1);
    }

    pub(in crate::app) fn delete_selected_queued_prompt_preview(&mut self) {
        if self.composer.queued_prompts.is_empty() {
            self.composer.queued_prompt_count = 0;
            self.composer.queued_prompt_dialog_visible = false;
            return;
        }
        let selected = self
            .composer
            .queued_prompt_selected
            .min(self.composer.queued_prompts.len().saturating_sub(1));
        let entry = self.composer.queued_prompts.remove(selected);
        if let Some(task_id) = entry.task_id().map(str::to_string) {
            self.emit_ui_intent(UiIntent::CancelQueuedPrompt { task_id });
        } else {
            self.composer
                .pending_queued_prompt_cancellations
                .push(PendingQueuedPromptCancellation::from_entry(entry));
        }
        self.sync_queued_prompt_count();
        self.composer.queued_prompt_selected =
            selected.min(self.composer.queued_prompts.len().saturating_sub(1));
    }

    pub(crate) fn queued_prompts_visual_row_count(&self) -> usize {
        self.composer.queued_prompts.len().max(1)
    }

    fn attach_queued_prompt_request_id(&mut self, text: &str, request_id: &str) {
        if let Some(cancellation) = self
            .composer
            .pending_queued_prompt_cancellations
            .iter_mut()
            .rev()
            .find(|cancellation| cancellation.matches_unclaimed_text(text))
        {
            cancellation.attach_request_id(request_id.to_string());
            return;
        }
        if let Some(entry) = self
            .composer
            .queued_prompts
            .iter_mut()
            .rev()
            .find(|entry| entry.request_id().is_none() && entry.text() == text)
        {
            entry.attach_request_id(request_id.to_string());
        }
    }

    fn attach_or_cancel_queued_prompt_task(&mut self, request_id: &str, task_id: &str) {
        if self.take_pending_queued_prompt_cancellation(request_id) {
            self.emit_ui_intent(UiIntent::CancelQueuedPrompt {
                task_id: task_id.to_string(),
            });
            return;
        }

        if let Some(entry) = self
            .composer
            .queued_prompts
            .iter_mut()
            .find(|entry| entry.request_id() == Some(request_id))
        {
            entry.attach_task_id(task_id.to_string());
            return;
        }

        let Some(text) = self
            .queued_prompt_text_for_request(request_id)
            .map(str::to_string)
        else {
            return;
        };

        if let Some(entry) = self
            .composer
            .queued_prompts
            .iter_mut()
            .rev()
            .find(|entry| entry.request_id().is_none() && entry.text() == text)
        {
            entry.attach_request_id(request_id.to_string());
            entry.attach_task_id(task_id.to_string());
        } else {
            self.composer
                .queued_prompts
                .push(QueuedPromptEntry::scheduled(
                    text,
                    request_id.to_string(),
                    task_id.to_string(),
                ));
            self.composer.queued_prompt_selected =
                self.composer.queued_prompts.len().saturating_sub(1);
        }
        self.sync_queued_prompt_count();
    }

    fn take_pending_queued_prompt_cancellation(&mut self, request_id: &str) -> bool {
        let Some(index) = self
            .composer
            .pending_queued_prompt_cancellations
            .iter()
            .position(|cancellation| cancellation.request_id() == Some(request_id))
        else {
            return false;
        };
        self.composer
            .pending_queued_prompt_cancellations
            .remove(index);
        true
    }

    fn queued_prompt_text_for_request(&self, request_id: &str) -> Option<&str> {
        self.activities
            .iter()
            .find(|activity| activity.request_id == request_id)
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str())
    }

    fn remove_queued_prompt_by_request_or_task(&mut self, request_id: Option<&str>, task_id: &str) {
        self.composer.queued_prompts.retain(|entry| {
            entry.task_id() != Some(task_id)
                && request_id.is_none_or(|request_id| entry.request_id() != Some(request_id))
        });
        if let Some(request_id) = request_id {
            self.composer
                .pending_queued_prompt_cancellations
                .retain(|cancellation| cancellation.request_id() != Some(request_id));
        }
        self.sync_queued_prompt_count();
        self.composer.queued_prompt_selected = self
            .composer
            .queued_prompt_selected
            .min(self.composer.queued_prompts.len().saturating_sub(1));
    }

    fn sync_queued_prompt_count(&mut self) {
        self.composer.queued_prompt_count = self.composer.queued_prompts.len();
    }

    #[cfg(test)]
    pub(crate) fn set_pending_prompt_count_for_test(&mut self, count: usize) {
        self.composer.queued_prompts = (1..=count)
            .map(|index| QueuedPromptEntry::new(format!("queued prompt {index}")))
            .collect();
        self.sync_queued_prompt_count();
        self.composer.queued_prompt_selected = self.composer.queued_prompts.len().saturating_sub(1);
    }
}
