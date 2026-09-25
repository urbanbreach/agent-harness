use super::*;
use crate::rewind_view::{self, RewindInput, RewindPhase, RewindPointInfo, RewindState};
use harness_core::conversation_rewind::RewindPoint;

pub(crate) struct RewindUi {
    pub state: Option<RewindState>,
    pub points: Vec<RewindPoint>,
    pub generation: u64,
    pub last_escape: Option<Instant>,
    pub suppress_until: Option<Instant>,
    pub confirm: bool,
    pub config_path: Option<PathBuf>,
}

impl Default for RewindUi {
    fn default() -> Self {
        Self {
            state: None,
            points: Vec::new(),
            generation: 0,
            last_escape: None,
            suppress_until: None,
            confirm: true,
            config_path: None,
        }
    }
}

impl AppState {
    pub(crate) fn configure_rewind(&mut self) {
        let context = harness_core::config::ConfigDiscoveryContext::from_env();
        match harness_core::config::rewind_confirmation_settings(
            self.settings_project_config_path.as_deref(),
            &context,
        ) {
            Ok((enabled, path)) => {
                self.rewind.confirm = enabled;
                self.rewind.config_path = Some(path);
            }
            Err(error) => self.show_toast(error.to_string(), ToastVariant::Error),
        }
    }

    pub(in crate::app) fn set_rewind_confirmation(&mut self, enabled: bool) -> bool {
        let Some(path) = self.rewind.config_path.as_ref() else {
            self.rewind_error("No TUI config path is available.".into());
            return false;
        };
        match harness_core::config::write_rewind_confirmation(path, enabled) {
            Ok(()) => {
                self.rewind.confirm = enabled;
                true
            }
            Err(error) => {
                let message = error.to_string();
                if self.rewind.state.is_some() {
                    self.rewind_error(message);
                } else {
                    self.show_toast(message, ToastVariant::Error);
                }
                false
            }
        }
    }

    pub(crate) fn open_rewind(&mut self) {
        if self.replay_mode || self.startup_shell_visible() || self.rewind.state.is_some() {
            return;
        }
        self.rewind.generation = self.rewind.generation.wrapping_add(1);
        self.rewind.last_escape = None;
        self.slash_visible = false;
        self.palette_visible = false;
        let busy = self.active_turn_in_progress() || self.active_compaction().is_some();
        self.rewind.state = Some(RewindState {
            phase: if busy {
                RewindPhase::CancelOffer { active_idx: 0 }
            } else {
                RewindPhase::Loading
            },
            anchor_entry_idx: 0,
            stashed_draft: Some(std::mem::take(&mut self.composer)),
            selected_prompt_index: self
                .transcript_view
                .selected_entry
                .and_then(|_| self.selected_transcript_entry())
                .and_then(|entry| {
                    harness_core::conversation_rewind::rewind_points(&self.events)
                        .iter()
                        .position(|point| point.seq == entry.activity_first_seq)
                }),
        });
        self.focus = Focus::Prompt;
        if !busy {
            self.load_rewind_points(false);
        }
    }

    fn load_rewind_points(&mut self, cancel: bool) {
        if let Some(state) = self.rewind.state.as_mut() {
            state.phase = RewindPhase::Loading;
        }
        if cancel {
            if let Some(compaction) = self.active_compaction() {
                self.emit_ui_intent(UiIntent::CancelCompaction {
                    agent_id: compaction.agent_id.clone(),
                });
            }
        }
        self.emit_ui_intent(UiIntent::LoadRewindPoints {
            generation: self.rewind.generation,
            cancel_task_ids: if cancel {
                self.active_interrupt_task_ids().into_iter().collect()
            } else {
                Vec::new()
            },
        });
    }

