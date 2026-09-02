// allow: SIZE_OK — TUI app state (session projection + interaction)
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::session_lineage::{latest_clone_stable_prefix, StableSessionPrefix};

pub(crate) use super::session_history::{
    session_history_category_label, session_history_current_marker, session_history_display_title,
    session_history_footer_label,
};
use super::session_slash::{
    auth_slash_args_from_prompt, slash_command_display_width, slash_command_match_rank,
};
use super::{
    auth_status_banner, set_pending_live_launch_metadata, set_pending_live_prompt_draft, AppState,
    Focus, PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
    PostRunHandoffAction, StartupLauncherAction, Tab, UiIntent,
};
use crate::keybindings::{self, Action};
use crate::leaf_actions::group_d_dashboard::{action_for_command, DashboardAction};
use crate::text::has_trimmed_content;

const SLASH_COMMAND_RESULT_LIMIT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineageSlashCommand {
    Fork,
    Tree,
    Clone,
}

impl LineageSlashCommand {
    fn status_banner(self, blocked_reason: Option<&'static str>) -> &'static str {
        match (self, blocked_reason) {
            (Self::Fork, Some("replay")) => "session fork blocked: replay mode is read-only",
            (Self::Clone, Some("replay")) => "session clone blocked: replay mode is read-only",
            (Self::Fork, Some("active")) => {
                "Harness session fork blocked: live session has active work"
            }
            (Self::Clone, Some("active")) => {
                "Harness session clone blocked: live session has active work"
            }
            (Self::Fork, _) => "session fork is prepared; creation is not available yet",
            (Self::Tree, _) => "session tree is prepared; browser is not available yet",
            (Self::Clone, _) => "Harness session clone blocked: no stable prefix is available",
        }
    }
}

fn non_empty_str(value: &str) -> Option<&str> {
    has_trimmed_content(value).then_some(value)
}

impl AppState {
    pub(in crate::app) fn post_run_can_reopen(&self) -> bool {
        self.post_run_reopen_target().is_some()
    }

    fn post_run_reopen_target(&self) -> Option<(&str, &PathBuf)> {
        let run_id = self.run_id().and_then(non_empty_str)?;
        let session_path = self.session_path.as_ref()?;
        Some((run_id, session_path))
    }

    fn default_post_run_handoff_action(&self) -> PostRunHandoffAction {
        if self.post_run_can_reopen() {
            PostRunHandoffAction::ContinueSession
        } else {
            PostRunHandoffAction::StartAnotherSession
        }
    }

    pub(crate) fn selected_post_run_handoff_action(&self) -> PostRunHandoffAction {
        let selected = self.post_run_handoff_action;
        if self.post_run_handoff_actions().contains(&selected) {
            selected
        } else {
            self.default_post_run_handoff_action()
        }
    }

    fn reset_post_run_handoff_selection(&mut self) {
        self.post_run_handoff_action = self.default_post_run_handoff_action();
    }

    pub(in crate::app) fn handle_navigation_overlay_key(&mut self, key: &KeyEvent) -> bool {
        if self.foreign_import_picker.visible {
            return self.handle_foreign_import_picker_key(key);
        }

        if self.lineage_browser_visible {
            return self.handle_lineage_browser_key(key);
        }

        if self.fork_selector_visible {
            return self.handle_fork_selector_key(key);
        }

        if self.session_history_visible {
            return self.handle_session_history_key(key);
        }

        if self.model_switcher_visible {
            return self.handle_model_key(key);
        }

        if self.toggles_menu_visible {
            return self.handle_toggles_key(key);
        }

        if self.palette_visible {
            return self.handle_palette_key(key);
        }

        if self.file_mention_overlay_should_render() {
            return self.handle_file_mention_key(key);
        }

        self.slash_overlay_should_render() && self.handle_slash_key(key)
    }

    fn active_slash_start(&self) -> Option<usize> {
        let cursor_byte = self.prompt_cursor_byte_index();
        let input = &self.composer.prompt_buffer[..cursor_byte];
        input.char_indices().rev().find_map(|(index, character)| {
            if character != '/' {
                return None;
            }
            let at_boundary = index == 0
                || input[..index]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace);
            at_boundary.then_some(index)
        })
    }

    fn active_slash_parts(&self) -> Option<(&str, Option<&str>)> {
        let cursor_byte = self.prompt_cursor_byte_index();
        let slash_start = self.active_slash_start()?;
        let input = &self.composer.prompt_buffer[..cursor_byte];
        let expression = &input[slash_start + 1..];
        let (command, args) = expression
            .split_once(char::is_whitespace)
            .map_or((expression, None), |(command, args)| (command, Some(args)));
        Some((command, args))
    }

