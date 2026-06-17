use super::*;

impl AppState {
    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.onboarding_accepts_hidden_text()
            && !self.onboarding_auth_in_progress
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Char(c) => {
                    self.execute_action(Action::Char(c));
                    self.maybe_auto_exit();
                    return;
                }
                KeyCode::Backspace => {
                    self.execute_action(Action::Backspace);
                    self.maybe_auto_exit();
                    return;
                }
                _ => {}
            }
        }

        if self.overlay_stack().top() == Some(OverlayKind::PermissionModal) {
            self.handle_permission_modal_key(key);
            return;
        }

        if self.overlay_stack().top() == Some(OverlayKind::StatusDialog) {
            if key.code == KeyCode::Esc {
                self.status_dialog_visible = false;
            }
            self.maybe_auto_exit();
            return;
        }

        if clipboard::copy_on_select_disabled()
            && (self.transcript_selection.is_some() || self.operator_sidebar_selection.is_some())
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

        if key.code == KeyCode::Esc && self.handle_interrupt_escape() {
            self.maybe_auto_exit();
            return;
        }

        if self.focus != Focus::Prompt && self.handle_transcript_navigation_key(key) {
            self.maybe_auto_exit();
            return;
        }

        let mapped_action = self.keymap.get_action(&key);

        if self.startup_shell_visible()
            && self.focus != Focus::Prompt
            && !self.composer_disabled()
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
            && matches!(key.code, KeyCode::Char(_))
        {
            if mapped_action.is_some_and(|action| action_preempts_text_input(action, key)) {
                self.execute_action(mapped_action.expect("preempting action"));
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
            if mapped_action.is_some_and(|action| action_preempts_text_input(action, key)) {
                self.execute_action(mapped_action.expect("preempting action"));
                self.maybe_auto_exit();
                return;
            }

            if let KeyCode::Char(c) = key.code {
                self.execute_action(Action::Char(c));
                self.maybe_auto_exit();
                return;
            }
        }

        let Some(action) = mapped_action else {
            return;
        };

        self.execute_action(action);
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
        self.normalize_focus_for_active_surface();
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
        self.active_tab = Tab::Run;
        self.active_review_surface = Some(surface);
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
                self.focus = if self.focus == Focus::Details && self.terminal_panel_visible() {
                    Focus::Terminal
                } else {
                    Focus::Details
                };
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

    fn move_onboarding_selection(&mut self, delta: isize) {
        let len = onboarding::screen_for(self.onboarding_step, self.onboarding_selected)
            .choices
            .len();
        if len == 0 {
            self.onboarding_selected = 0;
            return;
        }
        self.onboarding_selected = if delta < 0 {
            if self.onboarding_selected == 0 {
                len - 1
            } else {
                self.onboarding_selected - 1
            }
        } else {
            (self.onboarding_selected + 1) % len
        };
    }

    fn request_onboarding_auth(&mut self, args: Vec<String>, stdin: Option<String>) {
        self.onboarding_auth_in_progress = true;
        self.status_banner = Some(auth_status_banner(&args));
        self.emit_ui_intent(UiIntent::OpenAuthManager { args, stdin });
    }

    fn execute_onboarding_auth_step(&mut self) {
        match self.onboarding_step {
            OnboardingStep::CodexBrowser => self.request_onboarding_auth(
                vec![
                    "login".to_string(),
                    "codex".to_string(),
                    "--method".to_string(),
                    "browser".to_string(),
                ],
                None,
            ),
            OnboardingStep::CodexDevice => self.request_onboarding_auth(
                vec![
                    "login".to_string(),
                    "codex".to_string(),
                    "--method".to_string(),
                    "device".to_string(),
                ],
                None,
            ),
            OnboardingStep::CopilotPublicDevice => self.request_onboarding_auth(
                vec![
                    "login".to_string(),
                    "github-copilot".to_string(),
                    "--method".to_string(),
                    "device".to_string(),
                ],
                None,
            ),
            OnboardingStep::CopilotEnterpriseDevice => {
                let enterprise_url = self.onboarding_secret_input.trim().to_string();
                if enterprise_url.is_empty() {
                    self.status_banner =
                        Some("enterprise login requires a domain; input stays hidden".to_string());
                    return;
                }
                self.onboarding_secret_input.clear();
                self.request_onboarding_auth(
                    vec![
                        "login".to_string(),
                        "github-copilot".to_string(),
                        "--method".to_string(),
                        "device".to_string(),
                        "--enterprise-url".to_string(),
                        enterprise_url,
                    ],
                    None,
                );
            }
            OnboardingStep::ApiKeyEntry => {
                let secret = self.onboarding_secret_input.trim().to_string();
                if secret.is_empty() {
                    self.status_banner =
                        Some("api-key login requires a pasted key; input stays hidden".to_string());
                    return;
                }
                self.onboarding_secret_input.clear();
                self.request_onboarding_auth(
                    vec![
                        "login".to_string(),
                        "codex".to_string(),
                        "--method".to_string(),
                        "api-key".to_string(),
                        "--api-key-stdin".to_string(),
                    ],
                    Some(secret),
                );
            }
            _ => {}
        }
    }

    fn execute_onboarding_selection(&mut self) {
        if self.onboarding_auth_in_progress {
            self.status_banner = Some("auth backend already running".to_string());
            return;
        }
        match self.onboarding_step {
            OnboardingStep::StartSplash if self.onboarding_selected == 1 => {
                self.onboarding_step = OnboardingStep::SkipConfirmation;
                self.onboarding_selected = 0;
            }
            OnboardingStep::ProviderPick if self.onboarding_selected == 1 => {
                self.onboarding_step = OnboardingStep::CopilotTargetPick;
                self.onboarding_selected = 0;
            }
            OnboardingStep::CopilotTargetPick => {
                self.onboarding_step = if self.onboarding_selected == 1 {
                    OnboardingStep::CopilotEnterpriseDevice
                } else {
                    OnboardingStep::CopilotPublicDevice
                };
                self.onboarding_selected = 0;
            }
            OnboardingStep::AuthMethodPick => {
                self.onboarding_step = match self.onboarding_selected {
                    1 => OnboardingStep::CodexBrowser,
                    2 => OnboardingStep::ApiKeyEntry,
                    _ => OnboardingStep::CodexDevice,
                };
                self.onboarding_selected = 0;
            }
            OnboardingStep::LoginErrorTimeout if self.onboarding_selected == 1 => {
                self.onboarding_step = OnboardingStep::SkipConfirmation;
                self.onboarding_selected = 0;
            }
            OnboardingStep::CodexBrowser
            | OnboardingStep::CodexDevice
            | OnboardingStep::CopilotPublicDevice
            | OnboardingStep::CopilotEnterpriseDevice
            | OnboardingStep::ApiKeyEntry => {
                self.execute_onboarding_auth_step();
            }
            OnboardingStep::SkipConfirmation if self.onboarding_selected == 0 => {
                self.onboarding_visible = false;
                self.onboarding_skipped_for_launch = true;
                self.status_banner = Some(
                    "onboarding skipped for this launch; no credential was written".to_string(),
                );
            }
            OnboardingStep::FirstPromptSuccess => {
                self.onboarding_visible = false;
                self.apply_new_session_launcher_selection();
            }
            _ => {
                self.onboarding_step = self.onboarding_step.next();
                self.onboarding_selected = 0;
            }
        }
    }

    fn handle_onboarding_text_action(&mut self, action: Action) -> bool {
        if !self.onboarding_visible
            || self.onboarding_auth_in_progress
            || !matches!(
                self.onboarding_step,
                OnboardingStep::ApiKeyEntry | OnboardingStep::CopilotEnterpriseDevice
            )
        {
            return false;
        }

        match action {
            Action::Char(c) => {
                if !c.is_control() {
                    self.onboarding_secret_input.push(c);
                }
                true
            }
            Action::Backspace => {
                self.onboarding_secret_input.pop();
                true
            }
            Action::ClearPrompt => {
                self.onboarding_secret_input.clear();
                true
            }
            _ => false,
        }
    }

    pub(in crate::app) fn execute_action(&mut self, action: Action) {
        if self.execute_permission_action(action) {
            return;
        }

        if self.handle_onboarding_text_action(action) {
            return;
        }

        if self.onboarding_visible && self.focus == Focus::List {
            match action {
                Action::SubmitPrompt => {
                    self.execute_onboarding_selection();
                    return;
                }
                Action::MoveUp | Action::HistoryUp => {
                    self.move_onboarding_selection(-1);
                    return;
                }
                Action::MoveDown | Action::HistoryDown => {
                    self.move_onboarding_selection(1);
                    return;
                }
                Action::DismissModal => {
                    self.onboarding_step = OnboardingStep::SkipConfirmation;
                    self.onboarding_selected = 0;
                    return;
                }
                _ => {}
            }
        }

        if self.handle_operator_sidebar_action(action) {
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
                    | Action::InsertNewline
                    | Action::ClearPrompt
                    | Action::HistoryUp
                    | Action::HistoryDown
                    | Action::CursorLeft
                    | Action::CursorRight
                    | Action::Backspace
                    | Action::Delete
                    | Action::Char(_) => return,
                    _ => {}
                }
            }

            match action {
                Action::SubmitPrompt => {
                    self.submit_prompt();
                    return;
                }
                Action::InsertNewline => {
                    self.insert_prompt_char('\n');
                    return;
                }
                Action::ClearPrompt => {
                    self.clear_prompt_input();
                    return;
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
                    if self.prompt_cursor > 0 {
                        self.prompt_cursor -= 1;
                    }
                    self.sync_file_mention_overlay();
                    return;
                }
                Action::CursorRight => {
                    if self.prompt_cursor < self.prompt_char_count() {
                        self.prompt_cursor += 1;
                    }
                    self.sync_file_mention_overlay();
                    return;
                }
                Action::Backspace => {
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
                _ => {}
            }
        }

        // Handle global actions
        match action {
            Action::Quit => {
                self.restore_parent_session_for_quit();
                self.should_quit = true;
                self.emit_ui_intent(UiIntent::QuitRequested);
            }
            Action::Palette => {
                self.open_palette();
            }
            Action::Help => {
                if self.active_review_surface == Some(ReviewSurface::Help) {
                    self.close_review_surface();
                } else {
                    self.open_review_surface(ReviewSurface::Help);
                }
            }
            Action::ToggleFollow => {
                self.follow_mode = !self.follow_mode;
                if self.follow_mode {
                    self.transcript_scroll = 0;
                }
            }
            Action::ToggleOperatorSidebar
                if !self.replay_mode && !self.post_run_handoff_visible() =>
            {
                let opening = self.active_review_surface.is_some() || !self.details_drawer_open();
                self.active_tab = Tab::Run;
                self.active_review_surface = None;
                self.live_details_drawer_open = opening;
                if (!opening && self.focus == Focus::List)
                    || (opening && self.focus == Focus::Prompt)
                {
                    self.focus = Focus::Details;
                }
            }
            Action::CloseReviewSurface if self.focus != Focus::Prompt => {
                self.close_review_surface();
            }
            Action::OpenEventLog if self.focus != Focus::Prompt => {
                self.open_review_surface(ReviewSurface::Events);
            }
            Action::Reload if self.replay_mode => {
                self.reload_requested = true;
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
            Action::DiffHunkNext => {
                self.navigate_diff_hunk(false);
            }
            Action::DiffHunkPrevious => {
                self.navigate_diff_hunk(true);
            }
            Action::AgentCycle => {
                self.cycle_agent(false);
            }
            Action::AgentCycleReverse => {
                self.cycle_agent(true);
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
                self.cycle_focus_forward();
            }
            Action::FocusPrev => {
                self.cycle_focus_backward();
            }
            Action::RevertWorkspace => {
                self.request_workspace_revert();
            }
            _ => {}
        }
    }

    pub fn next_event(&mut self) {
        if !self.events.is_empty() && self.selected_event_index < self.events.len() - 1 {
            self.selected_event_index += 1;
            self.follow_mode = false;
            self.details_scroll = 0;
        }
    }

    pub fn previous_event(&mut self) {
        if self.selected_event_index > 0 {
            self.selected_event_index -= 1;
            self.follow_mode = false;
            self.details_scroll = 0;
        }
    }

    fn next_activity(&mut self) {
        if !self.activities.is_empty() && self.selected_activity_index < self.activities.len() - 1 {
            self.selected_activity_index += 1;
            self.follow_mode = false;
            self.details_scroll = 0;
            self.transcript_scroll = 0;
        }
    }

    fn previous_activity(&mut self) {
        if self.selected_activity_index > 0 {
            self.selected_activity_index -= 1;
            self.follow_mode = false;
            self.details_scroll = 0;
            self.transcript_scroll = 0;
        }
    }

    fn transcript_surface_active(&self) -> bool {
        self.active_review_surface.is_none()
            && self.focus == Focus::Details
            && !self.details_drawer_open()
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
                    self.terminal_panel_follow = false;
                    self.terminal_panel_scroll = self.last_terminal_panel_max_scroll.get();
                    true
                }
                KeyCode::End => {
                    self.terminal_panel_follow = true;
                    self.terminal_panel_scroll = 0;
                    true
                }
                _ => false,
            };
        }

        if !self.transcript_surface_active() || key.modifiers != KeyModifiers::NONE {
            return false;
        }

        match key.code {
            KeyCode::PageUp => {
                self.scroll_transcript_up(10);
                true
            }
            KeyCode::PageDown => {
                self.scroll_transcript_down(10);
                true
            }
            KeyCode::Home => {
                self.follow_mode = false;
                self.transcript_scroll = self.last_transcript_max_scroll.get();
                true
            }
            KeyCode::End => {
                self.follow_mode = true;
                self.transcript_scroll = 0;
                true
            }
            _ => false,
        }
    }
}

fn action_preempts_text_input(action: Action, key: KeyEvent) -> bool {
    matches!(
        action,
        Action::SessionChildCycle | Action::SessionChildCycleReverse | Action::ToggleTerminalPanel
    ) || matches!(
        (action, key.code, key.modifiers),
        (
            Action::ToggleOperatorSidebar,
            KeyCode::Char('2'),
            KeyModifiers::NONE
        )
    )
}
