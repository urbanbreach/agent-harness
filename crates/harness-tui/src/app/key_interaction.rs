// allow: SIZE_OK — indivisible key dispatch state machine (TUI key event routing)
use super::*;
use crate::UnwrapOrAbort;

const QUIT_CONFIRMATION_TTL: Duration = Duration::from_millis(1_000);
const QUIT_CONFIRMATION_BANNER: &str = "Press Ctrl+Q again to quit";

impl AppState {
    pub fn handle_key(&mut self, key: KeyEvent) {
        self.modal_interaction.invalidate();
        if self.handle_pending_quit_confirmation(&key) {
            self.maybe_auto_exit();
            return;
        }

        if self.replay_mode && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.focus = Focus::Details;
            return;
        }

        if self.keymap.leader_pending() {
            self.keymap.set_leader_pending(false);
            if let Some(action) = self.keymap.leader_action(&key) {
                if self.active_review_surface == Some(ReviewSurface::Help) {
                    self.close_review_surface();
                }
                self.execute_action_from_key(action, key);
                self.maybe_auto_exit();
                return;
            }
        }

        if key.code == KeyCode::Char('x') && key.modifiers == KeyModifiers::CONTROL {
            self.keymap.set_leader_pending(true);
            self.execute_action(Action::Help);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::ReleaseNotes) {
            self.handle_release_notes_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::PermissionModal) {
            self.handle_permission_modal_key(key);
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::AuthDialog) {
            self.handle_connect_dialog_key(key);
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::NewWorktreeDialog) {
            self.handle_new_worktree_dialog_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::StatusDialog) {
            if key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::CONTROL {
                self.execute_action(Action::Quit);
            } else {
                self.handle_status_dashboard_key(key);
            }
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::SubagentActions) {
            self.handle_subagent_actions_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::ThemeDialog) {
            self.handle_theme_dialog_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::ErrorDetails) {
            self.handle_error_details_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::PromptStashList) {
            self.handle_prompt_stash_list_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::SettingsEditor) {
            self.handle_settings_editor_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::PlanView) {
            self.handle_plan_view_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::MemoryBrowser) {
            self.handle_memory_browser_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::WorktreePicker) {
            self.handle_worktree_picker_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::ForeignImportPicker) {
            self.handle_foreign_import_picker_key(&key);
            self.maybe_auto_exit();
            return;
        }