    fn active_slash_parts_full(&self) -> Option<(&str, Option<&str>)> {
        let slash_start = self.active_slash_start()?;
        let expression = &self.composer.prompt_buffer[slash_start + 1..];
        let (command, args) = expression
            .split_once(char::is_whitespace)
            .map_or((expression, None), |(command, args)| (command, Some(args)));
        Some((command, args))
    }

    fn active_slash_query(&self) -> Option<&str> {
        self.active_slash_parts().map(|(command, _)| command)
    }

    pub(crate) fn slash_match_query(&self) -> &str {
        self.active_slash_parts()
            .filter(|(_, args)| args.is_none())
            .map_or("", |(command, _)| command)
    }

    pub(crate) fn slash_argument_required(&self, command: &str) -> Option<bool> {
        let (active_command, args) = self.active_slash_parts()?;
        args?;
        keybindings::slash_commands()
            .iter()
            .find(|entry| {
                entry.id == command
                    && (entry.id == active_command || entry.aliases.contains(&active_command))
            })
            .map(|entry| entry.args_required)
    }

    fn active_slash_command_range(&self) -> Option<(usize, usize)> {
        let slash_start = self.active_slash_start()?;
        let command_len = self.composer.prompt_buffer[slash_start + 1..]
            .find(char::is_whitespace)
            .unwrap_or_else(|| {
                self.composer
                    .prompt_buffer
                    .len()
                    .saturating_sub(slash_start + 1)
            });
        Some((slash_start, slash_start + 1 + command_len))
    }

    pub(in crate::app) fn clear_slash_menu(&mut self) {
        self.slash_visible = false;
        self.slash_filtered.clear();
        self.slash_selected = 0;
    }

    pub(in crate::app) fn slash_overlay_should_render(&self) -> bool {
        self.slash_visible
    }

