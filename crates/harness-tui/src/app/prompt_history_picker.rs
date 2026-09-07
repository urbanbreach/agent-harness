use super::*;

#[derive(Default)]
pub(crate) struct PromptHistoryPicker {
    pub(crate) visible: bool,
    pub(crate) query: String,
    pub(crate) selected: usize,
    pub(crate) entries: Vec<String>,
}

pub(in crate::app) struct QueuedPromptReturn {
    draft: composer::ComposerSnapshot,
    viewport: super::transcript_viewport::MeasuredTranscriptViewport,
    anchor: Option<ui::TranscriptContentAnchor>,
    selected_entry: Option<ui::TranscriptVisualEntryId>,
    focus: Focus,
}

impl AppState {
    pub(crate) fn prompt_history_matches(&self) -> Vec<&str> {
        let query = self.prompt_history_picker.query.to_lowercase();
        self.prompt_history_picker
            .entries
            .iter()
            .filter(|entry| {
                query
                    .split_whitespace()
                    .all(|word| entry.to_lowercase().contains(word))
            })
            .map(String::as_str)
            .collect()
    }

    pub(in crate::app) fn open_prompt_history_or_queue(&mut self) {
        let hidden = self.hidden_delegated_child_request_ids_in_current_view();
        if let Some(activity) = self.activities.iter().find(|activity| {
            !hidden.contains(activity.request_id.as_str())
                && activity.status == ActivityStatus::Queued
                && activity.user_message.is_some()
        }) {
            let seq = activity.first_seq;
            self.queued_prompt_navigation = Some(QueuedPromptReturn {
                draft: self.composer.snapshot(),
                viewport: self.transcript_view.measured_viewport(),
                anchor: self.transcript_view.measured_anchor.get(),
                selected_entry: self.transcript_view.selected_entry,
                focus: self.focus,
            });
            self.focus = Focus::Details;
            let entries = ui::transcript_navigation_entries(
                self,
                self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)),
            );
            if let Some(entry) = entries.iter().find(|entry| entry.activity_first_seq == seq) {
                self.select_transcript_entry(entry);
            }
            return;
        }
        self.prompt_history_picker = PromptHistoryPicker {
            visible: true,
            query: String::new(),
            selected: 0,
            entries: self.composer.prompt_history.iter().rev().cloned().collect(),
        };
        self.modal_interaction.invalidate();
    }

    pub(in crate::app) fn close_queued_prompt_navigation(&mut self, key: KeyEvent) -> bool {
        if key.code != KeyCode::Esc {
            return false;
        }
        if let Some(snapshot) = self.queued_prompt_navigation.take() {
            self.composer.restore(snapshot.draft);
            self.transcript_view
                .set_measured_viewport(snapshot.viewport);
            self.transcript_view.measured_anchor.set(snapshot.anchor);
            self.transcript_view.selected_entry = snapshot.selected_entry;
            self.focus = snapshot.focus;
            return true;
        }
        false
    }

    pub(in crate::app) fn handle_prompt_history_picker_key(&mut self, key: KeyEvent) {
        let count = self.prompt_history_matches().len();
        match key.code {
            KeyCode::Esc => self.prompt_history_picker.visible = false,
            KeyCode::Enter => {
                let selected = self
                    .prompt_history_matches()
                    .get(self.prompt_history_picker.selected)
                    .map(|text| (*text).to_string());
                if let Some(text) = selected {
                    self.composer.push_undo();
                    self.replace_prompt_input(text);
                }
                self.prompt_history_picker.visible = false;
            }
            KeyCode::Up => {
                self.prompt_history_picker.selected =
                    self.prompt_history_picker.selected.saturating_sub(1)
            }
            KeyCode::Down => {
                self.prompt_history_picker.selected =
                    (self.prompt_history_picker.selected + 1).min(count.saturating_sub(1))
            }
            KeyCode::PageUp => {
                self.prompt_history_picker.selected =
                    self.prompt_history_picker.selected.saturating_sub(8)
            }
            KeyCode::PageDown => {
                self.prompt_history_picker.selected =
                    (self.prompt_history_picker.selected + 8).min(count.saturating_sub(1))
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.prompt_history_picker.query.push(c);
                self.prompt_history_picker.selected = 0;
            }
            KeyCode::Backspace => {
                let _ = self.prompt_history_picker.query.pop();
                self.prompt_history_picker.selected = 0;
            }
            _ => {}
        }
    }
}
