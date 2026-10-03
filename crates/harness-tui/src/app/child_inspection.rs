use super::*;

impl AppState {
    pub(super) fn handle_child_quit_key(&mut self, key: KeyEvent) -> bool {
        if !self.current_subagent_session_present()
            || key.modifiers != KeyModifiers::CONTROL
            || key.code != KeyCode::Char('q')
        {
            return false;
        }
        self.execute_action_from_key(Action::Quit, key);
        true
    }
    pub(crate) fn presentation_is_live(&self) -> bool {
        !self.replay_mode
            || self
                .session_navigation_stack
                .first()
                .is_some_and(|parent| !parent.replay_mode)
    }

    pub(super) fn handle_child_status_mouse(&mut self, mouse: MouseEvent, area: Rect) -> bool {
        if !self.current_subagent_session_present()
            || !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        {
            return false;
        }
        let Some(status) = crate::layout::FrameLayoutPlan::for_app(self, area).status else {
            return false;
        };
        let stop = Rect::new(
            status.right().saturating_sub(6),
            status.y,
            6.min(status.width),
            status.height,
        );
        if !stop.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
            return false;
        }
        if self.subagent_header().is_some_and(|header| {
            matches!(
                header.status,
                ActivityStatus::Queued | ActivityStatus::Streaming
            )
        }) {
            self.cancel_inspected_child();
        }
        true
    }

    pub(super) fn cancel_inspected_child(&mut self) {
        if !self.subagent_header().is_some_and(|header| {
            matches!(
                header.status,
                ActivityStatus::Queued | ActivityStatus::Streaming
            )
        }) {
            return;
        }
        if self
            .session_navigation_stack
            .first()
            .is_some_and(|parent| !parent.replay_mode)
        {
            if let Some(session_id) = self.current_session_id().map(str::to_owned) {
                self.request_child_cancel(session_id);
            }
        }
    }

    pub(super) fn request_child_cancel(&mut self, session_id: String) {
        let generation = self
            .projection
            .subagents
            .history
            .records
            .get(&session_id)
            .map_or(0, |record| record.generation);
        if self
            .pending_child_cancels
            .get(&session_id)
            .is_none_or(|(pending, _)| *pending != generation)
        {
            self.pending_child_cancels
                .insert(session_id.clone(), (generation, self.now()));
            self.bump_transcript_render_epoch();
        }
        self.emit_ui_intent(UiIntent::CancelSubagent { session_id });
    }

    pub(crate) fn inspected_child_cancel_started(&self) -> Option<Instant> {
        let id = self.current_session_id()?;
        let (generation, started) = self.pending_child_cancels.get(id)?;
        let record = self.projection.subagents.history.records.get(id)?;
        (record.generation == *generation && record.outcome.is_none()).then_some(*started)
    }

    pub(super) fn handle_child_inspection_key(&mut self, key: KeyEvent) -> bool {
        if !self.current_subagent_session_present() {
            return false;
        }
        if key.kind == crossterm::event::KeyEventKind::Release {
            return true;
        }
        if self.help_is_open() {
            return self.handle_help_browser_key(key);
        }
        if key.modifiers == KeyModifiers::CONTROL
            && matches!(key.code, KeyCode::Char('.') | KeyCode::Char('x'))
        {
            self.help_browser = HelpBrowserState::default();
            self.active_review_surface = Some(ReviewSurface::Help);
            return true;
        }
        self.focus = Focus::Details;
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('q') {
            self.execute_action_from_key(Action::Quit, key);
        } else if key.modifiers == KeyModifiers::NONE
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        {
            self.navigate_to_parent_session();
        } else if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            self.cancel_inspected_child();
        } else if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('e') {
            self.transcript_view.show_transcript_thinking =
                !self.transcript_view.show_transcript_thinking;
            self.bump_transcript_render_epoch();
        } else if !self.composer.vim_mode
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(c) if c.is_ascii_alphabetic() || c == '/')
        {
            // The read-only composer consumes typing when Vim navigation is disabled.
        } else if !self.handle_active_selection_key(key)
            && !self.handle_child_link_key(key)
            && !self.handle_child_content_key(key)
            && !self.handle_transcript_navigation_key(key)
        {
            if let Some(action) = self
                .keymap
                .get_action(&key)
                .filter(|action| child_inspection_action(*action))
            {
                self.execute_action(action);
            }
        }
        true
    }

    fn handle_child_content_key(&mut self, key: KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        match key.code {
            KeyCode::Char('h') => {
                self.set_selected_entry_expanded(false);
            }
            KeyCode::Char('l') => {
                self.set_selected_entry_expanded(true);
            }
            KeyCode::Char('e') => {
                self.toggle_selected_transcript_fold();
            }
            KeyCode::Char('r') => {
                if key.modifiers.is_empty() {
                    self.toggle_child_markdown();
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(entry) = self.selected_transcript_entry() {
                    let content = self.selected_entry_content(&entry);
                    let text = if key.code == KeyCode::Char('Y') {
                        format!("{}\n{}", entry.text, content.content())
                    } else {
                        content.content().to_owned()
                    };
                    match clipboard::copy(&text) {
                        Ok(()) => self.show_toast("Copied to clipboard", ToastVariant::Info),
                        Err(error) => self.show_toast(
                            format!("clipboard copy failed: {error}"),
                            ToastVariant::Error,
                        ),
                    }
                }
            }
            KeyCode::Char('H') => self.execute_action(Action::PreviousMessage),
            KeyCode::Char('L') => self.execute_action(Action::NextMessage),
            KeyCode::Char('j') | KeyCode::Down if key.modifiers.is_empty() => {
                self.move_transcript_entry(true);
            }
            KeyCode::Char('k') | KeyCode::Up if key.modifiers.is_empty() => {
                self.move_transcript_entry(false);
            }
            _ => return false,
        }
        true
    }
}

pub(super) fn child_inspection_action(action: Action) -> bool {
    matches!(
        action,
        Action::Quit
            | Action::ToggleFollow
            | Action::ScrollUp
            | Action::ScrollDown
            | Action::HalfPageDown
            | Action::MoveDown
            | Action::MoveUp
            | Action::FirstMessage
            | Action::LastMessage
            | Action::NextMessage
            | Action::PreviousMessage
            | Action::CopyMessage
            | Action::ToggleScrollbar
            | Action::DiffHunkNext
            | Action::DiffHunkPrevious
            | Action::CloseReviewSurface
            | Action::DismissModal
    )
}