    pub(in crate::app) fn sync_slash_overlay(&mut self) {
        if self.focus != Focus::Prompt
            || self.composer_disabled()
            || self.active_slash_query().is_none()
            || self.palette_visible
            || self.session_history_visible
            || self.model_switcher_visible
            || self.toggles_menu_visible
            || self.active_permission().is_some()
        {
            if !self.composer.prompt_buffer.starts_with('/') {
                self.slash_draft_snapshot = None;
            }
            self.clear_slash_menu();
            return;
        }

        let (slash_query, has_args) = self
            .active_slash_parts()
            .map(|(query, args)| (query.to_lowercase(), args.is_some()))
            .unwrap_or_default();

        self.slash_visible = true;
        if has_args {
            let exact = keybindings::slash_commands().iter().find(|command| {
                command.id == slash_query
                    || command.aliases.iter().any(|alias| *alias == slash_query)
            });
            if let Some(command) = exact.filter(|command| command.takes_args) {
                self.slash_filtered = vec![command.id.to_string()];
                self.slash_selected = 0;
                return;
            }
            self.clear_slash_menu();
            return;
        }
        let mut filtered = keybindings::slash_commands()
            .iter()
            .filter(|command| self.slash_command_available(command.id))
            .filter_map(|command| {
                slash_command_match_rank(
                    command.id,
                    keybindings::slash_command_description(command.id),
                    &slash_query,
                )
                .map(|rank| (rank, command.id.to_string()))
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        self.slash_filtered = filtered
            .into_iter()
            .take(SLASH_COMMAND_RESULT_LIMIT)
            .map(|(_, command)| command)
            .collect();
        self.slash_selected = 0;
    }

    pub(in crate::app) fn typed_slash_command(&self) -> Option<&'static str> {
        self.composer
            .prompt_buffer
            .trim()
            .strip_prefix('/')
            .and_then(|command| {
                let command = command.split_whitespace().next().unwrap_or(command);
                keybindings::slash_commands().iter().find_map(|entry| {
                    ((entry.id == command || entry.aliases.contains(&command))
                        && self.slash_command_available(entry.id))
                    .then_some(entry.id)
                })
            })
    }

    pub(crate) fn slash_command_column_width(&self) -> usize {
        keybindings::slash_commands()
            .iter()
            .filter(|command| self.slash_command_available(command.id))
            .map(|command| slash_command_display_width(command.id))
            .max()
            .unwrap_or(0)
            .saturating_add(2)
    }

    fn slash_command_available(&self, command: &str) -> bool {
        match command {
            "new" | "status" | "dashboard" | "toggles" | "auth" | "connect" | "help" | "exit"
            | "mcps" | "timestamps" | "thinking" | "settings" | "view-plan" => true,
            "sessions" | "replay" => !self.replay_mode,
            "fork" => !self.startup_mode && !self.replay_mode,
            "clone" => !self.startup_mode && self.lineage_write_blocked_reason().is_none(),
            "tree" => !self.startup_mode,
            "models" | "agents" => self.model_switcher_supported(),
            "events" => !self.startup_mode,
            "shell" => self.active_review_surface.is_some(),
            "follow" => !self.replay_mode && !self.startup_mode,
            "compact" => self.compact_session_supported,
            "rename" => !self.replay_mode && !self.startup_mode,
            "copy" | "export" => !self.startup_mode,
            "import" => !self.startup_mode && !self.replay_mode,
            _ => false,
        }
    }

    fn restore_slash_draft(&mut self, preserved_draft: Option<String>) {
        self.replace_prompt_input(preserved_draft.unwrap_or_default());
    }

    fn navigate_to_home_shell(&mut self, draft: String) {
        self.projection.reset();
        self.selected_event_index = 0;
        self.transcript_view.selected_activity_index = 0;
        self.transcript_view.follow_mode = true;
        self.active_tab = Tab::Run;
        self.live_details_drawer_open = false;
        self.secondary_surfaces.close_status_dialog();
        self.startup_mode = true;
        self.startup_launcher_action = StartupLauncherAction::NewSession;
        self.status_banner = None;
        self.details_scroll = 0;
        self.transcript_view.transcript_scroll = 0;
        self.composer.prompt_history.clear();
        self.composer.prompt_history_index = None;
        self.replay_mode = false;
        self.session_path = None;
        self.palette_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.palette_selected = 0;
        self.palette_focus_return = None;
        self.session_history_visible = false;
        self.session_history_selected = 0;
        self.model_switcher_visible = false;
        self.model_filtered.clear();
        self.model_selected = 0;
        self.toggles_menu_visible = false;
        self.toggles_selected = 0;
        self.toggles_yolo_confirm_visible = false;
        self.lineage_browser_visible = false;
        self.fork_selector_visible = false;
        self.continued_post_run_handoff_active = false;
        self.continued_live_reopen_surface_active = false;
        self.continue_disabled_banner = None;
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.permission_prompt.permission_id = None;
        self.permission_prompt.stage = PermissionModalStage::Decision;
        self.permission_prompt.selection = PermissionModalSelection::AllowAlways;
        self.permission_prompt.confirm_selection = PermissionConfirmSelection::Confirm;
        self.question_prompt = super::QuestionPromptState::default();
        self.reload_requested = false;
        self.should_quit = false;
        self.focus = Focus::Prompt;
        self.replace_prompt_input(draft);
    }

    pub fn execute_slash_command(&mut self, command: &str, preserved_draft: Option<String>) {
        self.clear_slash_menu();
        if action_for_command(command) == Some(DashboardAction::OpenDashboard) {
            self.restore_slash_draft(preserved_draft);
            self.open_status_dashboard();
            return;
        }
        match command {
            "new" => self.navigate_to_home_shell(preserved_draft.unwrap_or_default()),
            "sessions" => {
                self.restore_slash_draft(preserved_draft);
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
            "replay" => {
                self.restore_slash_draft(preserved_draft);
                self.begin_session_history_picker(StartupLauncherAction::ReplaySession);
            }
            "models" => {
                self.restore_slash_draft(preserved_draft);
                self.open_model_switcher();
            }
            "agents" => {
                self.restore_slash_draft(preserved_draft);
                self.open_model_switcher();
            }
            "mcps" => {
                self.restore_slash_draft(preserved_draft);
                self.open_toggles_menu();
            }
            "toggles" => {
                self.restore_slash_draft(preserved_draft);
                self.open_toggles_menu();
            }
            "auth" => {
                let auth_args = auth_slash_args_from_prompt(&self.composer.prompt_buffer);
                self.restore_slash_draft(preserved_draft);
                self.status_banner = Some(auth_status_banner(&auth_args));
                self.emit_ui_intent(UiIntent::OpenAuthManager {
                    args: auth_args,
                    stdin: None,
                });
            }
            "connect" => {
                self.restore_slash_draft(preserved_draft);
                self.open_connect_dialog();
            }
            "help" | "feedback" => {
                self.restore_slash_draft(preserved_draft);
                self.execute_action(Action::Help);
            }
            "shell" => {
                self.restore_slash_draft(preserved_draft);
                self.close_review_surface();
            }
            "follow" => {
                self.restore_slash_draft(preserved_draft);
                self.execute_action(Action::ToggleFollow);
            }
            "compact" => {
                self.restore_slash_draft(preserved_draft);
                self.emit_ui_intent(UiIntent::CompactSession);
            }
            "rename" => {
                let title = self
                    .active_slash_parts_full()
                    .and_then(|(active_command, args)| {
                        (active_command == "rename").then_some(args.unwrap_or("").trim())
                    })
                    .unwrap_or_default()
                    .to_string();
                self.restore_slash_draft(preserved_draft);
                if title.is_empty() {
                    self.set_status_banner(Some("session title cannot be empty".to_string()));
                } else {
                    self.emit_ui_intent(UiIntent::UpdateSessionTitle { title });
                }
            }
            "fork" => self
                .execute_passive_lineage_slash_command(preserved_draft, LineageSlashCommand::Fork),
            "tree" => self
                .execute_passive_lineage_slash_command(preserved_draft, LineageSlashCommand::Tree),
            "clone" => self
                .execute_passive_lineage_slash_command(preserved_draft, LineageSlashCommand::Clone),
            "copy" => {
                self.restore_slash_draft(preserved_draft);
                let text: String = self
                    .activities
                    .iter()
                    .map(|a| a.transcript_text.as_str())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if !text.is_empty() {
                    let _ = crate::clipboard::copy(&text);
                    self.show_toast("Copied session transcript", crate::app::ToastVariant::Info);
                }
            }
            "export" => {
                self.restore_slash_draft(preserved_draft);
                self.execute_action(Action::ExportSession);
            }
            "import" => {
                self.restore_slash_draft(preserved_draft);
                let scan_root = self
                    .session_path
                    .clone()
                    .or_else(|| self.file_mention_workspace_root_opt())
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                self.open_foreign_import_picker(scan_root);
            }
            "timestamps" => {
                self.restore_slash_draft(preserved_draft);
                self.transcript_view.show_transcript_timestamps =
                    !self.transcript_view.show_transcript_timestamps;
            }
            "thinking" => {
                self.restore_slash_draft(preserved_draft);
                self.transcript_view.show_transcript_thinking =
                    !self.transcript_view.show_transcript_thinking;
            }
            "settings" => {
                self.restore_slash_draft(preserved_draft);
                self.open_settings_editor();
            }
            "view-plan" => {
                self.restore_slash_draft(preserved_draft);
                self.open_plan_view();
            }
            "exit" => self.quit_immediately(),
            _ => {}
        }
    }

    fn execute_passive_lineage_slash_command(
        &mut self,
        preserved_draft: Option<String>,
        command: LineageSlashCommand,
    ) {
        self.restore_slash_draft(preserved_draft);
        let blocked_reason = match command {
            LineageSlashCommand::Fork if self.replay_mode => Some("replay"),
            LineageSlashCommand::Fork => None,
            LineageSlashCommand::Clone => self.lineage_write_blocked_reason(),
            LineageSlashCommand::Tree => None,
        };
        if blocked_reason.is_none() {
            match command {
                LineageSlashCommand::Fork => {
                    self.open_fork_selector();
                    return;
                }
                LineageSlashCommand::Tree => {
                    self.open_lineage_browser();
                    return;
                }
                LineageSlashCommand::Clone => {
                    self.execute_clone_from_latest_stable_prefix();
                    return;
                }
            }
        }
        self.set_status_banner(Some(command.status_banner(blocked_reason).to_string()));
    }

    pub(in crate::app) fn source_run_dir_for_lineage_write(&self) -> Result<PathBuf, String> {
        self.session_path
            .clone()
            .ok_or_else(|| "Harness session write blocked: no live session path".to_string())
    }

    pub(in crate::app) fn emit_fork_session_intent(
        &mut self,
        stable_prefix: StableSessionPrefix,
        prompt_text: String,
    ) -> Result<(), String> {
        let source_run_dir = self.source_run_dir_for_lineage_write()?;
        self.emit_ui_intent(UiIntent::ForkSession {
            source_run_dir,
            events: self.events.clone(),
            stable_prefix,
            prompt_text,
        });
        Ok(())
    }

    fn execute_clone_from_latest_stable_prefix(&mut self) {
        let stable_prefix = match latest_clone_stable_prefix(&self.events) {
            Ok(prefix) if prefix.event_count > 0 => prefix,
            Ok(_) => {
                self.set_status_banner(Some(
                    "Harness session clone blocked: no stable events are available".to_string(),
                ));
                return;
            }
            Err(err) => {
                self.set_status_banner(Some(format!("Harness session clone blocked: {err}")));
                return;
            }
        };

        match self.source_run_dir_for_lineage_write() {
            Ok(source_run_dir) => {
                self.emit_ui_intent(UiIntent::CloneSession {
                    source_run_dir,
                    events: self.events.clone(),
                    stable_prefix,
                });
            }
            Err(err) => self.set_status_banner(Some(err)),
        }
    }

    fn lineage_write_blocked_reason(&self) -> Option<&'static str> {
        if self.replay_mode {
            Some("replay")
        } else if self.active_turn_in_progress() {
            Some("active")
        } else {
            None
        }
    }

    fn selected_slash_command(&self) -> Option<&'static keybindings::SlashCommand> {
        let selected = self.slash_filtered.get(self.slash_selected)?;
        keybindings::slash_commands()
            .iter()
            .find(|command| command.id == selected)
    }