        if self.composer_completion_active() {
            match key.code {
                KeyCode::Esc => self.composer_cancel_completion(),
                KeyCode::Up => self.composer_move_completion(
                    crate::completion_controller::SelectionDirection::Previous,
                ),
                KeyCode::Down => self.composer_move_completion(
                    crate::completion_controller::SelectionDirection::Next,
                ),
                KeyCode::Enter => {
                    let _ = self.composer_accept_completion_keyboard();
                }
                _ => {}
            }
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Up | KeyCode::Down | KeyCode::Enter
            ) {
                self.maybe_auto_exit();
                return;
            }
        }

        if self.handle_transcript_viewer_key(key) {
            self.maybe_auto_exit();
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::TrustFolderPrompt) {
            if key.code == KeyCode::Esc {
                self.trust_folder_prompt_visible = false;
            }
            self.maybe_auto_exit();
            return;
        }

        if clipboard::copy_on_select_disabled()
            && (self.transcript_view.transcript_selection.is_some()
                || self.secondary_surfaces.selection.is_some())
        {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
            {
                if let Some(frame_area) = self.last_frame_area() {
                    if !self.copy_active_selection(frame_area) {
                        self.clear_operator_sidebar_selection();
                        self.clear_transcript_selection();
                        return;
                    }
                }
                self.clear_operator_sidebar_selection();
                self.clear_transcript_selection();
                self.maybe_auto_exit();
                return;
            }

            if key.code == KeyCode::Esc {
                self.clear_operator_sidebar_selection();
                self.clear_transcript_selection();
                self.maybe_auto_exit();
                return;
            }

            self.clear_operator_sidebar_selection();
            self.clear_transcript_selection();
        }

        if self.handle_navigation_overlay_key(&key) {
            self.maybe_auto_exit();
            return;
        }

        if self.active_review_surface == Some(ReviewSurface::Help) {
            self.handle_help_browser_key(key);
            self.maybe_auto_exit();
            return;
        }

        if self.active_review_surface.is_some() && key.code == KeyCode::Esc {
            self.close_review_surface();
            self.maybe_auto_exit();
            return;
        }

        if self.replay_mode && key.code == KeyCode::Esc && !self.session_navigation_stack.is_empty()
        {
            self.navigate_to_parent_session();
            self.maybe_auto_exit();
            return;
        }

        if key.modifiers == KeyModifiers::CONTROL
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
            && self.handle_ctrl_c_clear_or_cancel()
        {
            self.maybe_auto_exit();
            return;
        }

        if key.code == KeyCode::Esc && self.handle_interrupt_escape() {
            self.maybe_auto_exit();
            return;
        }

        if key.code == KeyCode::Esc
            && key.modifiers == KeyModifiers::NONE
            && self.composer_render_text().is_empty()
            && self.composer_ghost_eligible()
        {
            self.composer_dismiss_suggestion();
            self.maybe_auto_exit();
            return;
        }

        if key.code == KeyCode::Esc && self.handle_clear_prompt_escape() {
            self.maybe_auto_exit();
            return;
        }

        if key.modifiers == KeyModifiers::NONE
            && matches!(key.code, KeyCode::Tab | KeyCode::Right)
            && self.composer_ghost_eligible()
            && self.composer_accept_full_suggestion().is_ok()
        {
            self.maybe_auto_exit();
            return;
        }

        if self.handle_prompt_reachable_session_key(key) {
            self.maybe_auto_exit();
            return;
        }

        if self.focus == Focus::Prompt && self.handle_prompt_transcript_scroll_key(key) {
            self.maybe_auto_exit();
            return;
        }

        if self.focus == Focus::Prompt
            && key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('j')
        {
            self.execute_action(Action::InsertNewline);
            self.maybe_auto_exit();
            return;
        }

        if self.focus != Focus::Prompt && self.handle_transcript_navigation_key(key) {
            self.maybe_auto_exit();
            return;
        }

        let mapped_action = if self.focus == Focus::Prompt
            && self.composer.multiline_mode
            && key.code == KeyCode::Enter
        {
            if key.modifiers == KeyModifiers::NONE {
                Some(Action::InsertNewline)
            } else if key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
            {
                Some(Action::SubmitPrompt)
            } else {
                self.keymap.get_action(&key)
            }
        } else {
            self.keymap.get_action(&key)
        };

        if self.startup_shell_visible()
            && self.focus != Focus::Prompt
            && !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(_))
        {
            if mapped_action.is_some_and(|action| action_preempts_text_input(action, key)) {
                self.execute_action_from_key(mapped_action.unwrap_or_abort(), key);
                self.maybe_auto_exit();
                return;
            }

            if let KeyCode::Char(c) = key.code {
                self.focus = Focus::Prompt;
                self.execute_action(Action::Char(c));
                self.maybe_auto_exit();
                return;
            }
        }

        if self.focus == Focus::Details
            && !self.composer_disabled()
            && !self.replay_mode
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(_))
        {
            if mapped_action.is_some_and(|action| action_preempts_text_input(action, key)) {
                self.execute_action_from_key(mapped_action.unwrap_or_abort(), key);
                self.maybe_auto_exit();
                return;
            }

            if let KeyCode::Char(c) = key.code {
                self.focus = Focus::Prompt;
                self.execute_action(Action::Char(c));
                self.maybe_auto_exit();
                return;
            }
        }

        if self.focus == Focus::Prompt
            && !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(_))
        {
            if mapped_action.is_some_and(|action| {
                action != Action::TogglePromptFocus && action_preempts_text_input(action, key)
            }) {
                self.execute_action_from_key(mapped_action.unwrap_or_abort(), key);
                self.maybe_auto_exit();
                return;
            }

            if let KeyCode::Char(c) = key.code {
                if c == '!' && self.composer.prompt_buffer.is_empty() && !self.composer.shell_mode {
                    self.composer.shell_mode = true;
                    self.maybe_auto_exit();
                    return;
                }
                self.execute_action(Action::Char(c));
                self.maybe_auto_exit();
                return;
            }
        }

        let Some(action) = mapped_action else {
            return;
        };

        self.execute_action_from_key(action, key);
        self.maybe_auto_exit();
    }

    pub(in crate::app) fn overlay_backspace(&mut self, on_change: fn(&mut Self)) {
        if self.palette_cursor == 0 {
            return;
        }

        self.palette_cursor -= 1;
        let byte_idx = self
            .palette_input
            .char_indices()
            .nth(self.palette_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.palette_input.len());
        self.palette_input.remove(byte_idx);
        on_change(self);
    }

    fn handle_release_notes_key(&mut self, key: KeyEvent) {
        let area = self.last_frame_area().unwrap_or(Rect::new(0, 0, 100, 30));
        let popup = crate::layout::release_notes_modal_area(area);
        let max_scroll = crate::ui::ui_overlays::release_notes_max_scroll(popup);
        self.release_notes_scroll = match key.code {
            KeyCode::Esc => {
                self.close_release_notes();
                return;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.release_notes_scroll.saturating_add(3).min(max_scroll)
            }
            KeyCode::Up | KeyCode::Char('k') => self.release_notes_scroll.saturating_sub(3),
            KeyCode::PageDown => self.release_notes_scroll.saturating_add(20).min(max_scroll),
            KeyCode::PageUp => self.release_notes_scroll.saturating_sub(20),
            KeyCode::Home => 0,
            KeyCode::End => max_scroll,
            _ => self.release_notes_scroll,
        };
    }

    pub(in crate::app) fn overlay_delete(&mut self, on_change: fn(&mut Self)) {
        if self.palette_cursor >= self.palette_input.chars().count() {
            return;
        }

        let byte_idx = self
            .palette_input
            .char_indices()
            .nth(self.palette_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.palette_input.len());
        self.palette_input.remove(byte_idx);
        on_change(self);
    }

    pub(in crate::app) fn overlay_insert_char(&mut self, c: char, on_change: fn(&mut Self)) {
        let byte_idx = self
            .palette_input
            .char_indices()
            .nth(self.palette_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.palette_input.len());
        self.palette_input.insert(byte_idx, c);
        self.palette_cursor += 1;
        on_change(self);
    }

    pub(in crate::app) fn close_review_surface(&mut self) {
        self.active_review_surface = None;
        self.active_tab = Tab::Run;
        if let Some(previous_focus) = self.review_surface_focus_return.take() {
            self.focus = previous_focus;
        } else {
            self.normalize_focus_for_active_surface();
        }
    }

    pub(in crate::app) fn handle_slash_mouse(&mut self, mouse: MouseEvent, overlay: Rect) {
        let list_area = crate::layout::slash_command_overlay_content_area(overlay);
        if self.slash_filtered.is_empty()
            || list_area.height == 0
            || !rect_contains(list_area, mouse.column, mouse.row)
        {
            return;
        }

        let visible_rows = usize::from(list_area.height);
        let selected = self
            .slash_selected
            .min(self.slash_filtered.len().saturating_sub(1));
        let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        let row = usize::from(mouse.row.saturating_sub(list_area.y));
        let Some(next) = scroll
            .checked_add(row)
            .filter(|index| *index < self.slash_filtered.len())
        else {
            return;
        };

        match mouse.kind {
            MouseEventKind::Moved
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Down(MouseButton::Left) => {
                self.slash_selected = next;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.slash_selected = next;
                self.apply_selected_slash_completion();
            }
            _ => {}
        }
    }

    pub(in crate::app) fn open_review_surface(&mut self, surface: ReviewSurface) {
        self.review_surface_focus_return.get_or_insert(self.focus);
        if self.overlay_state().command_palette_channel_visible() {
            self.close_palette();
        }
        self.active_tab = Tab::Run;
        self.active_review_surface = Some(surface);
        if surface == ReviewSurface::Help {
            self.help_browser.reset();
        }
        if !self.replay_mode {
            self.live_details_drawer_open = false;
        }
        self.normalize_focus_for_active_surface();
    }

    pub(in crate::app) fn normalize_focus_for_active_surface(&mut self) {
        if self.replay_mode {
            if self.focus == Focus::Prompt {
                self.focus = if self.session_shell_operator_rail_interactive() {
                    Focus::List
                } else {
                    Focus::Details
                };
            } else if (self.focus == Focus::Terminal && !self.terminal_panel_visible())
                || (self.active_review_surface.is_none()
                    && !self.session_shell_operator_rail_interactive()
                    && self.focus == Focus::List)
            {
                self.focus = Focus::Details;
            }
            return;
        }

        if self.post_run_handoff_visible() {
            if matches!(self.focus, Focus::Prompt | Focus::Terminal) || self.active_tab == Tab::Run
            {
                self.focus = Focus::List;
            }
            return;
        }

        if self.startup_shell_visible() {
            if self.focus == Focus::Prompt {
                self.focus = Focus::List;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusMenu);
            } else {
                self.focus = Focus::Prompt;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
            }
            return;
        }

        if self.active_review_surface.is_some() && self.focus == Focus::Prompt {
            self.focus = Focus::List;
        } else if (self.active_review_surface.is_some() && self.focus == Focus::Terminal)
            || (self.active_review_surface.is_none()
                && !self.startup_shell_visible()
                && !self.session_shell_operator_rail_interactive()
                && self.focus == Focus::List)
        {
            self.focus = Focus::Details;
        }
    }

    fn cycle_focus_forward(&mut self) {
        if self.replay_mode {
            if !self.session_shell_operator_rail_interactive() {
                self.focus = Focus::Details;
                return;
            }

            self.focus = match self.focus {
                Focus::List => Focus::Details,
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Terminal | Focus::Details | Focus::Prompt => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt | Focus::Terminal => Focus::Details,
                    Focus::Details => Focus::List,
                }
            };
            return;
        }

        if self.startup_shell_visible() {
            if self.focus == Focus::Prompt {
                self.focus = Focus::List;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusMenu);
            } else {
                self.focus = Focus::Prompt;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
            }
            return;
        }

        if self.active_review_surface.is_none()
            && !self.startup_shell_visible()
            && !self.session_shell_operator_rail_interactive()
        {
            self.focus = match self.focus {
                Focus::Prompt => Focus::Details,
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Terminal | Focus::Details | Focus::List => Focus::Prompt,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details => Focus::List,
                Focus::List => Focus::Prompt,
                Focus::Terminal => Focus::Prompt,
                Focus::Prompt => Focus::Details,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Details,
                Focus::Details | Focus::Terminal => Focus::Prompt,
                Focus::Prompt => Focus::List,
            }
        };

        if self.active_review_surface.is_none() {
            self.live_details_drawer_open = self.focus == Focus::List;
        }
    }

    fn cycle_focus_backward(&mut self) {
        if self.replay_mode {
            if !self.session_shell_operator_rail_interactive() {
                self.focus = if self.focus == Focus::Terminal {
                    Focus::Details
                } else if self.terminal_panel_visible() {
                    Focus::Terminal
                } else {
                    Focus::Details
                };
                return;
            }

            self.focus = match self.focus {
                Focus::List | Focus::Prompt => {
                    if self.terminal_panel_visible() {
                        Focus::Terminal
                    } else {
                        Focus::Details
                    }
                }
                Focus::Terminal => Focus::Details,
                Focus::Details => Focus::List,
            };
            return;
        }

        if self.post_run_handoff_visible() {
            self.focus = if self.active_tab == Tab::Run {
                Focus::List
            } else {
                match self.focus {
                    Focus::List | Focus::Prompt | Focus::Terminal => Focus::Details,
                    Focus::Details => Focus::List,
                }
            };
            return;
        }

        if self.startup_shell_visible() {
            if self.focus == Focus::Prompt {
                self.focus = Focus::List;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusMenu);
            } else {
                self.focus = Focus::Prompt;
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
            }
            return;
        }

        if self.active_review_surface.is_none()
            && !self.startup_shell_visible()
            && !self.session_shell_operator_rail_interactive()
        {
            self.focus = match self.focus {
                Focus::Prompt if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Prompt => Focus::Details,
                Focus::Terminal => Focus::Details,
                Focus::Details => Focus::Prompt,
                Focus::List => Focus::Details,
            };
            self.live_details_drawer_open = false;
            return;
        }

        self.focus = if self.active_review_surface.is_none() {
            match self.focus {
                Focus::Details if self.terminal_panel_visible() => Focus::Terminal,
                Focus::Details => Focus::Prompt,
                Focus::Terminal => Focus::Prompt,
                Focus::List => Focus::Details,
                Focus::Prompt => Focus::List,
            }
        } else {
            match self.focus {
                Focus::List => Focus::Prompt,
                Focus::Details | Focus::Terminal => Focus::List,
                Focus::Prompt => Focus::Details,
            }
        };

        if self.active_review_surface.is_none() {
            self.live_details_drawer_open = self.focus == Focus::List;
        }
    }

    fn execute_action_from_key(&mut self, action: Action, key: KeyEvent) {
        if action == Action::Quit {
            self.arm_quit_confirmation(key);
        } else {
            self.execute_action(action);
        }
    }

    fn clear_quit_confirmation(&mut self) {
        self.quit_confirmation_pending = false;
        self.quit_confirmation_shortcut = None;
        self.quit_confirmation_expires_at = None;
        if self.status_banner.as_deref() == Some(QUIT_CONFIRMATION_BANNER) {
            self.status_banner = None;
        }
    }

    fn arm_quit_confirmation(&mut self, key: KeyEvent) {
        self.quit_confirmation_pending = true;
        self.quit_confirmation_shortcut = Some(KeyBinding::new(key.code, key.modifiers));
        self.quit_confirmation_expires_at = Some(self.now() + QUIT_CONFIRMATION_TTL);
        self.status_banner = Some(QUIT_CONFIRMATION_BANNER.to_string());
    }

    fn handle_pending_quit_confirmation(&mut self, key: &KeyEvent) -> bool {
        if !self.quit_confirmation_pending {
            return false;
        }

        let confirmed = self
            .quit_confirmation_shortcut
            .is_some_and(|shortcut| shortcut.matches(key))
            && self
                .quit_confirmation_expires_at
                .is_some_and(|deadline| self.now() < deadline);
        self.clear_quit_confirmation();

        if confirmed {
            self.quit_immediately();
            true
        } else {
            false
        }
    }

    pub(crate) fn clear_expired_quit_confirmation(&mut self) -> bool {
        if self.quit_confirmation_pending
            && self
                .quit_confirmation_expires_at
                .is_some_and(|deadline| self.now() >= deadline)
        {
            self.clear_quit_confirmation();
            true
        } else {
            false
        }
    }

    pub(in crate::app) fn quit_immediately(&mut self) {
        self.clear_quit_confirmation();
        self.restore_parent_session_for_quit();
        self.should_quit = true;
        self.emit_ui_intent(UiIntent::QuitRequested);
    }

    pub(in crate::app) fn execute_action(&mut self, action: Action) {
        if action != Action::Quit {
            self.clear_quit_confirmation();
        }

        if self.execute_permission_action(action) {
            return;
        }

        if self.handle_operator_sidebar_action(action) {
            return;
        }

        if matches!(action, Action::DeleteWordBackward)
            && self.startup_mode
            && self.composer.prompt_buffer.is_empty()
        {
            self.request_new_worktree_session();
            return;
        }

        if self.post_run_handoff_visible() && self.focus == Focus::List {
            match action {
                Action::SubmitPrompt => {
                    self.execute_post_run_handoff_action();
                    return;
                }
                Action::MoveUp | Action::HistoryUp => {
                    self.select_previous_post_run_handoff_action();
                    return;
                }
                Action::MoveDown | Action::HistoryDown => {
                    self.select_next_post_run_handoff_action();
                    return;
                }
                _ => {}
            }
        }

        if self.startup_shell_visible() && self.focus == Focus::List {
            match action {
                Action::SubmitPrompt => {
                    self.execute_startup_launcher_action();
                    return;
                }
                Action::MoveUp | Action::HistoryUp => {
                    self.select_previous_startup_launcher_action();
                    return;
                }
                Action::MoveDown | Action::HistoryDown => {
                    self.select_next_startup_launcher_action();
                    return;
                }
                _ => {}
            }
        }

        if matches!(action, Action::ToggleTerminalPanel) && !self.startup_shell_visible() {
            self.toggle_terminal_panel();
            return;
        }

        // Handle prompt-focused actions
        if self.focus == Focus::Prompt {
            if self.composer_disabled() {
                match action {
                    Action::SubmitPrompt
                    | Action::InterjectPrompt
                    | Action::InsertNewline
                    | Action::ToggleMultiline
                    | Action::ClearPrompt
                    | Action::HistoryUp
                    | Action::HistoryDown
                    | Action::CursorLeft
                    | Action::CursorRight
                    | Action::Backspace
                    | Action::Delete
                    | Action::Char(_)
                    | Action::SelectCharLeft
                    | Action::SelectCharRight
                    | Action::SelectWordLeft
                    | Action::SelectWordRight
                    | Action::SelectLine
                    | Action::SelectAll
                    | Action::MoveWordLeft
                    | Action::MoveWordRight
                    | Action::MoveLineStart
                    | Action::MoveLineEnd
                    | Action::MoveBufferStart
                    | Action::MoveBufferEnd
                    | Action::DeleteWordForward
                    | Action::DeleteWordBackward
                    | Action::DeleteLine
                    | Action::KillToLineStart
                    | Action::KillToLineEnd
                    | Action::Undo
                    | Action::Redo => return,
                    _ => {}
                }
            }

            match action {
                Action::SubmitPrompt => {
                    if self.composer.shell_mode {
                        let command = self.composer.prompt_buffer.trim().to_string();
                        if !command.is_empty() {
                            self.emit_ui_intent(UiIntent::RunShellCommand { command });
                            self.composer.prompt_buffer.clear();
                            self.composer.prompt_cursor = 0;
                            self.composer.shell_mode = false;
                        }
                        return;
                    }
                    if self.send_queued_prompt_now() {
                        return;
                    }
                    self.submit_prompt();
                    return;
                }
                Action::InterjectPrompt => {
                    let task_ids: Vec<String> =
                        self.active_interrupt_task_ids().into_iter().collect();
                    if !task_ids.is_empty() {
                        self.emit_ui_intent(UiIntent::InterruptSession {
                            task_ids,
                            reason: InterruptReason::User,
                        });
                    }
                    self.submit_prompt();
                    return;
                }
                Action::InsertNewline => {
                    self.insert_prompt_char('\n');
                    return;
                }
                Action::ToggleMultiline => {
                    self.composer.multiline_mode = !self.composer.multiline_mode;
                    return;
                }
                Action::ClearPrompt => {
                    if !self.composer.prompt_buffer.is_empty() {
                        self.composer.push_undo();
                    }
                    self.clear_prompt_input();
                    if self.composer.shell_mode {
                        self.composer.shell_mode = false;
                    }
                    return;
                }
                Action::DismissModal => {
                    if self.composer.shell_mode {
                        self.composer.shell_mode = false;
                        return;
                    }
                }
                Action::HistoryUp => {
                    if self.move_prompt_cursor_up() {
                        self.sync_file_mention_overlay();
                        return;
                    }

                    if self.prompt_cursor_at_start() {
                        self.select_previous_prompt_history();
                    }
                    return;
                }
                Action::HistoryDown => {
                    if self.move_prompt_cursor_down() {
                        self.sync_file_mention_overlay();
                        return;
                    }

                    if self.prompt_cursor_at_end() {
                        self.select_next_prompt_history();
                    }
                    return;
                }
                Action::CursorLeft => {
                    if self.composer.editor_matches_prompt_fields()
                        && self.composer.editor_move_left().is_ok()
                    {
                        self.sync_file_mention_overlay();
                        return;
                    }
                    if self.composer.prompt_cursor > 0 {
                        self.composer.prompt_cursor -= 1;
                    }
                    self.composer.selection_anchor = None;
                    self.sync_file_mention_overlay();
                    return;
                }
                Action::CursorRight => {
                    if self.composer.editor_matches_prompt_fields()
                        && self.composer.editor_move_right().is_ok()
                    {
                        self.sync_file_mention_overlay();
                        return;
                    }
                    if self.composer.prompt_cursor < self.prompt_char_count() {
                        self.composer.prompt_cursor += 1;
                    }
                    self.composer.selection_anchor = None;
                    self.sync_file_mention_overlay();
                    return;
                }
                Action::Backspace => {
                    if self.composer.shell_mode
                        && self.composer.prompt_cursor == 0
                        && self.composer.prompt_buffer.is_empty()
                    {
                        self.composer.shell_mode = false;
                        return;
                    }
                    self.backspace_prompt_char();
                    return;
                }
                Action::Delete => {
                    self.delete_prompt_char();
                    return;
                }
                Action::Char(c) => {
                    self.insert_prompt_char(c);
                    return;
                }
                Action::SelectCharLeft => {
                    self.composer_select_char_left();
                    return;
                }
                Action::SelectCharRight => {
                    self.composer_select_char_right();
                    return;
                }
                Action::SelectWordLeft => {
                    self.composer_select_word_left();
                    return;
                }
                Action::SelectWordRight => {
                    self.composer_select_word_right();
                    return;
                }
                Action::SelectLine => {
                    self.composer_select_line();
                    return;
                }
                Action::SelectAll => {
                    self.composer_select_all();
                    return;
                }
                Action::MoveWordLeft => {
                    self.composer_move_word_left();
                    return;
                }
                Action::MoveWordRight => {
                    self.composer_move_word_right();
                    return;
                }
                Action::MoveLineStart => {
                    self.composer_move_line_start();
                    return;
                }
                Action::MoveLineEnd => {
                    self.composer_move_line_end();
                    return;
                }
                Action::MoveBufferStart => {
                    self.composer_move_buffer_start();
                    return;
                }
                Action::MoveBufferEnd => {
                    self.composer_move_buffer_end();
                    return;
                }
                Action::DeleteWordForward => {
                    self.composer_delete_word_forward();
                    return;
                }
                Action::DeleteWordBackward => {
                    if self.startup_mode && self.composer.prompt_buffer.is_empty() {
                        self.request_new_worktree_session();
                        return;
                    }
                    self.composer_delete_word_backward();
                    return;
                }
                Action::DeleteLine => {
                    self.composer_delete_line();
                    return;
                }
                Action::KillToLineStart => {
                    self.composer_kill_to_line_start();
                    return;
                }
                Action::KillToLineEnd => {
                    self.composer_kill_to_line_end();
                    return;
                }
                Action::Undo => {
                    self.composer_undo();
                    return;
                }
                Action::Redo => {
                    self.composer_redo();
                    return;
                }
                _ => {}
            }
        }

        // Handle global actions
        match action {
            Action::Quit => {
                let default_key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
                if self.quit_confirmation_pending
                    && self.handle_pending_quit_confirmation(&default_key)
                {
                    return;
                }
                self.arm_quit_confirmation(default_key);
            }
            Action::HalfPageDown if self.focus == Focus::Details => {
                let viewport = self
                    .last_frame_area()
                    .and_then(|area| crate::layout::FrameLayoutPlan::for_app(self, area).transcript)
                    .map_or(1, |area| usize::from(area.height));
                self.scroll_half_page_down(viewport);
            }
            Action::ScrollDown if self.focus == Focus::Details => {
                self.scroll_page_down(1);
            }
            Action::ToggleTasks => {
                self.live_details_drawer_open = !self.live_details_drawer_open;
                self.focus = if self.live_details_drawer_open {
                    Focus::List
                } else {
                    Focus::Prompt
                };
            }
            Action::TogglePromptFocus => {
                self.focus = if self.focus == Focus::Prompt {
                    Focus::Details
                } else {
                    Focus::Prompt
                };
                self.live_details_drawer_open = false;
            }
            Action::AlwaysApprovePermission => {
                self.toggle_always_approve_mode();
            }
            Action::Palette => {
                self.open_palette();
            }
            Action::OpenSessionHistory => {
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
            Action::Help => {
                if self.active_review_surface == Some(ReviewSurface::Help) {
                    self.close_review_surface();
                } else {
                    self.open_review_surface(ReviewSurface::Help);
                }
            }
            Action::ToggleFollow => {
                self.set_transcript_following(!self.transcript_following());
            }
            Action::OpenStatusDialog => {
                self.open_status_dashboard();
            }
            Action::CloseReviewSurface if self.focus != Focus::Prompt => {
                self.close_review_surface();
            }
            Action::OpenEventLog if self.focus != Focus::Prompt => {
                self.status_banner = Some("event log surface has been removed".to_string());
            }
            Action::Reload if self.replay_mode => {
                self.status_banner = Some("event log reload has been removed".to_string());
            }
            Action::SessionChildFirst => {
                self.navigate_to_first_child_session();
            }
            Action::SessionChildCycle => {
                self.navigate_to_child_sibling(false);
            }
            Action::SessionChildCycleReverse => {
                self.navigate_to_child_sibling(true);
            }
            Action::SessionParent => {
                self.navigate_to_parent_session();
            }
            Action::SessionBackground => {
                if self.replay_mode {
                    self.status_banner = Some(
                        "foreground subagent backgrounding unavailable: replay mode is read-only"
                            .to_string(),
                    );
                } else if let Some(handle_id) = self.focused_demote_handle_id() {
                    self.status_banner = Some(format!(
                        "foreground subagent demote requested ({handle_id})"
                    ));
                    self.emit_ui_intent(UiIntent::DemoteForegroundChildTask { handle_id });
                } else {
                    self.status_banner =
                        Some("foreground subagent backgrounding requested".to_string());
                    self.emit_ui_intent(UiIntent::BackgroundForegroundSubagents);
                }
            }
            Action::DiffHunkNext => {
                self.navigate_diff_hunk(false);
            }
            Action::DiffHunkPrevious => {
                self.navigate_diff_hunk(true);
            }
            Action::VariantCycle => {
                self.cycle_variant();
            }
            Action::MoveDown if self.focus != Focus::Prompt => {
                if self.active_review_surface.is_none() && self.focus == Focus::List {
                    self.next_activity();
                } else if self.focus == Focus::List {
                    self.next_event();
                } else if self.focus == Focus::Terminal {
                    self.scroll_terminal_panel_down(1);
                } else {
                    if self.focus == Focus::Details {
                        if self.transcript_surface_active() {
                            self.scroll_transcript_up(1);
                        } else {
                            self.details_scroll = self.details_scroll.saturating_add(1);
                        }
                    }
                }
            }
            Action::MoveUp if self.focus != Focus::Prompt => {
                if self.active_review_surface.is_none() && self.focus == Focus::List {
                    self.previous_activity();
                } else if self.focus == Focus::List {
                    self.previous_event();
                } else if self.focus == Focus::Terminal {
                    self.scroll_terminal_panel_up(1);
                } else {
                    if self.focus == Focus::Details {
                        if self.transcript_surface_active() {
                            self.scroll_transcript_down(1);
                        } else {
                            self.details_scroll = self.details_scroll.saturating_sub(1);
                        }
                    }
                }
            }
            Action::FocusNext => {
                if self.replay_mode && !self.session_shell_operator_rail_interactive() {
                    self.focus = Focus::Details;
                } else {
                    self.cycle_focus_forward();
                }
            }
            Action::FocusPrev => {
                self.cycle_focus_backward();
            }
            Action::RevertWorkspace => {
                self.request_workspace_revert();
            }
            Action::OpenThemeDialog => {
                self.theme_dialog_visible = true;
                self.theme_dialog_selected = Theme::available_theme_names()
                    .iter()
                    .position(|name| *name == self.theme_name)
                    .unwrap_or(0);
            }
            Action::OpenModelSwitcher => {
                if !self.replay_mode {
                    self.open_model_switcher();
                }
            }
            Action::FirstMessage | Action::MoveBufferStart => {
                if self.transcript_view_model().is_some() {
                    let _ = self.select_transcript_turn_at(0);
                } else if !self.activities.is_empty() {
                    self.transcript_view.selected_activity_index = 0;
                    self.transcript_view.follow_mode = false;
                    self.details_scroll = 0;
                    self.transcript_view.transcript_scroll = 0;
                }
            }
            Action::LastMessage | Action::MoveBufferEnd => {
                if let Some(last) = self
                    .transcript_view_model()
                    .map(|view| view.turns.len().saturating_sub(1))
                {
                    let _ = self.select_transcript_turn_at(last);
                } else if !self.activities.is_empty() {
                    self.transcript_view.selected_activity_index =
                        self.activities.len().saturating_sub(1);
                    self.transcript_view.follow_mode = true;
                    self.details_scroll = 0;
                    self.transcript_view.transcript_scroll = 0;
                }
            }
            Action::NextMessage => {
                if self.transcript_view_model().is_some() {
                    let next = self
                        .transcript_view
                        .selected_activity_index
                        .saturating_add(1);
                    let _ = self.select_transcript_turn_at(next);
                } else if !self.activities.is_empty()
                    && self.transcript_view.selected_activity_index
                        < self.activities.len().saturating_sub(1)
                {
                    self.transcript_view.selected_activity_index += 1;
                    self.transcript_view.follow_mode = false;
                    self.details_scroll = 0;
                    self.transcript_view.transcript_scroll = 0;
                }
            }
            Action::PreviousMessage => {
                if self.transcript_view_model().is_some() {
                    let previous = self
                        .transcript_view
                        .selected_activity_index
                        .saturating_sub(1);
                    let _ = self.select_transcript_turn_at(previous);
                } else if self.transcript_view.selected_activity_index > 0 {
                    self.transcript_view.selected_activity_index -= 1;
                    self.transcript_view.follow_mode = false;
                    self.details_scroll = 0;
                    self.transcript_view.transcript_scroll = 0;
                }
            }
            Action::ToggleScrollbar => {
                self.transcript_view.transcript_scrollbar_visible =
                    !self.transcript_view.transcript_scrollbar_visible;
            }
            Action::CopyMessage => {
                if let Some(activity) = self
                    .activities
                    .get(self.transcript_view.selected_activity_index)
                {
                    let text = activity.transcript_text.clone();
                    if !text.is_empty() {
                        let _ = clipboard::copy(&text);
                        self.show_toast("Copied message", ToastVariant::Info);
                    }
                }
            }
            Action::ExportSession => {
                self.emit_ui_intent(UiIntent::ExportSession);
            }
            Action::OpenErrorDetails => {
                self.error_details_visible = true;
            }
            Action::PromptStash => {
                self.prompt_stash_push();
            }
            Action::PromptStashPop => {
                self.prompt_stash_pop();
            }
            Action::PromptStashList => {
                self.open_prompt_stash_list();
            }
            Action::OpenSettings => {
                self.open_settings_editor();
            }
            Action::OpenViewPlan => {
                self.open_plan_view();
            }
            Action::OpenLineageBrowser => {
                self.open_lineage_browser();
            }
            Action::OpenMemoryBrowser => {
                self.open_memory_browser();
            }
            Action::OpenWorktreePicker => {
                self.open_worktree_picker();
            }
            _ => {}
        }
    }

    fn handle_settings_editor_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.close_settings_editor();
            }
            KeyCode::Up => {
                self.settings_editor_move(-1);
            }
            KeyCode::Down => {
                self.settings_editor_move(1);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.settings_editor_activate_selected();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.settings_editor_reset_selected();
            }
            _ => {}
        }
    }

    fn handle_plan_view_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if self.plan_view_preview.is_some() {
                    self.plan_view_preview = None;
                } else {
                    self.close_plan_view();
                }
            }
            KeyCode::Up => {
                self.plan_view_move(-1);
            }
            KeyCode::Down => {
                self.plan_view_move(1);
            }
            KeyCode::Enter => {
                self.plan_view_open_selected();
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.plan_view_copy_selected_path();
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                self.plan_view_copy_selected_body();
            }
            KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete => {
                self.plan_view_delete_selected();
            }
            _ => {}
        }
    }

    fn handle_theme_dialog_key(&mut self, key: KeyEvent) {
        let names = Theme::available_theme_names();
        let len = names.len();
        match key.code {
            KeyCode::Esc => {
                self.theme_dialog_visible = false;
            }
            KeyCode::Enter => {
                if let Some(name) = names.get(self.theme_dialog_selected) {
                    self.apply_theme_by_name(name);
                }
                self.theme_dialog_visible = false;
            }
            KeyCode::Up => {
                if len > 0 {
                    self.theme_dialog_selected = if self.theme_dialog_selected == 0 {
                        len - 1
                    } else {
                        self.theme_dialog_selected - 1
                    };
                }
            }
            KeyCode::Down => {
                if len > 0 {
                    self.theme_dialog_selected = (self.theme_dialog_selected + 1) % len;
                }
            }
            KeyCode::Home => {
                self.theme_dialog_selected = 0;
            }
            KeyCode::End => {
                self.theme_dialog_selected = len.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn handle_error_details_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.error_details_visible = false;
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.error_details_visible = false;
                if let Some(activity) = self
                    .activities
                    .get(self.transcript_view.selected_activity_index)
                {
                    if let Some(request_id) = activity.request_id.as_str().strip_prefix("error:") {
                        let prompt = request_id.to_string();
                        self.replace_prompt_input(prompt);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_subagent_actions_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_subagent_actions_dialog(),
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('O') => {
                self.open_selected_subagent_session();
            }
            _ => {}
        }
    }

    fn handle_prompt_stash_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.close_prompt_stash_list();
            }
            KeyCode::Up => {
                self.prompt_stash_list_move(-1);
            }
            KeyCode::Down => {
                self.prompt_stash_list_move(1);
            }
            KeyCode::Enter => {
                self.prompt_stash_list_restore_selected();
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.prompt_stash_list_delete_selected();
            }
            _ => {}
        }
    }

    pub fn next_event(&mut self) {
        if !self.events.is_empty() && self.selected_event_index < self.events.len() - 1 {
            self.selected_event_index += 1;
            self.transcript_view.follow_mode = false;
            self.details_scroll = 0;
        }
    }

    pub fn previous_event(&mut self) {
        if self.selected_event_index > 0 {
            self.selected_event_index -= 1;
            self.transcript_view.follow_mode = false;
            self.details_scroll = 0;
        }
    }

    fn next_activity(&mut self) {
        if !self.activities.is_empty()
            && self.transcript_view.selected_activity_index < self.activities.len() - 1
        {
            self.transcript_view.selected_activity_index += 1;
            self.transcript_view.follow_mode = false;
            self.details_scroll = 0;
            self.transcript_view.transcript_scroll = 0;
        }
    }

    fn previous_activity(&mut self) {
        if self.transcript_view.selected_activity_index > 0 {
            self.transcript_view.selected_activity_index -= 1;
            self.transcript_view.follow_mode = false;
            self.details_scroll = 0;
            self.transcript_view.transcript_scroll = 0;
        }
    }

    fn transcript_surface_active(&self) -> bool {
        self.active_review_surface.is_none()
            && self.focus == Focus::Details
            && !self.details_drawer_open()
    }

    fn handle_prompt_reachable_session_key(&mut self, key: KeyEvent) -> bool {
        if self.focus != Focus::Prompt {
            return false;
        }

        match self.keymap.get_session_action(&key) {
            Some(Action::SessionBackground) => {
                self.execute_action(Action::SessionBackground);
                true
            }
            _ => false,
        }
    }

    fn handle_prompt_transcript_scroll_key(&mut self, key: KeyEvent) -> bool {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::PageUp) => {
                self.scroll_page_up(usize::from(self.transcript_page_scroll_rows()));
                true
            }
            (KeyModifiers::NONE, KeyCode::PageDown) => {
                self.scroll_page_down(usize::from(self.transcript_page_scroll_rows()));
                true
            }
            (KeyModifiers::CONTROL, KeyCode::Up) => {
                self.scroll_transcript_up(1);
                true
            }
            (KeyModifiers::CONTROL, KeyCode::Down) => {
                self.scroll_transcript_down(1);
                true
            }
            _ => false,
        }
    }

    fn handle_transcript_navigation_key(&mut self, key: KeyEvent) -> bool {
        if self.terminal_panel_surface_active() && key.modifiers == KeyModifiers::NONE {
            return match key.code {
                KeyCode::PageUp => {
                    self.scroll_terminal_panel_up(10);
                    true
                }
                KeyCode::PageDown => {
                    self.scroll_terminal_panel_down(10);
                    true
                }
                KeyCode::Home => {
                    self.terminal_panel.follow = false;
                    self.terminal_panel.scroll = self.terminal_panel.last_max_scroll.get();
                    true
                }
                KeyCode::End => {
                    self.terminal_panel.follow = true;
                    self.terminal_panel.scroll = 0;
                    true
                }
                _ => false,
            };
        }

        if !self.transcript_surface_active() {
            return false;
        }

        if key.modifiers == KeyModifiers::NONE
            && matches!(
                key.code,
                KeyCode::Char('f') | KeyCode::Char('F') | KeyCode::Char('s') | KeyCode::Char('S')
            )
            && self.handle_transcript_timeline_key(key)
        {
            return true;
        }

        // Shift+Left/Right: turn nav on transcript focus only (composer uses same
        // keys for selection while Focus::Prompt).
        if key.modifiers == KeyModifiers::SHIFT {
            return match key.code {
                KeyCode::Left => {
                    self.execute_action(Action::PreviousMessage);
                    true
                }
                KeyCode::Right => {
                    self.execute_action(Action::NextMessage);
                    true
                }
                _ => false,
            };
        }

        if key.modifiers == KeyModifiers::CONTROL
            && matches!(key.code, KeyCode::Char('e') | KeyCode::Char('E'))
        {
            return self.toggle_selected_transcript_fold();
        }

        if let Some(action) = self.keymap.get_session_action(&key) {
            self.execute_action(action);
            return true;
        }

        if key.modifiers != KeyModifiers::NONE {
            return false;
        }

        match key.code {
            KeyCode::Enter => {
                let tool_call_ids = self.selected_activity_expandable_tool_ids();
                if tool_call_ids.is_empty() {
                    false
                } else {
                    let expand = tool_call_ids.iter().any(|tool_call_id| {
                        !self
                            .tool_call_entry(tool_call_id)
                            .is_some_and(|tool_call| self.tool_output_expanded(tool_call))
                    });
                    self.set_tool_group_outputs_expanded(&tool_call_ids, expand);
                    true
                }
            }
            KeyCode::Char('v') | KeyCode::Char('V') => self.open_selected_transcript_viewer(),
            KeyCode::PageUp => {
                self.scroll_page_up(usize::from(self.transcript_page_scroll_rows()));
                true
            }
            KeyCode::PageDown => {
                self.scroll_page_down(usize::from(self.transcript_page_scroll_rows()));
                true
            }
            KeyCode::Home => {
                self.scroll_goto_top();
                true
            }
            KeyCode::End => {
                self.scroll_goto_bottom();
                true
            }
            _ => false,
        }
    }
}

