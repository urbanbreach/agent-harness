//! Prompts typed while context maintenance runs.
//!
//! Enter queues free text as steering and the follow-up key queues it for after the turn.
//! When maintenance ends the whole queue is snapshotted, emptied and delivered. Delivery
//! failure puts the entire snapshot back into the editor.

mod band;
mod flush;
mod markers;
mod restore;

use super::*;
use crate::composer_integration::{AttachmentEntry, SubmissionAttachment};
pub(crate) use band::PendingBandRow;
use markers::{rewrite_markers, shifted_position, MarkerEdit};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum QueueMode {
    Steer,
    FollowUp,
}

pub(in crate::app) struct QueuedItem {
    pub(in crate::app) text: String,
    pub(in crate::app) mode: QueueMode,
    /// Absent, never an empty list, when the draft carried no image.
    pub(in crate::app) images: Option<Vec<AttachmentEntry>>,
    tags: Vec<FileMentionTag>,
    metadata: LaunchMetadata,
}

impl QueuedItem {
    fn submission_attachments(&self) -> Vec<SubmissionAttachment> {
        self.images
            .iter()
            .flatten()
            .map(|entry| SubmissionAttachment {
                id: entry.id,
                mime: entry.attachment.mime(),
                bytes: entry.attachment.bytes().to_vec(),
            })
            .collect()
    }
}

#[derive(Default)]
pub(in crate::app) struct MaintenanceQueue {
    pub(in crate::app) items: VecDeque<QueuedItem>,
    pub(in crate::app) awaiting: Option<u64>,
    pub(in crate::app) command_dispatch: Option<u64>,
    generation: u64,
    flush: Option<flush::FlushRun>,
    restore_layout: Option<restore::RestoreLayout>,
    /// Texts of a failed flush that were already accepted by the agent. The withdrawal that
    /// clears them returns them again, and they are already back in the editor.
    withdrawn_echo: Vec<String>,
}

/// The pending queue as two bands, `Steering` then `After yield`. In each band the agent's
/// own queue comes first and the compaction queue second.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PendingQueueDisplay {
    pub(crate) steering: Vec<String>,
    pub(crate) after_yield: Vec<String>,
}

fn sync_tag_sources(tags: &mut [FileMentionTag]) {
    for tag in tags {
        let source = match &mut tag.part {
            file_mentions::FileMentionSelectedTag::File(value) => &mut value.source,
            file_mentions::FileMentionSelectedTag::Agent(value) => &mut value.source,
            file_mentions::FileMentionSelectedTag::Resource(value) => &mut value.source,
        };
        source.start = tag.start;
        source.end = tag.end;
    }
}

fn shift_tags_for_edits(tags: &mut [FileMentionTag], edits: &[MarkerEdit]) {
    for tag in tags.iter_mut() {
        tag.start = shifted_position(edits, tag.start);
        tag.end = shifted_position(edits, tag.end);
    }
    sync_tag_sources(tags);
}

impl AppState {
    /// Compaction-queue counts as `(steering, after yield)`. Hidden while a child session is
    /// shown, and during replay.
    pub(crate) fn maintenance_queue_lanes(&self) -> (usize, usize) {
        if self.replay_mode || self.current_subagent_session_present() {
            return (0, 0);
        }
        let items = &self.composer.maintenance_queue.items;
        let after = items
            .iter()
            .filter(|item| item.mode == QueueMode::FollowUp)
            .count();
        (items.len() - after, after)
    }

    pub(crate) fn pending_queue_display(&self) -> PendingQueueDisplay {
        if self.replay_mode || self.current_subagent_session_present() {
            return PendingQueueDisplay::default();
        }
        let hidden = self.hidden_delegated_child_request_ids_in_current_view();
        let items = &self.composer.maintenance_queue.items;
        let queued = |mode: QueueMode| items.iter().filter(move |item| item.mode == mode);
        PendingQueueDisplay {
            steering: self
                .activities
                .iter()
                .filter(|activity| {
                    // The local echo of steering stays until the turn takes it.
                    activity.request_id.is_empty()
                        && activity.status == ActivityStatus::Done
                        && !hidden.contains(activity.request_id.as_str())
                })
                .filter_map(|activity| activity.user_message.as_ref())
                .map(|message| message.text.clone())
                .chain(queued(QueueMode::Steer).map(|item| item.text.clone()))
                .collect(),
            after_yield: self
                .activities
                .iter()
                .filter(|activity| {
                    activity.status == ActivityStatus::Queued
                        && !hidden.contains(activity.request_id.as_str())
                })
                .filter_map(|activity| activity.user_message.as_ref())
                .map(|message| message.text.clone())
                .chain(queued(QueueMode::FollowUp).map(|item| item.text.clone()))
                .collect(),
        }
    }

    /// Queues the draft for after maintenance. A steer consumes the draft; a follow-up keeps
    /// its text. Either way the pending images move to the queue, so the next draft cannot
    /// resend them.
    pub(in crate::app) fn queue_maintenance_draft(&mut self, follow_up: bool) {
        if let Err(error) = self.composer_submission() {
            self.set_status_banner(Some(error.to_string()));
            return;
        }
        let text = self.composer.prompt_buffer.clone();
        let images = self.composer.slice.take_attachments();
        let image_ids: Vec<u64> = images.iter().map(|entry| entry.id.get()).collect();
        let tags = if follow_up {
            self.file_mention_tags.clone()
        } else {
            std::mem::take(&mut self.file_mention_tags)
        };
        let attached = !images.is_empty();
        let queue = &mut self.composer.maintenance_queue;
        queue.withdrawn_echo.clear();
        queue.items.push_back(QueuedItem {
            text: text.clone(),
            mode: if follow_up {
                QueueMode::FollowUp
            } else {
                QueueMode::Steer
            },
            images: attached.then_some(images),
            tags,
            metadata: self.launch_metadata.clone(),
        });
        if self.composer.prompt_history.last() != Some(&text) {
            self.composer.prompt_history.push(text.clone());
            self.save_prompt_history();
        }
        if follow_up {
            self.drop_image_markers_from_draft(&text, &image_ids);
        } else {
            self.clear_prompt_input();
        }
        self.show_toast(
            if attached {
                "Queued message with image for after compaction"
            } else {
                "Queued message for after compaction"
            }
            .to_owned(),
            ToastVariant::Info,
        );
    }

    /// The images of a kept draft went to the queue, so their markers must not dangle.
    fn drop_image_markers_from_draft(&mut self, text: &str, image_ids: &[u64]) {
        let (rewritten, edits) =
            rewrite_markers(text, |number| image_ids.contains(&number).then_some(None));
        if edits.is_empty() {
            return;
        }
        let mut tags = std::mem::take(&mut self.file_mention_tags);
        shift_tags_for_edits(&mut tags, &edits);
        self.replace_prompt_input(rewritten);
        self.file_mention_tags = tags;
    }
}
