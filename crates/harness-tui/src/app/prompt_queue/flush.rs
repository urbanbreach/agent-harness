//! Delivering the queue when maintenance ends.

use super::*;

/// One delivery of a queue snapshot. The queue itself is empty while this runs.
pub(super) struct FlushRun {
    snapshot: Vec<QueuedItem>,
    /// First item not yet handed over.
    next: usize,
    will_retry: bool,
    /// The first ordinary prompt has been started.
    launched: bool,
    /// A queued `/new` reset the session; the next ordinary text starts the new one.
    fresh_session: bool,
    /// Texts the agent accepted during this flush, as sent.
    accepted: Vec<String>,
    in_flight: Option<String>,
}

enum Step {
    Continue,
    Wait,
    Done,
    Failed(String),
}

const CHANNEL_UNAVAILABLE: &str = "live session input channel is unavailable";

impl AppState {
    /// Snapshots the queue, empties it and delivers the snapshot. With `will_retry` every
    /// item goes to its requested lane and recognised commands run locally in order.
    /// Otherwise commands run locally until the first ordinary prompt, which starts a turn,
    /// and the remaining items go to their lanes as text.
    pub(crate) fn flush_maintenance_queue(&mut self, will_retry: bool) {
        let queue = &self.composer.maintenance_queue;
        if self.replay_mode
            || self.maintenance_running()
            || queue.awaiting.is_some()
            || queue.flush.is_some()
            || queue.items.is_empty()
        {
            return;
        }
        let queue = &mut self.composer.maintenance_queue;
        queue.withdrawn_echo.clear();
        queue.restore_layout = None;
        queue.flush = Some(FlushRun {
            snapshot: queue.items.drain(..).collect(),
            next: 0,
            will_retry,
            launched: false,
            fresh_session: false,
            accepted: Vec::new(),
            in_flight: None,
        });
        self.advance_flush();
    }

    pub(crate) fn maintenance_prompt_receipt(&mut self, id: u64, result: Result<(), String>) {
        let queue = &mut self.composer.maintenance_queue;
        if queue.awaiting != Some(id) {
            return;
        }
        queue.awaiting = None;
        let Some(mut run) = queue.flush.take() else {
            return;
        };
        match result {
            Ok(()) => {
                run.accepted.extend(run.in_flight.take());
                self.composer.maintenance_queue.flush = Some(run);
                self.advance_flush();
            }
            Err(error) => self.fail_flush(run, error),
        }
    }

    fn advance_flush(&mut self) {
        let Some(mut run) = self.composer.maintenance_queue.flush.take() else {
            return;
        };
        loop {
            if run.next >= run.snapshot.len() {
                break;
            }
            if self.maintenance_running() {
                let remaining = run.snapshot.split_off(run.next);
                let queue = &mut self.composer.maintenance_queue;
                for item in remaining.into_iter().rev() {
                    queue.items.push_front(item);
                }
                return;
            }
            match self.deliver_next(&mut run) {
                Step::Continue => {}
                Step::Wait => {
                    self.composer.maintenance_queue.flush = Some(run);
                    return;
                }
                Step::Done => break,
                Step::Failed(error) => {
                    self.fail_flush(run, error);
                    return;
                }
            }
        }
        // Prompts typed while the last deliveries were in flight go out as their own flush.
        self.flush_maintenance_queue(false);
    }