    pub(in crate::app) fn apply_selected_slash_completion(&mut self) {
        let Some(command) = self.selected_slash_command() else {
            return;
        };
        let Some((start, end)) = self.active_slash_command_range() else {
            return;
        };
        let cursor_byte = self.prompt_cursor_byte_index();
        let argument_separator_len = self.composer.prompt_buffer[end..]
            .chars()
            .next()
            .filter(|character| character.is_whitespace())
            .map_or(0, char::len_utf8);
        let append_space = command.takes_args && argument_separator_len == 0;
        let replacement = if append_space {
            format!("/{} ", command.id)
        } else {
            format!("/{}", command.id)
        };
        self.composer.push_undo();
        self.composer
            .prompt_buffer
            .replace_range(start..end, &replacement);
        let cursor_byte = if cursor_byte <= end {
            start + replacement.len() + argument_separator_len
        } else {
            start + replacement.len() + cursor_byte.saturating_sub(end)
        };
        self.composer.prompt_cursor = self.composer.prompt_buffer[..cursor_byte].chars().count();
        self.composer.selection_anchor = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    fn execute_selected_slash_completion(&mut self) {
        let Some(command) = self.slash_filtered.get(self.slash_selected).cloned() else {
            return;
        };
        let Some(metadata) = keybindings::slash_commands()
            .iter()
            .find(|entry| entry.id == command)
        else {
            return;
        };
        if metadata.args_required
            && self
                .active_slash_parts_full()
                .is_none_or(|(_, args)| args.is_none_or(|args| args.trim().is_empty()))
        {
            self.apply_selected_slash_completion();
            return;
        }
        self.execute_slash_command(&command, self.slash_draft_snapshot.clone());
    }

    pub(in crate::app) fn handle_slash_key(&mut self, key: &KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.restore_slash_draft(self.slash_draft_snapshot.clone());
                true
            }
            (KeyCode::Enter, _) => {
                self.execute_selected_slash_completion();
                true
            }
            (KeyCode::Tab, _) => {
                self.apply_selected_slash_completion();
                true
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.move_slash_selection(-1);
                true
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.move_slash_selection(1);
                true
            }
            _ => false,
        }
    }

    fn move_slash_selection(&mut self, delta: isize) {
        let len = self.slash_filtered.len();
        if len == 0 {
            self.slash_selected = 0;
            return;
        }

        if delta == -1 {
            self.slash_selected = if self.slash_selected == 0 {
                len - 1
            } else {
                self.slash_selected - 1
            };
            return;
        }

        if delta == 1 {
            self.slash_selected = (self.slash_selected + 1) % len;
            return;
        }

        let current =
            isize::try_from(self.slash_selected.min(len.saturating_sub(1))).unwrap_or(isize::MAX);
        let next = (current + delta).clamp(
            0,
            isize::try_from(len.saturating_sub(1)).unwrap_or(isize::MAX),
        );
        self.slash_selected = usize::try_from(next).unwrap_or(0);
    }

    fn handle_palette_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Char('c') if ctrl_only => {
                self.close_palette();
                true
            }
            KeyCode::Enter => {
                self.execute_palette_command();
                true
            }
            KeyCode::Tab => true,
            KeyCode::BackTab => true,
            KeyCode::PageUp => {
                self.move_palette_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.move_palette_selection(10);
                true
            }
            KeyCode::Home => {
                self.palette_selected = 0;
                true
            }
            KeyCode::End => {
                self.palette_selected = self.palette_filtered.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.move_palette_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_palette_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_palette_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_palette_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.move_palette_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.move_palette_selection(1);
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_palette_filter);
                true
            }
            _ => false,
        }
    }

    fn update_palette_filter(&mut self) {
        let filter_length = self.palette_input.len();
        let rows = super::palette_controller::compute_palette_rows(self, &self.palette_input);
        self.palette_filtered = rows.into_iter().map(|row| row.value).collect();
        self.palette_selected = 0;
        self.palette_log
            .push(super::palette_controller::PaletteLogEntry {
                command_id: String::new(),
                dialog_state: super::palette_controller::PaletteDialogState::Filtered,
                dispatch_target: "lifecycle",
                status: super::palette_controller::PaletteLogStatus::Success,
                availability_reason: None,
                filter_length,
                error_kind: None,
                session_id_redacted: super::palette_controller::redacted_session_id(self),
                provider_id_redacted: super::palette_controller::redacted_provider_id(self),
                model_id_redacted: super::palette_controller::redacted_model_id(self),
            });
    }

    fn move_palette_selection(&mut self, delta: isize) {
        let len = self.palette_filtered.len();
        if len == 0 {
            self.palette_selected = 0;
            return;
        }

        let current = isize::try_from(self.palette_selected).unwrap_or(isize::MAX);
        let mut next = current + delta;
        while next < 0 {
            next += isize::try_from(len).unwrap_or(isize::MAX);
        }
        next %= isize::try_from(len).unwrap_or(isize::MAX);
        self.palette_selected = usize::try_from(next).unwrap_or(usize::MAX);
        self.palette_log
            .push(super::palette_controller::PaletteLogEntry {
                command_id: String::new(),
                dialog_state: super::palette_controller::PaletteDialogState::Selected,
                dispatch_target: "lifecycle",
                status: super::palette_controller::PaletteLogStatus::Success,
                availability_reason: None,
                filter_length: self.palette_input.len(),
                error_kind: None,
                session_id_redacted: super::palette_controller::redacted_session_id(self),
                provider_id_redacted: super::palette_controller::redacted_provider_id(self),
                model_id_redacted: super::palette_controller::redacted_model_id(self),
            });
    }

    fn execute_palette_command(&mut self) {
        let Some(cmd) = self.palette_filtered.get(self.palette_selected) else {
            self.close_palette();
            return;
        };

        let cmd = cmd.clone();
        let filter_length = self.palette_input.len();
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.palette_selected = 0;
        self.palette_visible = false;

        let log_len_before = self.palette_log.len();
        super::palette_controller::dispatch_palette_command(self, &cmd);

        for entry in self.palette_log.iter_mut().skip(log_len_before) {
            entry.filter_length = filter_length;
        }

        if self.session_history_visible
            || self.model_switcher_visible
            || self.toggles_menu_visible
            || self.lineage_browser_visible
            || self.fork_selector_visible
            || self.session_rename_visible
        {
            self.palette_visible = true;
        } else {
            self.close_palette();
        }
    }

    pub(in crate::app) fn close_palette(&mut self) {
        self.palette_log
            .push(super::palette_controller::PaletteLogEntry {
                command_id: String::new(),
                dialog_state: super::palette_controller::PaletteDialogState::Closed,
                dispatch_target: "lifecycle",
                status: super::palette_controller::PaletteLogStatus::Success,
                availability_reason: None,
                filter_length: self.palette_input.len(),
                error_kind: None,
                session_id_redacted: super::palette_controller::redacted_session_id(self),
                provider_id_redacted: super::palette_controller::redacted_provider_id(self),
                model_id_redacted: super::palette_controller::redacted_model_id(self),
            });
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.toggles_menu_visible = false;
        self.lineage_browser_visible = false;
        self.fork_selector_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered.clear();
        self.session_history_filtered.clear();
        self.model_filtered.clear();
        self.toggles_selected = 0;
        self.toggles_yolo_confirm_visible = false;
        self.palette_selected = 0;
        self.session_history_selected = 0;
        self.model_selected = 0;
        if let Some(previous_focus) = self.palette_focus_return.take() {
            self.focus = previous_focus;
        }
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn open_palette(&mut self) {
        if !self.palette_visible {
            self.palette_focus_return = Some(self.focus);
        }
        self.palette_visible = true;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.toggles_menu_visible = false;
        self.toggles_yolo_confirm_visible = false;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.palette_filtered = super::palette_controller::compute_palette_rows(self, "")
            .into_iter()
            .map(|row| row.value)
            .collect();
        self.session_history_filtered.clear();
        self.model_filtered.clear();
        self.palette_selected = 0;
        self.palette_log
            .push(super::palette_controller::PaletteLogEntry {
                command_id: String::new(),
                dialog_state: super::palette_controller::PaletteDialogState::Opened,
                dispatch_target: "lifecycle",
                status: super::palette_controller::PaletteLogStatus::Success,
                availability_reason: None,
                filter_length: 0,
                error_kind: None,
                session_id_redacted: super::palette_controller::redacted_session_id(self),
                provider_id_redacted: super::palette_controller::redacted_provider_id(self),
                model_id_redacted: super::palette_controller::redacted_model_id(self),
            });
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn palette_command_available(&self, command_id: &str) -> bool {
        let Some(entry) = crate::keybindings::palette_model::find(command_id) else {
            return false;
        };
        super::palette_controller::is_available(self, entry)
    }

    pub(in crate::app) fn apply_new_session_launcher_selection(&mut self) {
        self.apply_fresh_session_launcher_selection(UiIntent::NewSession);
    }

    pub(in crate::app) fn request_new_worktree_session(&mut self) {
        self.open_new_worktree_dialog();
    }

    pub(in crate::app) fn apply_fresh_session_launcher_selection(&mut self, intent: UiIntent) {
        let lifecycle_exit = self.startup_mode
            || self.post_run_handoff_visible()
            || self.completed_session_shell_active();
        let prompt_buffer = self.composer.prompt_buffer.clone();
        let prompt_cursor = self.composer.prompt_cursor;
        set_pending_live_prompt_draft(Some(prompt_buffer.clone()));
        set_pending_live_launch_metadata(self.launch_metadata.clone());

        self.projection.reset();
        self.selected_event_index = 0;
        self.transcript_view.selected_activity_index = 0;
        self.transcript_view.follow_mode = true;
        self.details_scroll = 0;
        self.transcript_view.transcript_scroll = 0;
        self.status_banner = None;
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.permission_prompt.permission_id = None;
        self.permission_prompt.stage = PermissionModalStage::Decision;
        self.permission_prompt.selection = PermissionModalSelection::AllowAlways;
        self.permission_prompt.confirm_selection = PermissionConfirmSelection::Confirm;
        self.question_prompt = super::QuestionPromptState::default();
        self.composer.prompt_history.clear();
        self.composer.prompt_history_index = None;
        self.replay_mode = false;
        self.session_path = None;
        self.continued_post_run_handoff_active = false;
        self.continued_live_reopen_surface_active = false;
        self.active_tab = Tab::Run;
        self.live_details_drawer_open = false;
        self.continue_disabled_banner = None;

        self.composer.prompt_buffer = prompt_buffer;
        self.composer.prompt_cursor =
            prompt_cursor.min(self.composer.prompt_buffer.chars().count());

        self.close_session_history();
        self.emit_ui_intent(intent);
        if lifecycle_exit {
            self.should_quit = true;
        }
    }

    pub(in crate::app) fn select_previous_startup_launcher_action(&mut self) {
        self.welcome
            .handle(crate::welcome_surface::WelcomeInput::MoveUp);
        self.continue_disabled_banner = None;
    }

    pub(in crate::app) fn select_next_startup_launcher_action(&mut self) {
        self.welcome
            .handle(crate::welcome_surface::WelcomeInput::MoveDown);
        self.continue_disabled_banner = None;
    }

    pub(in crate::app) fn execute_startup_launcher_action(&mut self) {
        match self.welcome.selected_action() {
            Some(crate::welcome_surface::WelcomeAction::NewWorktree) => {
                self.request_new_worktree_session();
            }
            Some(crate::welcome_surface::WelcomeAction::ResumeSession) => {
                self.begin_session_history_picker(StartupLauncherAction::ContinueSession);
            }
            Some(crate::welcome_surface::WelcomeAction::Changelog) => {
                self.open_release_notes();
            }
            Some(crate::welcome_surface::WelcomeAction::Quit) => self.should_quit = true,
            None => {}
        }
    }

    pub(in crate::app) fn open_release_notes(&mut self) {
        if self.release_notes_visible {
            return;
        }
        self.release_notes_focus_return = Some(self.focus);
        self.release_notes_visible = true;
        self.release_notes_scroll = 0;
        self.modal_interaction.invalidate();
    }

    pub(in crate::app) fn close_release_notes(&mut self) {
        if !self.release_notes_visible {
            return;
        }
        self.release_notes_visible = false;
        self.release_notes_scroll = 0;
        if let Some(focus) = self.release_notes_focus_return.take() {
            self.focus = focus;
        }
        self.modal_interaction.invalidate();
    }

    pub(in crate::app) fn select_previous_post_run_handoff_action(&mut self) {
        let actions = self.post_run_handoff_actions();
        let current = self.selected_post_run_handoff_action();
        let current_index = actions
            .iter()
            .position(|action| *action == current)
            .unwrap_or(0);
        let previous_index = if current_index == 0 {
            actions.len().saturating_sub(1)
        } else {
            current_index - 1
        };
        self.post_run_handoff_action = actions[previous_index];
    }

    pub(in crate::app) fn select_next_post_run_handoff_action(&mut self) {
        let actions = self.post_run_handoff_actions();
        let current = self.selected_post_run_handoff_action();
        let current_index = actions
            .iter()
            .position(|action| *action == current)
            .unwrap_or(0);
        let next_index = if current_index + 1 >= actions.len() {
            0
        } else {
            current_index + 1
        };
        self.post_run_handoff_action = actions[next_index];
    }

    pub(in crate::app) fn execute_post_run_handoff_action(&mut self) {
        match self.selected_post_run_handoff_action() {
            PostRunHandoffAction::ContinueSession => {
                if self.continued_post_run_handoff_active {
                    self.continued_post_run_handoff_active = false;
                    self.continued_live_reopen_surface_active = true;
                    self.active_tab = Tab::Run;
                    self.focus = Focus::Prompt;
                    return;
                }
                let Some((run_id, run_dir)) = self.post_run_reopen_target() else {
                    self.reset_post_run_handoff_selection();
                    return;
                };
                set_pending_live_prompt_draft(Some(self.composer.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ContinueSession {
                    run_id: run_id.to_string(),
                    run_dir: run_dir.clone(),
                });
                self.should_quit = true;
            }
            PostRunHandoffAction::ReplayRun => {
                let Some((run_id, run_dir)) = self.post_run_reopen_target() else {
                    self.reset_post_run_handoff_selection();
                    return;
                };
                set_pending_live_prompt_draft(Some(self.composer.prompt_buffer.clone()));
                self.emit_ui_intent(UiIntent::ReplaySession {
                    run_id: run_id.to_string(),
                    run_dir: run_dir.clone(),
                });
                self.should_quit = true;
            }
            PostRunHandoffAction::StartAnotherSession => {
                self.apply_new_session_launcher_selection();
            }
            PostRunHandoffAction::Quit => {
                self.should_quit = true;
                self.emit_ui_intent(UiIntent::QuitRequested);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::{EventEnvelopeV1, EventV1};
    use std::path::PathBuf;

    fn actor(
        kind: harness_core::event::ActorKind,
        agent_id: &str,
    ) -> harness_core::event::EventActor {
        harness_core::event::EventActor::new(kind, Some(agent_id.to_string()))
    }

    fn event(
        seq: u64,
        correlation_id: Option<&str>,
        actor: harness_core::event::EventActor,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_writer_lock_{seq:04}"),
            seq,
            run_id: "run_dash_consistency".into(),
            mono_ms: seq,
            ts: None,
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: Some("run:run_dash_consistency".to_string()),
            payload,
        }
    }

    fn user_message(seq: u64, req_id: &str, text: &str) -> EventEnvelopeV1 {
        event(
            seq,
            Some(req_id),
            actor(harness_core::event::ActorKind::User, "interactive-user"),
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: req_id.into(),
                text: text.to_string(),
            }),
        )
    }

    fn provider_started(seq: u64, req_id: &str) -> EventEnvelopeV1 {
        event(
            seq,
            Some(req_id),
            actor(harness_core::event::ActorKind::Worker, "agent_parent"),
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: req_id.into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "prompt".to_string(),
                request_digest: format!("digest-{req_id}"),
                metadata: None,
            }),
        )
    }

    fn run_started(seq: u64, run_name: &str) -> EventEnvelopeV1 {
        event(
            seq,
            None,
            actor(harness_core::event::ActorKind::System, "dash-consistency"),
            EventV1::RunStarted(harness_core::event::RunStartedEvent {
                run_name: run_name.into(),
                workspace_root: "/workspace".to_string(),
            }),
        )
    }

    // -----------------------------------------------------------------------
    // Relocated from dashboard_queue_worktree_consistency_test.rs (private API).
    // These scenarios exercise private `lineage_write_blocked_reason` and
    // pub(crate) `active_turn_in_progress` state.
    // -----------------------------------------------------------------------

    #[test]
    fn active_writer_lock_blocks_fork_during_active_turn() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-dash-consistency/parent_run"));
        app.ingest_event(user_message(1, "req_active", "active turn"));
        app.ingest_event(provider_started(2, "req_active"));
        // assert
        assert!(
            app.active_turn_in_progress(),
            "active turn must be in progress after provider started"
        );
        let blocked = app.lineage_write_blocked_reason();
        assert_eq!(
            blocked,
            Some("active"),
            "fork must be blocked during active turn"
        );
    }

    #[test]
    fn active_writer_lock_allows_fork_when_idle() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-dash-consistency/parent_run"));
        // assert
        assert!(!app.active_turn_in_progress(), "no active turn when idle");
        let blocked = app.lineage_write_blocked_reason();
        assert!(blocked.is_none(), "fork must not be blocked when idle");
    }

    #[test]
    fn active_writer_lock_blocks_clone_in_replay_mode() {
        // arrange
        // act
        let app = AppState::new_replay(
            PathBuf::from("/tmp/harness-dash-consistency/replay_run"),
            vec![run_started(1, "replay_run")],
        );
        let blocked = app.lineage_write_blocked_reason();
        // assert
        assert_eq!(
            blocked,
            Some("replay"),
            "clone must be blocked in replay mode"
        );
    }
}
