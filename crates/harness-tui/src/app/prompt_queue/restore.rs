//! Taking queued input back into the editor.

use super::*;
use std::collections::HashMap;

const SEPARATOR: &str = "\n\n";

/// Where the compaction queue sits in the editor right after a dequeue, so the agent's own
/// queue, which arrives later, can slot into its band.
pub(super) struct RestoreLayout {
    text: String,
    follow_at: usize,
    count: usize,
}

impl AppState {
    /// Drains the compaction queue into the editor in band order, steering before
    /// follow-ups, with the existing draft last.
    pub(in crate::app) fn restore_maintenance_queue(&mut self) {
        let busy = self.replay_mode || self.current_subagent_session_present();
        let queue = &mut self.composer.maintenance_queue;
        if busy || queue.awaiting.is_some() || queue.flush.is_some() || queue.items.is_empty() {
            return;
        }
        let mut items: Vec<_> = queue.items.drain(..).collect();
        items.sort_by_key(|item| item.mode == QueueMode::FollowUp);
        let count = items.len();
        let follow_at = self.merge_items_into_editor(items);
        self.composer.maintenance_queue.restore_layout = Some(RestoreLayout {
            text: self.composer.prompt_buffer.clone(),
            follow_at,
            count,
        });
    }

    /// Puts `items` into the editor in the order given, joined by blank lines with the
    /// existing draft appended last. Queued image markers move up by the draft's image count
    /// and by the images restored before them, and their images follow the draft's in the
    /// image buffer. Returns the character position where the follow-up band begins.
    pub(super) fn merge_items_into_editor(&mut self, items: Vec<QueuedItem>) -> usize {
        let draft = self.composer.prompt_buffer.clone();
        let mut draft_tags = std::mem::take(&mut self.file_mention_tags);
        let mut last_image = self
            .composer
            .slice
            .attachments()
            .iter()
            .map(|entry| entry.id.get())
            .max()
            .unwrap_or(0);
        let mut text = String::new();
        let mut chars = 0;
        let mut tags = Vec::new();
        let mut follow_at = None;
        for mut item in items {
            if !text.is_empty() {
                text.push_str(SEPARATOR);
                chars += SEPARATOR.len();
            }
            if item.mode == QueueMode::FollowUp {
                follow_at.get_or_insert(chars);
            }
            let mut images = item.images.take().unwrap_or_default();
            let mut renumbered = HashMap::new();
            for entry in &mut images {
                last_image += 1;
                renumbered.insert(entry.id.get(), last_image);
                entry.id = crate::composer_atoms::AttachmentId::new(last_image);
            }
            let (rewritten, edits) = rewrite_markers(&item.text, |number| {
                renumbered.get(&number).map(|number| Some(*number))
            });
            shift_tags_for_edits(&mut item.tags, &edits);
            tags.extend(item.tags.into_iter().map(|mut tag| {
                tag.start += chars;
                tag.end += chars;
                tag
            }));
            chars += rewritten.chars().count();
            text.push_str(&rewritten);
            self.composer.slice.append_attachments(images);
        }
        if !draft.is_empty() {
            if !text.is_empty() {
                text.push_str(SEPARATOR);
                chars += SEPARATOR.len();
            }
            follow_at.get_or_insert(chars);
            for tag in &mut draft_tags {
                tag.start += chars;
                tag.end += chars;
            }
            tags.extend(draft_tags);
            text.push_str(&draft);
        }
        sync_tag_sources(&mut tags);
        self.replace_prompt_input(text);
        self.file_mention_tags = tags;
        follow_at.unwrap_or(chars)
    }

    /// Inserts `block` as its own paragraph at a character position of the editor text.
    fn insert_prompt_block(&mut self, at: usize, block: &str) {
        if block.is_empty() {
            return;
        }
        let text = self.composer.prompt_buffer.clone();
        let byte = self.prompt_char_byte_index(at);
        let (before, after) = text.split_at(byte);
        let (inserted, shift) = if !after.is_empty() {
            (format!("{block}{SEPARATOR}"), block.chars().count() + 2)
        } else if !before.is_empty() {
            (format!("{SEPARATOR}{block}"), 0)
        } else {
            (block.to_owned(), 0)
        };
        let mut tags = std::mem::take(&mut self.file_mention_tags);
        for tag in &mut tags {
            if tag.start >= at {
                tag.start += shift;
            }
            if tag.end > at {
                tag.end += shift;
            }
        }
        sync_tag_sources(&mut tags);
        self.replace_prompt_input(format!("{before}{inserted}{after}"));
        self.file_mention_tags = tags;
    }

    /// Restores interrupted queued input and removes its local echoes. The agent's steering
    /// goes ahead of the compaction queue's steering, and its follow-ups ahead of the
    /// compaction queue's follow-ups, with the draft last.
    pub fn restore_queued_input(&mut self, texts: Vec<String>) {
        if texts.is_empty() {
            return;
        }
        let follow_up = |activity: &ActivityEntry, text: &String| {
            activity.status == ActivityStatus::Queued
                && activity
                    .user_message
                    .as_ref()
                    .is_some_and(|message| &message.text == text)
        };
        let lanes: Vec<bool> = texts
            .iter()
            .map(|text| {
                self.activities
                    .iter()
                    .any(|activity| follow_up(activity, text))
            })
            .collect();
        self.activities.retain(|activity| {
            let never_started = activity.request_id.is_empty()
                || (matches!(
                    activity.status,
                    ActivityStatus::Queued | ActivityStatus::Error
                ) && activity.transcript_text.is_empty()
                    && activity.tool_calls.is_empty());
            !(never_started
                && activity
                    .user_message
                    .as_ref()
                    .is_some_and(|message| texts.contains(&message.text)))
        });
        self.bump_transcript_render_epoch();
        let queue = &mut self.composer.maintenance_queue;
        let mut echoed = std::mem::take(&mut queue.withdrawn_echo);
        let mut steering = Vec::new();
        let mut follow_ups = Vec::new();
        for (text, follow_up) in texts.into_iter().zip(lanes) {
            if let Some(index) = echoed.iter().position(|echo| *echo == text) {
                echoed.remove(index);
            } else if follow_up {
                follow_ups.push(text);
            } else {
                steering.push(text);
            }
        }
        let queue = &mut self.composer.maintenance_queue;
        queue.withdrawn_echo = echoed;
        let layout = queue
            .restore_layout
            .take()
            .filter(|layout| layout.text == self.composer.prompt_buffer);
        let mut count = steering.len() + follow_ups.len();
        if count == 0 {
            return;
        }
        match layout {
            Some(layout) => {
                count += layout.count;
                self.insert_prompt_block(layout.follow_at, &follow_ups.join(SEPARATOR));
                self.insert_prompt_block(0, &steering.join(SEPARATOR));
            }
            None => {
                steering.extend(follow_ups);
                self.insert_prompt_block(0, &steering.join(SEPARATOR));
            }
        }
        self.show_toast(
            if count == 1 {
                "Restored the queued message to the editor".to_string()
            } else {
                format!("Restored {count} queued messages to the editor")
            },
            ToastVariant::Info,
        );
    }
}
