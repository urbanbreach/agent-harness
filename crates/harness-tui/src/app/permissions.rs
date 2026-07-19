// allow: SIZE_OK — TUI app state (session projection + interaction)
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::PermissionDecision as EventPermissionDecision;
use harness_core::perm::{PermissionDecision, PermissionGrantScope};

#[cfg(test)]
use harness_core::event::{ActorKind, EventEnvelopeV1, EventV1};

#[cfg(test)]
use super::OverlayKind;
use super::{Action, AppState, UiIntent};

mod modal;
mod question;

pub(crate) use modal::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
pub(super) use question::permission_display_summary;
use question::{
    build_question_answers, parse_question_prompts, question_prompt_choice_count,
    question_prompt_confirm_active, question_prompt_is_single_select, question_prompt_tab_count,
};
pub use question::{QuestionOptionView, QuestionPromptView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionEntry {
    pub permission_id: String,
    pub kind: String,
    pub tool_call_id: Option<String>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: EventPermissionDecision,
    pub resolved_decision: Option<EventPermissionDecision>,
    pub resolution_reason: Option<String>,
    pub first_seq: u64,
    pub last_seq: u64,
}

impl PermissionEntry {
    pub(crate) fn mark_resolved(
        &mut self,
        decision: EventPermissionDecision,
        reason: Option<&str>,
        seq: u64,
    ) {
        self.resolved_decision = Some(decision);
        self.resolution_reason = reason.map(str::to_owned);
        self.last_seq = seq;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingPermission {
    pub(crate) seq: u64,
    pub(crate) kind: String,
    pub(crate) summary: String,
    pub(crate) request_digest: String,
    pub(crate) timeout_ms: u64,
    pub(crate) default_decision: EventPermissionDecision,
    pub(crate) tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePermissionView {
    pub permission_id: String,
    pub kind: String,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: EventPermissionDecision,
    pub tool_call_id: Option<String>,
    pub tool_label: Option<String>,
    pub question_prompts: Option<Vec<QuestionPromptView>>,
}

impl AppState {
    fn permission_modal_is_active(&self, permission_id: &str) -> bool {
        self.permission_prompt.permission_id.as_deref() == Some(permission_id)
    }

    fn question_answer_is_active(&self, permission_id: &str) -> bool {
        self.question_prompt.permission_id.as_deref() == Some(permission_id)
    }

    pub(super) fn submitted_permission_is_active(&self, permission_id: &str) -> bool {
        self.submitted_permission_id.as_deref() == Some(permission_id)
    }

    pub fn active_permission(&self) -> Option<(String, String)> {
        self.projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .min_by_key(|(_, pending)| pending.seq)
            .map(|(permission_id, pending)| (permission_id.clone(), pending.summary.clone()))
    }

    pub fn active_permission_view(&self) -> Option<ActivePermissionView> {
        self.projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .min_by_key(|(_, pending)| pending.seq)
            .map(|(permission_id, pending)| ActivePermissionView {
                permission_id: permission_id.clone(),
                kind: pending.kind.clone(),
                summary: pending.summary.clone(),
                request_digest: pending.request_digest.clone(),
                timeout_ms: pending.timeout_ms,
                default_decision: pending.default_decision,
                tool_call_id: pending.tool_call_id.clone(),
                tool_label: pending
                    .tool_call_id
                    .as_deref()
                    .and_then(|tool_call_id| self.tool_label_for_call(tool_call_id)),
                question_prompts: parse_question_prompts(&pending.kind, &pending.summary),
            })
    }

    fn tool_label_for_call(&self, tool_call_id: &str) -> Option<String> {
        self.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .find(|tool_call| tool_call.tool_call_id.as_str() == tool_call_id)
            .map(|tool_call| tool_call.tool_id.clone())
    }

    pub fn transcript_pending_permissions(&self) -> Vec<(String, String)> {
        let mut pending = self
            .projection
            .pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .map(|(permission_id, permission)| {
                let summary = if permission.kind.eq_ignore_ascii_case("question") {
                    "Question requested".to_string()
                } else {
                    permission.summary.clone()
                };
                (permission.seq, permission_id.clone(), summary)
            })
            .collect::<Vec<_>>();
        pending.sort_by_key(|(seq, _, _)| *seq);
        pending
            .into_iter()
            .map(|(_, permission_id, summary)| (permission_id, summary))
            .collect()
    }

    pub fn permission_submission_pending(&self, permission_id: &str) -> bool {
        self.submitted_permission_is_active(permission_id)
    }

    pub(crate) fn permission_modal_selection(
        &self,
        permission_id: &str,
    ) -> PermissionModalSelection {
        if self.permission_modal_is_active(permission_id) {
            self.permission_prompt.selection
        } else {
            PermissionModalSelection::AllowAlways
        }
    }

    pub(crate) fn permission_modal_stage(&self, permission_id: &str) -> PermissionModalStage {
        if self.permission_modal_is_active(permission_id) {
            self.permission_prompt.stage
        } else {
            PermissionModalStage::Decision
        }
    }

    pub(crate) fn permission_modal_confirm_selection(
        &self,
        permission_id: &str,
    ) -> PermissionConfirmSelection {
        if self.permission_modal_is_active(permission_id) {
            self.permission_prompt.confirm_selection
        } else {
            PermissionConfirmSelection::Confirm
        }
    }

    pub(crate) fn question_prompt_tab(&self, permission_id: &str) -> usize {
        if self.question_answer_is_active(permission_id) {
            self.question_prompt.tab
        } else {
            0
        }
    }

    pub(crate) fn question_prompt_selection(&self, permission_id: &str) -> usize {
        if self.question_answer_is_active(permission_id) {
            self.question_prompt.selection
        } else {
            0
        }
    }

    pub(crate) fn question_prompt_editing(&self, permission_id: &str) -> bool {
        self.question_answer_is_active(permission_id) && self.question_prompt.editing
    }

    pub(crate) fn question_prompt_answers(&self, permission_id: &str) -> Vec<Vec<String>> {
        if self.question_answer_is_active(permission_id) {
            self.question_prompt.answers.clone()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn question_prompt_custom(&self, permission_id: &str, index: usize) -> Option<&str> {
        if !self.question_answer_is_active(permission_id) {
            return None;
        }

        self.question_prompt.custom.get(index).map(String::as_str)
    }

    fn cycle_permission_modal_selection(
        &mut self,
        permission_id: &str,
        forward: bool,
        allow_always: bool,
    ) {
        let current = self.permission_modal_selection(permission_id);
        self.permission_prompt.permission_id = Some(permission_id.to_string());
        self.permission_prompt.stage = PermissionModalStage::Decision;
        self.permission_prompt.selection = current.cycle(forward, allow_always);
    }

    fn cycle_permission_modal_confirm_selection(&mut self, permission_id: &str, forward: bool) {
        let current = self.permission_modal_confirm_selection(permission_id);
        self.permission_prompt.permission_id = Some(permission_id.to_string());
        self.permission_prompt.stage = PermissionModalStage::AlwaysConfirm;
        self.permission_prompt.confirm_selection = current.cycle(forward);
    }

    fn open_permission_allow_always_confirm(&mut self, permission_id: &str) {
        self.permission_prompt.permission_id = Some(permission_id.to_string());
        self.permission_prompt.stage = PermissionModalStage::AlwaysConfirm;
        self.permission_prompt.confirm_selection = PermissionConfirmSelection::Confirm;
    }

    fn close_permission_allow_always_confirm(&mut self, permission_id: &str) {
        self.permission_prompt.permission_id = Some(permission_id.to_string());
        self.permission_prompt.stage = PermissionModalStage::Decision;
        self.permission_prompt.confirm_selection = PermissionConfirmSelection::Confirm;
    }

    pub(super) fn clear_permission_modal_selection(&mut self, permission_id: &str) {
        if self.permission_modal_is_active(permission_id) {
            self.permission_prompt.permission_id = None;
            self.permission_prompt.stage = PermissionModalStage::Decision;
            self.permission_prompt.selection = PermissionModalSelection::AllowAlways;
            self.permission_prompt.confirm_selection = PermissionConfirmSelection::Confirm;
        }
    }

    #[cfg(test)]
    pub(crate) fn exact_test_overlay_stack_orders_permission_above_commands_and_slash() {
        fn permission_event(seq: u64, permission_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
            EventEnvelopeV1 {
                schema_version: harness_core::event::SCHEMA_VERSION,
                event_id: format!("evt_permission_overlay_{seq:04}"),
                seq,
                run_id: "run_overlay_stack_exact".into(),
                mono_ms: seq,
                ts: Some("2026-02-03T12:00:00Z".to_string()),
                actor: harness_core::event::EventActor::new(
                    ActorKind::System,
                    Some("overlay-stack-exact".to_string()),
                ),
                correlation_id: Some(permission_id.to_string()),
                causation_id: None,
                stream_key: None,
                payload: EventV1::PermissionRequested(
                    harness_core::event::PermissionRequestedEvent {
                        permission_id: permission_id.to_string(),
                        kind: "edit_fs".to_string(),
                        tool_call_id: Some(tool_call_id.into()),
                        summary: "permission summary".to_string(),
                        request_digest: format!("digest-{permission_id}"),
                        timeout_ms: 30_000,
                        default_decision: harness_core::event::PermissionDecision::Deny,
                    },
                ),
            }
        }

        let mut palette_app = AppState::new_live(None, false, None);
        palette_app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(
            palette_app.overlay_stack().top(),
            Some(OverlayKind::CommandPalette)
        );

        palette_app.ingest_event(permission_event(
            1,
            "perm_overlay_priority_palette",
            "tc_overlay_priority_palette",
        ));

        assert_eq!(
            palette_app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );
        assert!(!palette_app.palette_visible);

        let mut slash_app = AppState::new_live(None, false, None);
        slash_app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(slash_app.slash_visible);
        assert_eq!(
            slash_app.overlay_stack().top(),
            Some(OverlayKind::SlashCommands)
        );

        slash_app.ingest_event(permission_event(
            1,
            "perm_overlay_priority_slash",
            "tc_overlay_priority_slash",
        ));

        assert!(!slash_app.slash_visible);
        assert_eq!(
            slash_app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );

        slash_app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(slash_app.composer.prompt_buffer, "/");
        assert!(!slash_app.slash_visible);
        assert_eq!(
            slash_app.overlay_stack().top(),
            Some(OverlayKind::PermissionModal)
        );
    }

    pub(super) fn handle_permission_modal_key(&mut self, key: KeyEvent) {
        let Some(permission) = self.active_permission_view() else {
            return;
        };

        if permission.question_prompts.is_some() {
            self.handle_question_permission_modal_key(key);
            return;
        }

        if !key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT)
        {
            if self.permission_modal_stage(&permission.permission_id)
                == PermissionModalStage::AlwaysConfirm
            {
                match key.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        self.cycle_permission_modal_confirm_selection(
                            &permission.permission_id,
                            false,
                        );
                        return;
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        self.cycle_permission_modal_confirm_selection(
                            &permission.permission_id,
                            true,
                        );
                        return;
                    }
                    KeyCode::Enter => {
                        if self.permission_modal_confirm_selection(&permission.permission_id)
                            == PermissionConfirmSelection::Confirm
                        {
                            self.enable_always_approve_mode();
                            self.clear_permission_modal_selection(&permission.permission_id);
                            self.send_permission_intent(
                                permission.permission_id.clone(),
                                PermissionDecision::Allow,
                                None,
                                Some(PermissionGrantScope::Run),
                            );
                        } else {
                            self.close_permission_allow_always_confirm(&permission.permission_id);
                        }
                        self.maybe_auto_exit();
                        return;
                    }
                    KeyCode::Esc => {
                        self.close_permission_allow_always_confirm(&permission.permission_id);
                        return;
                    }
                    _ => return,
                }
            }

            match key.code {
                KeyCode::Left | KeyCode::Char('h') => {
                    self.cycle_permission_modal_selection(&permission.permission_id, false, true);
                    return;
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    self.cycle_permission_modal_selection(&permission.permission_id, true, true);
                    return;
                }
                KeyCode::Enter => {
                    match self.permission_modal_selection(&permission.permission_id) {
                        PermissionModalSelection::AllowOnce => {
                            self.execute_action(Action::AllowPermission);
                        }
                        PermissionModalSelection::AllowSession => {
                            // Product-honest session grant (freeze option 2).
                            // Scope=Session records a durable grant for this request's
                            // kind/tool/matcher for the remainder of the session.
                            self.clear_permission_modal_selection(&permission.permission_id);
                            self.send_permission_intent(
                                permission.permission_id.clone(),
                                PermissionDecision::Allow,
                                None,
                                Some(PermissionGrantScope::Session),
                            );
                        }
                        PermissionModalSelection::AllowAlways => {
                            self.open_permission_allow_always_confirm(&permission.permission_id);
                        }
                        PermissionModalSelection::Reject => {
                            self.execute_action(Action::DismissModal);
                        }
                    }
                    self.maybe_auto_exit();
                    return;
                }
                _ => {}
            }
        }

        if let Some(action) = self.keymap.get_action(&key) {
            if matches!(
                action,
                Action::AllowPermission
                    | Action::AlwaysApprovePermission
                    | Action::DenyPermission
                    | Action::DismissModal
            ) {
                self.execute_action(action);
                self.maybe_auto_exit();
            }
        }
    }

    fn handle_question_permission_modal_key(&mut self, key: KeyEvent) {
        let Some(permission) = self.active_permission_view() else {
            return;
        };
        let Some(prompts) = permission.question_prompts.as_ref() else {
            return;
        };
        self.ensure_question_answer_state(&permission.permission_id, prompts);
        let single = question_prompt_is_single_select(prompts);
        let tab_count = question_prompt_tab_count(prompts);
        let confirm = question_prompt_confirm_active(self.question_prompt.tab, prompts);

        if !self.composer_disabled()
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            if self.question_prompt.editing {
                match key.code {
                    KeyCode::Esc => {
                        self.question_prompt.editing = false;
                        return;
                    }
                    KeyCode::Enter => {
                        // allow: WIDENING — char is ASCII digit, lower byte is the digit value
                        self.commit_question_custom_answer(&permission.permission_id, prompts);
                        self.maybe_auto_exit();
                        return;
                    }
                    KeyCode::Backspace => {
                        self.backspace_question_answer_char();
                        self.maybe_auto_exit();
                        return;
                    }
                    KeyCode::Delete => {
                        self.delete_question_answer_char();
                        self.maybe_auto_exit();
                        return;
                    }
                    KeyCode::Left => {
                        self.question_prompt.answer_cursor =
                            self.question_prompt.answer_cursor.saturating_sub(1);
                        return;
                    }
                    KeyCode::Right => {
                        self.question_prompt.answer_cursor = self
                            .question_prompt
                            .answer_cursor
                            .saturating_add(1)
                            .min(self.question_answer_char_count());
                        return;
                    }
                    KeyCode::Home => {
                        self.question_prompt.answer_cursor = 0;
                        return;
                    }
                    KeyCode::End => {
                        self.question_prompt.answer_cursor = self.question_answer_char_count();
                        return;
                    }
                    KeyCode::Char(c) => {
                        self.insert_question_answer_char(c);
                        self.maybe_auto_exit();
                        return;
                    }
                    _ => return,
                }
            }

            match key.code {
                KeyCode::Left | KeyCode::Char('h') => {
                    if !single {
                        self.select_question_tab(
                            (self.question_prompt.tab + tab_count - 1) % tab_count,
                        );
                        return;
                    }
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                    if !single {
                        self.select_question_tab((self.question_prompt.tab + 1) % tab_count);
                        return;
                    }
                }
                KeyCode::BackTab => {
                    if !single {
                        self.select_question_tab(
                            (self.question_prompt.tab + tab_count - 1) % tab_count,
                        );
                        return;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if !confirm {
                        self.move_question_selection(prompts, -1);
                        return;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !confirm {
                        self.move_question_selection(prompts, 1);
                        return;
                    }
                }
                KeyCode::Char(c @ '1'..='9') => {
                    if !confirm {
                        let index = usize::from((c as u8).saturating_sub(b'1')); // allow: WIDENING — char is ASCII digit, lower byte is the digit value
                        let Some(prompt) = prompts.get(self.question_prompt.tab) else {
                            return;
                        };
                        let max = question_prompt_choice_count(prompt).min(9);
                        if index >= max {
                            return;
                        }
                        self.question_prompt.selection = index;
                        self.activate_question_selection(&permission.permission_id, prompts);
                        self.maybe_auto_exit();
                        return;
                    }
                }
                KeyCode::Enter => {
                    if confirm {
                        self.execute_action(Action::AllowPermission);
                    } else {
                        self.activate_question_selection(&permission.permission_id, prompts);
                    }
                    self.maybe_auto_exit();
                    return;
                }
                KeyCode::Esc => {
                    self.execute_action(Action::DismissModal);
                    self.maybe_auto_exit();
                    return;
                }
                _ => {}
            }
        }

        if let Some(action) = self.keymap.get_action(&key) {
            if matches!(
                action,
                Action::AllowPermission
                    | Action::AlwaysApprovePermission
                    | Action::DenyPermission
                    | Action::DismissModal
            ) {
                self.execute_action(action);
                self.maybe_auto_exit();
            }
        }
    }

    pub(super) fn execute_permission_action(&mut self, action: Action) -> bool {
        let Some((permission_id, _)) = self.active_permission() else {
            return false;
        };

        match action {
            Action::AllowPermission => {
                let reason = self
                    .active_permission_view()
                    .and_then(|permission| self.build_question_permission_reason(&permission));
                if self.active_permission_view().is_some_and(|permission| {
                    permission.question_prompts.is_some() && reason.is_none()
                }) {
                    return true;
                }
                self.clear_permission_modal_selection(&permission_id);
                self.send_permission_intent(permission_id, PermissionDecision::Allow, reason, None);
                true
            }
            Action::AlwaysApprovePermission => {
                self.open_permission_allow_always_confirm(&permission_id);
                true
            }
            Action::DenyPermission => {
                self.clear_permission_modal_selection(&permission_id);
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None, None);
                true
            }
            Action::DismissModal => {
                self.clear_permission_modal_selection(&permission_id);
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None, None);
                true
            }
            Action::Quit => {
                self.should_quit = true;
                self.emit_ui_intent(UiIntent::QuitRequested);
                true
            }
            _ => true,
        }
    }

    fn _handle_modal_key(&mut self, key: KeyEvent) -> bool {
        let Some((permission_id, _)) = self.active_permission() else {
            return false;
        };

        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                self.send_permission_intent(permission_id, PermissionDecision::Allow, None, None);
                true
            }
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None, None);
                true
            }
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None, None);
                true
            }
            _ => false,
        }
    }

    fn ensure_question_answer_state(
        &mut self,
        permission_id: &str,
        prompts: &[QuestionPromptView],
    ) {
        if self.question_answer_is_active(permission_id) {
            return;
        }

        self.question_prompt.permission_id = Some(permission_id.to_string());
        self.question_prompt.tab = 0;
        self.question_prompt.selection = 0;
        self.question_prompt.answers = vec![Vec::new(); prompts.len()];
        self.question_prompt.custom = vec![String::new(); prompts.len()];
        self.question_prompt.editing = false;
        self.question_prompt.answer_buffer.clear();
        self.question_prompt.answer_cursor = 0;
        self.question_prompt.answer_error = None;
    }

    fn select_question_tab(&mut self, tab: usize) {
        self.question_prompt.tab = tab;
        self.question_prompt.selection = 0;
        self.question_prompt.editing = false;
        self.question_prompt.answer_buffer.clear();
        self.question_prompt.answer_cursor = 0;
        self.question_prompt.answer_error = None;
    }

    fn move_question_selection(&mut self, prompts: &[QuestionPromptView], delta: isize) {
        let Some(prompt) = prompts.get(self.question_prompt.tab) else {
            return;
        };
        let total = question_prompt_choice_count(prompt);
        if total == 0 {
            self.question_prompt.selection = 0;
            return;
        }
        self.question_prompt.selection = if delta < 0 {
            if self.question_prompt.selection == 0 {
                total.saturating_sub(1)
            } else {
                self.question_prompt.selection - 1
            }
        } else if self.question_prompt.selection + 1 >= total {
            0
        } else {
            self.question_prompt.selection + 1
        };
    }

    fn activate_question_selection(&mut self, permission_id: &str, prompts: &[QuestionPromptView]) {
        let Some(prompt) = prompts.get(self.question_prompt.tab) else {
            return;
        };
        let total = question_prompt_choice_count(prompt);
        if total == 0 {
            return;
        }
        self.question_prompt.selection =
            self.question_prompt.selection.min(total.saturating_sub(1));
        if prompt.custom && self.question_prompt.selection == prompt.options.len() {
            if prompt.multiple {
                let current = self
                    .question_prompt
                    .custom
                    .get(self.question_prompt.tab)
                    .cloned()
                    .unwrap_or_default();
                if !current.is_empty()
                    && self.question_prompt.answers[self.question_prompt.tab]
                        .iter()
                        .any(|value| value == &current)
                {
                    self.toggle_question_answer(self.question_prompt.tab, &current);
                    return;
                }
            }
            self.start_question_custom_edit(permission_id);
            return;
        }

        let Some(option) = prompt.options.get(self.question_prompt.selection) else {
            return;
        };
        let answer = option.label.clone();
        if prompt.multiple {
            self.toggle_question_answer(self.question_prompt.tab, &answer);
            return;
        }

        self.question_prompt.answers[self.question_prompt.tab] = vec![answer];
        if question_prompt_is_single_select(prompts) {
            self.execute_action(Action::AllowPermission);
            return;
        }

        self.select_question_tab(
            (self.question_prompt.tab + 1)
                .min(question_prompt_tab_count(prompts).saturating_sub(1)),
        );
    }

    fn start_question_custom_edit(&mut self, permission_id: &str) {
        let Some(current) = self
            .question_prompt
            .custom
            .get(self.question_prompt.tab)
            .cloned()
        else {
            return;
        };
        self.question_prompt.permission_id = Some(permission_id.to_string());
        self.question_prompt.editing = true;
        self.question_prompt.answer_buffer = current;
        self.question_prompt.answer_cursor = self.question_answer_char_count();
        self.question_prompt.answer_error = None;
    }

    fn commit_question_custom_answer(
        &mut self,
        permission_id: &str,
        prompts: &[QuestionPromptView],
    ) {
        let Some(prompt) = prompts.get(self.question_prompt.tab) else {
            return;
        };
        let answer = self.question_prompt.answer_buffer.trim().to_string();
        let previous = self
            .question_prompt
            .custom
            .get(self.question_prompt.tab)
            .cloned()
            .unwrap_or_default();
        self.question_prompt.editing = false;
        self.question_prompt.answer_error = None;

        if answer.is_empty() {
            self.clear_question_custom_answer(self.question_prompt.tab, &previous);
            return;
        }

        self.question_prompt.custom[self.question_prompt.tab] = answer.clone();
        if prompt.multiple {
            self.replace_question_custom_answer(self.question_prompt.tab, &previous, answer);
            return;
        }

        self.question_prompt.answers[self.question_prompt.tab] = vec![answer];
        if question_prompt_is_single_select(prompts) {
            self.execute_action(Action::AllowPermission);
            return;
        }

        self.select_question_tab(
            (self.question_prompt.tab + 1)
                .min(question_prompt_tab_count(prompts).saturating_sub(1)),
        );
        self.question_prompt.permission_id = Some(permission_id.to_string());
    }

    fn toggle_question_answer(&mut self, index: usize, answer: &str) {
        let Some(values) = self.question_prompt.answers.get_mut(index) else {
            return;
        };
        if let Some(position) = values.iter().position(|value| value == answer) {
            values.remove(position);
        } else {
            values.push(answer.to_string());
        }
    }

    fn clear_question_custom_answer(&mut self, index: usize, previous: &str) {
        if let Some(value) = self.question_prompt.custom.get_mut(index) {
            value.clear();
        }
        if previous.is_empty() {
            return;
        }
        if let Some(values) = self.question_prompt.answers.get_mut(index) {
            values.retain(|value| value != previous);
        }
    }

    fn replace_question_custom_answer(&mut self, index: usize, previous: &str, answer: String) {
        let Some(values) = self.question_prompt.answers.get_mut(index) else {
            return;
        };
        values.retain(|value| value != previous);
        if !values.iter().any(|value| value == &answer) {
            values.push(answer);
        }
    }

    fn question_answer_char_count(&self) -> usize {
        self.question_prompt.answer_buffer.chars().count()
    }

    fn question_answer_cursor_byte_index(&self) -> usize {
        self.question_prompt
            .answer_buffer
            .char_indices()
            .nth(self.question_prompt.answer_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.question_prompt.answer_buffer.len())
    }

    fn insert_question_answer_char(&mut self, c: char) {
        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_prompt.answer_buffer.insert(byte_idx, c);
        self.question_prompt.answer_cursor += 1;
        self.question_prompt.answer_error = None;
    }

    fn backspace_question_answer_char(&mut self) {
        if self.question_prompt.answer_cursor == 0 {
            return;
        }

        self.question_prompt.answer_cursor -= 1;
        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_prompt.answer_buffer.remove(byte_idx);
        self.question_prompt.answer_error = None;
    }

    fn delete_question_answer_char(&mut self) {
        if self.question_prompt.answer_cursor >= self.question_answer_char_count() {
            return;
        }

        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_prompt.answer_buffer.remove(byte_idx);
        self.question_prompt.answer_error = None;
    }

    fn build_question_permission_reason(
        &mut self,
        permission: &ActivePermissionView,
    ) -> Option<String> {
        let prompts = permission.question_prompts.as_ref()?;
        match build_question_answers(prompts, &self.question_prompt.answers) {
            Ok(answers) => {
                self.question_prompt.answer_error = None;
                serde_json::to_string(&answers).ok()
            }
            Err(err) => {
                self.question_prompt.answer_error = Some(err);
                None
            }
        }
    }

    pub(crate) fn question_answer_preview(&self, permission_id: &str) -> String {
        if !self.question_answer_is_active(permission_id) {
            return String::new();
        }

        if !self.question_prompt.editing {
            return self
                .question_prompt
                .custom
                .get(self.question_prompt.tab)
                .cloned()
                .unwrap_or_default();
        }

        let mut preview = self.question_prompt.answer_buffer.clone();
        let byte_idx = preview
            .char_indices()
            .nth(self.question_prompt.answer_cursor)
            .map(|(index, _)| index)
            .unwrap_or(preview.len());
        preview.insert(byte_idx, '█');
        preview
    }

    pub(crate) fn question_answer_error(&self, permission_id: &str) -> Option<&str> {
        self.question_answer_is_active(permission_id)
            .then_some(self.question_prompt.answer_error.as_deref())
            .flatten()
    }

    pub(super) fn clear_question_answer_state(&mut self, permission_id: &str) {
        if !self.question_answer_is_active(permission_id) {
            return;
        }

        self.question_prompt.permission_id = None;
        self.question_prompt.tab = 0;
        self.question_prompt.selection = 0;
        self.question_prompt.answers.clear();
        self.question_prompt.custom.clear();
        self.question_prompt.editing = false;
        self.question_prompt.answer_buffer.clear();
        self.question_prompt.answer_cursor = 0;
        self.question_prompt.answer_error = None;
    }

    pub(super) fn send_permission_intent(
        &mut self,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
    ) {
        if self.submitted_permission_is_active(&permission_id) {
            return;
        }

        self.emit_ui_intent(UiIntent::ResolvePermission {
            permission_id: permission_id.clone(),
            decision,
            reason,
            grant_scope,
        });
        self.submitted_permission_id = Some(permission_id);
    }

    pub(super) fn maybe_auto_allow_active_permission(&mut self) {
        if !self.always_approve_mode() {
            return;
        }
        let Some(permission) = self.active_permission_view() else {
            return;
        };
        if permission.question_prompts.is_some() {
            return;
        }
        if self.submitted_permission_is_active(&permission.permission_id) {
            return;
        }
        self.clear_permission_modal_selection(&permission.permission_id);
        self.send_permission_intent(
            permission.permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        );
    }
}