fn action_preempts_text_input(action: Action, _key: KeyEvent) -> bool {
    matches!(
        action,
        Action::SessionChildCycle
            | Action::SessionChildCycleReverse
            | Action::ToggleTerminalPanel
            | Action::TogglePromptFocus
    ) && !matches!(_key.code, KeyCode::Char(' '))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use std::sync::{Arc, Mutex};

    #[test]
    fn shell_mode_enters_on_bang_when_prompt_empty() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::Prompt;
        assert!(!app.shell_mode());

        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('!'),
            crossterm::event::KeyModifiers::NONE,
        ));

        // assert
        assert!(
            app.shell_mode(),
            "typing ! with empty prompt should enter shell mode"
        );
        assert!(
            app.composer.prompt_buffer.is_empty(),
            "shell mode entry should not add ! to buffer"
        );
    }

    #[test]
    fn shell_mode_exits_on_escape() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::Prompt;
        app.composer.shell_mode = true;

        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));

        // assert
        assert!(!app.shell_mode(), "Esc should exit shell mode");
    }

    #[test]
    fn shell_mode_exits_on_backspace_at_empty_prompt() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::Prompt;
        app.composer.shell_mode = true;

        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        ));

        // assert
        assert!(
            !app.shell_mode(),
            "Backspace at empty prompt should exit shell mode"
        );
    }

    #[test]
    fn shell_mode_submit_emits_run_shell_command_intent() {
        // arrange
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let intent_sink = {
            let intents = Arc::clone(&intents);
            Arc::new(move |intent: UiIntent| {
                intents.lock().unwrap_or_abort().push(intent);
            })
        };

        let mut app = AppState::new_live(None, false, Some(intent_sink));
        app.focus = Focus::Prompt;
        app.composer.shell_mode = true;

        // act
        for ch in "ls -la".chars() {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
        }

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        // assert
        let intents = intents.lock().unwrap_or_abort();
        assert_eq!(intents.len(), 1);
        match &intents[0] {
            UiIntent::RunShellCommand { command } => {
                assert_eq!(command, "ls -la");
            }
            other => panic!("expected RunShellCommand intent, got {other:?}"),
        }
    }

    #[test]
    fn shell_mode_does_not_enter_when_prompt_has_text() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.focus = Focus::Prompt;
        app.composer.prompt_buffer = "hello".to_string();
        app.composer.prompt_cursor = 5;

        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('!'),
            crossterm::event::KeyModifiers::NONE,
        ));

        // assert
        assert!(
            !app.shell_mode(),
            "should not enter shell mode when prompt has text"
        );
        assert_eq!(app.composer.prompt_buffer, "hello!");
    }
}