    fn queued_slash_command(&mut self, text: &str) -> Option<&'static str> {
        let previous = std::mem::replace(&mut self.composer.prompt_buffer, text.to_owned());
        let command = self.typed_slash_command();
        self.composer.prompt_buffer = previous;
        command
    }

    fn deliver_next(&mut self, run: &mut FlushRun) -> Step {
        let index = run.next;
        let text = run.snapshot[index].text.clone();
        if (run.will_retry || !run.launched)
            && let Some(command) = self.queued_slash_command(&text)
        {
            return self.run_queued_command(run, index, command);
        }
        if run.fresh_session {
            return self.launch_fresh_session(run, index);
        }
        if self.on_ui_intent.is_none() {
            return Step::Failed(CHANNEL_UNAVAILABLE.to_owned());
        }
        let item = &run.snapshot[index];
        let sent =
            harness_core::commands::expand_input(&self.prompt_commands, &text).unwrap_or(text);
        let attachments = item.submission_attachments();
        let launch_metadata = item.metadata.clone();
        let tagged = !item.tags.is_empty();
        let mut files = Vec::new();
        let mut agents = Vec::new();
        let mut resources = Vec::new();
        for tag in &item.tags {
            match &tag.part {
                file_mentions::FileMentionSelectedTag::File(tag) => files.push(tag.clone()),
                file_mentions::FileMentionSelectedTag::Agent(tag) => agents.push(tag.clone()),
                file_mentions::FileMentionSelectedTag::Resource(tag) => resources.push(tag.clone()),
            }
        }
        let intent = if !run.will_retry && !run.launched {
            UiIntent::SubmitPrompt {
                text: sent.clone(),
                selected_file_tags: files,
                selected_agent_tags: agents,
                selected_resource_tags: resources,
                attachments,
                launch_metadata,
            }
        } else if item.mode == QueueMode::Steer && !tagged {
            // Steering carries no tags; a tagged item waits for the turn like a follow-up.
            UiIntent::SteerPrompt {
                text: sent.clone(),
                attachments,
                launch_metadata,
            }
        } else {
            UiIntent::FollowUpPrompt {
                text: sent.clone(),
                selected_file_tags: files,
                selected_agent_tags: agents,
                selected_resource_tags: resources,
                attachments,
                launch_metadata,
            }
        };
        run.launched = true;
        run.next += 1;
        run.in_flight = Some(sent);
        let queue = &mut self.composer.maintenance_queue;
        queue.generation += 1;
        let id = queue.generation;
        queue.awaiting = Some(id);
        self.emit_ui_intent(UiIntent::QueuedPrompt {
            id,
            prompt: Box::new(intent),
        });
        Step::Wait
    }

    /// Runs a queued command through the editor, as if typed. Commands that start
    /// maintenance report back through a receipt, and the flush waits for it.
    fn run_queued_command(&mut self, run: &mut FlushRun, index: usize, command: &str) -> Step {
        let observed = matches!(command, "compact" | "handoff" | "shake" | "rename");
        let id = observed.then(|| {
            let queue = &mut self.composer.maintenance_queue;
            queue.generation += 1;
            queue.command_dispatch = Some(queue.generation);
            queue.generation
        });
        let saved_text = self.composer.prompt_buffer.clone();
        let saved_tags = std::mem::take(&mut self.file_mention_tags);
        let saved_images = self.composer.slice.take_attachments();
        let item = &mut run.snapshot[index];
        self.replace_prompt_input(item.text.clone());
        self.file_mention_tags = item.tags.clone();
        if let Some(images) = item.images.take() {
            self.composer.slice.append_attachments(images);
        }
        let was_startup = self.startup_mode;
        let previous_banner = self.status_banner.take();
        self.execute_slash_command(command, None);
        self.composer.maintenance_queue.command_dispatch = None;
        let error = self.status_banner.take();
        self.status_banner = previous_banner;
        let images = self.composer.slice.take_attachments();
        item.images = (!images.is_empty()).then_some(images);
        self.clear_prompt_input();
        self.replace_prompt_input(saved_text);
        self.file_mention_tags = saved_tags;
        self.composer.slice.append_attachments(saved_images);
        run.next += 1;
        if let Some(error) = error {
            return Step::Failed(error);
        }
        if self.startup_mode && !was_startup {
            run.fresh_session = true;
        }
        if id.is_some() && self.composer.maintenance_queue.awaiting.is_some() {
            return if self.on_ui_intent.is_none() {
                Step::Failed(CHANNEL_UNAVAILABLE.to_owned())
            } else {
                Step::Wait
            };
        }
        Step::Continue
    }

    /// Starts the session a queued `/new` created with the queued text. A fresh session
    /// takes a single launch prompt, so everything queued behind it joins that prompt, and
    /// the images of those items go with it.
    fn launch_fresh_session(&mut self, run: &mut FlushRun, index: usize) -> Step {
        let rest = &mut run.snapshot[index..];
        let text = rest
            .iter()
            .map(|item| {
                harness_core::commands::expand_input(&self.prompt_commands, &item.text)
                    .unwrap_or_else(|| item.text.clone())
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let images = rest
            .iter_mut()
            .flat_map(|item| item.images.take().into_iter().flatten())
            .collect();
        set_pending_live_launch_metadata(rest[0].metadata.clone());
        set_pending_live_prompt_with_images(text, images);
        self.startup_mode = false;
        self.focus = Focus::Prompt;
        self.emit_ui_intent(UiIntent::NewSession);
        self.should_quit = true;
        run.next = run.snapshot.len();
        Step::Done
    }

    /// Clears what the agent already accepted from this flush, restores the entire snapshot
    /// to the editor in its original order and reports the error.
    fn fail_flush(&mut self, run: FlushRun, error: String) {
        let count = run.snapshot.len();
        let queue = &mut self.composer.maintenance_queue;
        queue.awaiting = None;
        queue.command_dispatch = None;
        queue.restore_layout = None;
        let mut items = run.snapshot;
        items.extend(queue.items.drain(..));
        if !run.accepted.is_empty() {
            queue.withdrawn_echo = run.accepted;
            self.emit_ui_intent(UiIntent::WithdrawAcceptedInput);
        }
        self.merge_items_into_editor(items);
        self.show_toast(
            format!(
                "Failed to send queued message{}: {error}",
                if count == 1 { "" } else { "s" }
            ),
            ToastVariant::Error,
        );
    }
}
