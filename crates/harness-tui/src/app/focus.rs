use super::{AppState, Focus};

impl AppState {
    pub(super) fn normalize_focus_for_active_surface(&mut self) {
        if self.replay_mode {
            if self.focus == Focus::Prompt
                || (self.focus == Focus::Terminal && !self.terminal_panel_visible())
                || (self.focus == Focus::List
                    && self.active_review_surface.is_none()
                    && !self.tasks_pane.focused)
            {
                self.focus = Focus::Details;
            }
        } else if self.startup_shell_visible() {
            self.toggle_startup_focus();
        } else {
            self.focus = match self.focus {
                Focus::Prompt if self.active_review_surface.is_some() => Focus::List,
                Focus::Terminal if self.active_review_surface.is_some() => Focus::Details,
                Focus::List
                    if self.active_review_surface.is_none()
                        && !self.session_shell_operator_rail_interactive()
                        && !self.tasks_pane.focused =>
                {
                    Focus::Details
                }
                focus => focus,
            };
        }
    }

    pub(super) fn cycle_focus(&mut self, backward: bool) {
        use Focus::{Details, List, Prompt, Terminal};
        if self.replay_mode {
            // Default Tab keys are consumed earlier; remapped reverse focus can
            // still reach the read-only terminal panel.
            self.focus = if backward && self.focus != Terminal && self.terminal_panel_visible() {
                Terminal
            } else {
                Details
            };
            return;
        }
        if self.startup_shell_visible() {
            self.toggle_startup_focus();
            return;
        }
        if self.active_review_surface.is_some() {
            self.focus = match (self.focus, backward) {
                (List, false) | (Prompt, true) => Details,
                (Prompt, false) | (Details | Terminal, true) => List,
                _ => Prompt,
            };
            return;
        }

        self.focus = if self.session_shell_operator_rail_interactive() {
            match (self.focus, backward) {
                (Details, false) | (Prompt, true) => List,
                (List, true) | (Prompt, false) => Details,
                (Details, true) if self.terminal_panel_visible() => Terminal,
                _ => Prompt,
            }
        } else {
            match (self.focus, backward) {
                (Details, false) | (Prompt, true) if self.terminal_panel_visible() => Terminal,
                (Prompt, _) | (Terminal | List, true) => Details,
                _ => Prompt,
            }
        };
        self.live_details_drawer_open = self.focus == List;
    }

    fn toggle_startup_focus(&mut self) {
        use crate::welcome_surface::WelcomeInput;
        let input = if self.focus == Focus::Prompt {
            self.focus = Focus::List;
            WelcomeInput::FocusMenu
        } else {
            self.focus = Focus::Prompt;
            WelcomeInput::FocusPrompt
        };
        self.welcome.handle(input);
    }
}