    pub(crate) fn apply_rewind_points(
        &mut self,
        generation: u64,
        result: Result<Vec<RewindPoint>, String>,
    ) {
        if generation != self.rewind.generation
            || !self
                .rewind
                .state
                .as_ref()
                .is_some_and(|state| matches!(state.phase, RewindPhase::Loading))
        {
            return;
        }
        match result {
            Ok(points) if points.is_empty() => {
                self.dismiss_rewind();
                self.show_toast("No undoable prompts", ToastVariant::Info);
            }
            Ok(points) => {
                self.rewind.points = points;
                let points = self
                    .rewind
                    .points
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(index, point)| RewindPointInfo {
                        prompt_index: index,
                        created_at: String::new(),
                        num_file_snapshots: 0,
                        prompt_preview: Some(crate::ui::safe_product_text(
                            &point.text.replace('\n', " "),
                        )),
                        has_file_changes: false,
                    })
                    .collect();
                let preselected = self
                    .rewind
                    .state
                    .as_ref()
                    .and_then(|state| state.selected_prompt_index)
                    .filter(|index| *index < self.rewind.points.len());
                if let Some(state) = self.rewind.state.as_mut() {
                    state.phase = RewindPhase::Picker {
                        points,
                        selected: 0,
                    };
                }
                if let Some(index) = preselected {
                    self.apply_rewind_input(RewindInput::PickerSelect(index));
                } else {
                    self.sync_rewind_anchor();
                }
            }
            Err(message) => self.rewind_error(message),
        }
    }

    fn dismiss_rewind(&mut self) {
        if let Some(draft) = self
            .rewind
            .state
            .take()
            .and_then(|state| state.stashed_draft)
        {
            self.composer = draft;
        }
        self.rewind.points.clear();
        self.rewind.generation = self.rewind.generation.wrapping_add(1);
        self.focus = Focus::Prompt;
    }

    fn rewind_error(&mut self, message: String) {
        if let Some(state) = self.rewind.state.as_mut() {
            state.phase = RewindPhase::Error {
                message: crate::ui::safe_product_text(&message),
            };
        }
    }

    pub(crate) fn apply_rewind_result(
        &mut self,
        generation: u64,
        result: Result<RewindPoint, String>,
    ) {
        if generation != self.rewind.generation
            || !self
                .rewind
                .state
                .as_ref()
                .is_some_and(|state| matches!(state.phase, RewindPhase::Executing { .. }))
        {
            return;
        }
        match result {
            Ok(point) => {
                self.dismiss_rewind();
                self.clear_prompt_input();
                self.replace_prompt_input(point.text);
                self.show_toast("Reverted conversation", ToastVariant::Rewind);
                self.transcript_view.follow_mode = true;
            }
            Err(message) => self.rewind_error(message),
        }
    }

    pub(crate) fn handle_rewind_key(&mut self, key: KeyEvent) {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.focus = if self.focus == Focus::Prompt {
                Focus::Details
            } else {
                Focus::Prompt
            };
            return;
        }
        if self.focus == Focus::Details {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.scroll_transcript_up(1),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_transcript_down(1),
                KeyCode::PageUp => self.scroll_transcript_up(10),
                KeyCode::PageDown => self.scroll_transcript_down(10),
                KeyCode::Esc => {
                    self.focus = Focus::Prompt;
                }
                _ => {}
            }
            return;
        }
        let Some(state) = self.rewind.state.as_ref() else {
            return;
        };
        let input = rewind_view::handle_rewind_key(state, &key);
        self.apply_rewind_input(input);
    }

    fn apply_rewind_input(&mut self, input: RewindInput) {
        match input {
            RewindInput::Dismissed | RewindInput::DismissError => self.dismiss_rewind(),
            RewindInput::CancelTurnThenProceed => self.load_rewind_points(true),
            RewindInput::MoveUp | RewindInput::MoveDown => {
                if let Some(state) = self.rewind.state.as_mut() {
                    rewind_view::move_cursor(
                        &mut state.phase,
                        if matches!(input, RewindInput::MoveUp) {
                            -1
                        } else {
                            1
                        },
                    );
                }
                self.sync_rewind_anchor();
            }
            RewindInput::ConfirmCursor => {
                if let Some(state) = self.rewind.state.as_ref() {
                    let input = rewind_view::confirm_cursor(&state.phase);
                    self.apply_rewind_input(input);
                }
            }
            RewindInput::PickerSelect(index) => {
                self.anchor_rewind_point(index);
                let prompt_preview = self
                    .rewind
                    .points
                    .get(index)
                    .map(|point| crate::ui::safe_product_text(&point.text.replace('\n', " ")));
                if self.rewind.confirm {
                    if let Some(state) = self.rewind.state.as_mut() {
                        state.phase = RewindPhase::Confirm {
                            target_prompt_index: index,
                            active_idx: 0,
                            prompt_preview,
                        };
                    }
                } else {
                    self.execute_rewind(index);
                }
            }
            RewindInput::Confirm(index) => self.execute_rewind(index),
            RewindInput::ConfirmNeverAsk(index) => {
                self.set_rewind_confirmation(false);
                self.execute_rewind(index);
            }
            RewindInput::Consumed => {}
        }
    }

    fn execute_rewind(&mut self, index: usize) {
        let Some(point) = self.rewind.points.get(index) else {
            return;
        };
        let request_id = point.request_id.clone();
        if let Some(state) = self.rewind.state.as_mut() {
            state.phase = RewindPhase::Executing {
                target_prompt_index: index,
            };
        }
        self.emit_ui_intent(UiIntent::RewindConversation {
            generation: self.rewind.generation,
            request_id,
        });
    }

    pub(crate) fn handle_rewind_escape(&mut self, key: KeyEvent) -> bool {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return key.code == KeyCode::Esc;
        }
        if key.code != KeyCode::Esc || key.modifiers != KeyModifiers::NONE {
            self.rewind.last_escape = None;
            return false;
        }
        if !self.replay_mode
            && (self.active_turn_in_progress() || self.active_compaction().is_some())
        {
            self.rewind.last_escape = None;
            self.rewind.suppress_until = Some(self.now() + Duration::from_millis(1000));
            self.show_toast("Press Ctrl+c to cancel the turn", ToastVariant::Rewind);
            return true;
        }
        if self
            .rewind
            .suppress_until
            .is_some_and(|deadline| self.now() < deadline)
        {
            return true;
        }
        self.rewind.suppress_until = None;
        if self.replay_mode
            || self.startup_shell_visible()
            || self.composer.shell_mode
            || !self.composer.prompt_buffer.is_empty()
            || harness_core::conversation_rewind::rewind_points(&self.events).is_empty()
        {
            return false;
        }
        let now = self.now();
        if self
            .rewind
            .last_escape
            .take()
            .is_some_and(|last| now.saturating_duration_since(last) <= Duration::from_millis(800))
        {
            self.open_rewind();
            if let Some(state) = self.rewind.state.as_mut() {
                state.selected_prompt_index = None;
            }
        } else {
            self.rewind.last_escape = Some(now);
        }
        true
    }

    fn sync_rewind_anchor(&mut self) {
        let index = match self.rewind.state.as_ref().map(|state| &state.phase) {
            Some(RewindPhase::Picker { points, selected }) => {
                points.get(*selected).map(|point| point.prompt_index)
            }
            _ => None,
        };
        if let Some(index) = index {
            self.anchor_rewind_point(index);
        }
    }

    fn anchor_rewind_point(&mut self, index: usize) {
        let Some(point) = self.rewind.points.get(index) else {
            return;
        };
        let seq = point.seq;
        if let Some(state) = self.rewind.state.as_mut() {
            state.anchor_entry_idx = index;
        }
        let screen = self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24));
        let entries = ui::transcript_navigation_entries(self, screen);
        if let Some(entry) = entries.iter().find(|entry| entry.activity_first_seq == seq) {
            let height = crate::layout::FrameLayoutPlan::for_app(self, screen)
                .transcript
                .map_or(0, |area| usize::from(area.height));
            self.set_transcript_scroll_from_top_with_max(
                entry.top.saturating_sub(height / 2),
                entry.max_scroll,
            );
        }
    }

    pub(crate) fn rewind_dim_from_seq(&self) -> Option<u64> {
        let state = self.rewind.state.as_ref()?;
        if matches!(
            state.phase,
            RewindPhase::Picker { .. }
                | RewindPhase::Confirm { .. }
                | RewindPhase::Executing { .. }
        ) {
            self.rewind
                .points
                .get(state.anchor_entry_idx)
                .map(|point| point.seq)
        } else {
            None
        }
    }

    pub(crate) fn rewind_area(&self, screen: Rect) -> Option<Rect> {
        let state = self.rewind.state.as_ref()?;
        let plan = crate::layout::FrameLayoutPlan::for_app(self, screen);
        let prompt = plan.composer?;
        let height = rewind_view::rewind_overlay_height(&state.phase, screen.height)
            .min(prompt.bottom().saturating_sub(plan.content.y));
        Some(Rect::new(
            prompt.x,
            prompt.bottom().saturating_sub(height),
            prompt.width,
            height,
        ))
    }

    pub(crate) fn handle_rewind_mouse(&mut self, mouse: MouseEvent, screen: Rect) -> bool {
        let Some(area) = self.rewind_area(screen) else {
            return false;
        };
        if !area.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
            match mouse.kind {
                MouseEventKind::ScrollUp => self.scroll_transcript_up(3),
                MouseEventKind::ScrollDown => self.scroll_transcript_down(3),
                MouseEventKind::Down(MouseButton::Left) => self.focus = Focus::Details,
                _ => {}
            }
            return true;
        }
        let Some(state) = self.rewind.state.as_mut() else {
            return false;
        };
        let Some(index) = rewind_view::rewind_row_at(&state.phase, area, mouse.column, mouse.row)
        else {
            return true;
        };
        if !matches!(
            mouse.kind,
            MouseEventKind::Moved | MouseEventKind::Down(MouseButton::Left)
        ) {
            return true;
        }
        if rewind_view::set_rewind_cursor(&mut state.phase, index) {
            self.sync_rewind_anchor();
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.focus = Focus::Prompt;
            if let Some(state) = self.rewind.state.as_ref() {
                let input = rewind_view::rewind_activate(&state.phase);
                self.apply_rewind_input(input);
            }
        }
        true
    }
}
