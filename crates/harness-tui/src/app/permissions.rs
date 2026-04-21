use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::PermissionDecision as EventPermissionDecision;
use harness_core::perm::PermissionDecision;

#[cfg(test)]
use harness_core::event::{ActorKind, EventEnvelopeV1, EventV1};

#[cfg(test)]
use super::OverlayKind;
use super::{Action, AppState, UiIntent};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionPromptView {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOptionView>,
    pub multiple: bool,
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOptionView {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionModalSelection {
    #[default]
    AllowOnce,
    AllowAlways,
    Reject,
}

impl PermissionModalSelection {
    fn cycle(self, forward: bool, allow_always: bool) -> Self {
        let options = if allow_always {
            [Self::AllowOnce, Self::AllowAlways, Self::Reject].as_slice()
        } else {
            [Self::AllowOnce, Self::Reject].as_slice()
        };
        let current = options
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % options.len()
        } else {
            (current + options.len() - 1) % options.len()
        };
        options[next]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionConfirmSelection {
    #[default]
    Confirm,
    Cancel,
}

impl PermissionConfirmSelection {
    fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Confirm, true) | (Self::Cancel, false) => Self::Cancel,
            (Self::Cancel, true) | (Self::Confirm, false) => Self::Confirm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionModalStage {
    #[default]
    Decision,
    AlwaysConfirm,
}

impl AppState {
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
            .find(|tool_call| tool_call.tool_call_id == tool_call_id)
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
        self.submitted_permission_id.as_deref() == Some(permission_id)
    }

    pub(crate) fn permission_modal_selection(
        &self,
        permission_id: &str,
    ) -> PermissionModalSelection {
        if self.permission_modal_permission_id.as_deref() == Some(permission_id) {
            self.permission_modal_selection
        } else {
            PermissionModalSelection::AllowOnce
        }
    }

    pub(crate) fn permission_modal_stage(&self, permission_id: &str) -> PermissionModalStage {
        if self.permission_modal_permission_id.as_deref() == Some(permission_id) {
            self.permission_modal_stage
        } else {
            PermissionModalStage::Decision
        }
    }

    pub(crate) fn permission_modal_confirm_selection(
        &self,
        permission_id: &str,
    ) -> PermissionConfirmSelection {
        if self.permission_modal_permission_id.as_deref() == Some(permission_id) {
            self.permission_modal_confirm_selection
        } else {
            PermissionConfirmSelection::Confirm
        }
    }

    pub(crate) fn question_prompt_tab(&self, permission_id: &str) -> usize {
        if self.question_answer_permission_id.as_deref() == Some(permission_id) {
            self.question_prompt_tab
        } else {
            0
        }
    }

    pub(crate) fn question_prompt_selection(&self, permission_id: &str) -> usize {
        if self.question_answer_permission_id.as_deref() == Some(permission_id) {
            self.question_prompt_selection
        } else {
            0
        }
    }

    pub(crate) fn question_prompt_editing(&self, permission_id: &str) -> bool {
        self.question_answer_permission_id.as_deref() == Some(permission_id)
            && self.question_prompt_editing
    }

    pub(crate) fn question_prompt_answers(&self, permission_id: &str) -> Vec<Vec<String>> {
        if self.question_answer_permission_id.as_deref() == Some(permission_id) {
            self.question_prompt_answers.clone()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn question_prompt_custom(&self, permission_id: &str, index: usize) -> Option<&str> {
        if self.question_answer_permission_id.as_deref() != Some(permission_id) {
            return None;
        }

        self.question_prompt_custom.get(index).map(String::as_str)
    }

    fn cycle_permission_modal_selection(
        &mut self,
        permission_id: &str,
        forward: bool,
        allow_always: bool,
    ) {
        let current = self.permission_modal_selection(permission_id);
        self.permission_modal_permission_id = Some(permission_id.to_string());
        self.permission_modal_stage = PermissionModalStage::Decision;
        self.permission_modal_selection = current.cycle(forward, allow_always);
    }

    fn cycle_permission_modal_confirm_selection(&mut self, permission_id: &str, forward: bool) {
        let current = self.permission_modal_confirm_selection(permission_id);
        self.permission_modal_permission_id = Some(permission_id.to_string());
        self.permission_modal_stage = PermissionModalStage::AlwaysConfirm;
        self.permission_modal_confirm_selection = current.cycle(forward);
    }

    fn open_permission_allow_always_confirm(&mut self, permission_id: &str) {
        self.permission_modal_permission_id = Some(permission_id.to_string());
        self.permission_modal_stage = PermissionModalStage::AlwaysConfirm;
        self.permission_modal_confirm_selection = PermissionConfirmSelection::Confirm;
    }

    fn close_permission_allow_always_confirm(&mut self, permission_id: &str) {
        self.permission_modal_permission_id = Some(permission_id.to_string());
        self.permission_modal_stage = PermissionModalStage::Decision;
        self.permission_modal_confirm_selection = PermissionConfirmSelection::Confirm;
    }

    pub(super) fn clear_permission_modal_selection(&mut self, permission_id: &str) {
        if self.permission_modal_permission_id.as_deref() == Some(permission_id) {
            self.permission_modal_permission_id = None;
            self.permission_modal_stage = PermissionModalStage::Decision;
            self.permission_modal_selection = PermissionModalSelection::AllowOnce;
            self.permission_modal_confirm_selection = PermissionConfirmSelection::Confirm;
        }
    }

    #[cfg(test)]
    pub(crate) fn exact_test_overlay_stack_orders_permission_above_commands_and_slash() {
        fn permission_event(seq: u64, permission_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
            EventEnvelopeV1 {
                schema_version: harness_core::event::SCHEMA_VERSION,
                event_id: format!("evt_permission_overlay_{seq:04}"),
                seq,
                run_id: "run_overlay_stack_exact".to_string(),
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
                        tool_call_id: Some(tool_call_id.to_string()),
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
        assert_eq!(slash_app.overlay_stack().top(), None);

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
        assert_eq!(slash_app.prompt_buffer, "/");
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
                            self.always_allowed_permission_digests
                                .insert(permission.request_digest.clone());
                            self.clear_permission_modal_selection(&permission.permission_id);
                            self.send_permission_intent(
                                permission.permission_id.clone(),
                                PermissionDecision::Allow,
                                None,
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
                Action::AllowPermission | Action::DenyPermission | Action::DismissModal
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
        let confirm = question_prompt_confirm_active(self.question_prompt_tab, prompts);

        if !self.composer_disabled()
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            if self.question_prompt_editing {
                match key.code {
                    KeyCode::Esc => {
                        self.question_prompt_editing = false;
                        return;
                    }
                    KeyCode::Enter => {
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
                        self.question_answer_cursor = self.question_answer_cursor.saturating_sub(1);
                        return;
                    }
                    KeyCode::Right => {
                        self.question_answer_cursor = self
                            .question_answer_cursor
                            .saturating_add(1)
                            .min(self.question_answer_char_count());
                        return;
                    }
                    KeyCode::Home => {
                        self.question_answer_cursor = 0;
                        return;
                    }
                    KeyCode::End => {
                        self.question_answer_cursor = self.question_answer_char_count();
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
                            (self.question_prompt_tab + tab_count - 1) % tab_count,
                        );
                        return;
                    }
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                    if !single {
                        self.select_question_tab((self.question_prompt_tab + 1) % tab_count);
                        return;
                    }
                }
                KeyCode::BackTab => {
                    if !single {
                        self.select_question_tab(
                            (self.question_prompt_tab + tab_count - 1) % tab_count,
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
                        let index = usize::from((c as u8).saturating_sub(b'1'));
                        self.question_prompt_selection = index;
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
                Action::AllowPermission | Action::DenyPermission | Action::DismissModal
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
                self.send_permission_intent(permission_id, PermissionDecision::Allow, reason);
                true
            }
            Action::DenyPermission => {
                self.clear_permission_modal_selection(&permission_id);
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None);
                true
            }
            Action::DismissModal => {
                self.clear_permission_modal_selection(&permission_id);
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None);
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
                self.send_permission_intent(permission_id, PermissionDecision::Allow, None);
                true
            }
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None);
                true
            }
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny, None);
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
        if self.question_answer_permission_id.as_deref() == Some(permission_id) {
            return;
        }

        self.question_answer_permission_id = Some(permission_id.to_string());
        self.question_prompt_tab = 0;
        self.question_prompt_selection = 0;
        self.question_prompt_answers = vec![Vec::new(); prompts.len()];
        self.question_prompt_custom = vec![String::new(); prompts.len()];
        self.question_prompt_editing = false;
        self.question_answer_buffer.clear();
        self.question_answer_cursor = 0;
        self.question_answer_error = None;
    }

    fn select_question_tab(&mut self, tab: usize) {
        self.question_prompt_tab = tab;
        self.question_prompt_selection = 0;
        self.question_prompt_editing = false;
        self.question_answer_buffer.clear();
        self.question_answer_cursor = 0;
        self.question_answer_error = None;
    }

    fn move_question_selection(&mut self, prompts: &[QuestionPromptView], delta: isize) {
        let Some(prompt) = prompts.get(self.question_prompt_tab) else {
            return;
        };
        let total = question_prompt_choice_count(prompt);
        if total == 0 {
            self.question_prompt_selection = 0;
            return;
        }
        self.question_prompt_selection = if delta < 0 {
            if self.question_prompt_selection == 0 {
                total.saturating_sub(1)
            } else {
                self.question_prompt_selection - 1
            }
        } else if self.question_prompt_selection + 1 >= total {
            0
        } else {
            self.question_prompt_selection + 1
        };
    }

    fn activate_question_selection(&mut self, permission_id: &str, prompts: &[QuestionPromptView]) {
        let Some(prompt) = prompts.get(self.question_prompt_tab) else {
            return;
        };
        let total = question_prompt_choice_count(prompt);
        if total == 0 {
            return;
        }
        self.question_prompt_selection =
            self.question_prompt_selection.min(total.saturating_sub(1));
        if prompt.custom && self.question_prompt_selection == prompt.options.len() {
            self.start_question_custom_edit(permission_id);
            return;
        }

        let Some(option) = prompt.options.get(self.question_prompt_selection) else {
            return;
        };
        let answer = option.label.clone();
        if prompt.multiple {
            self.toggle_question_answer(self.question_prompt_tab, &answer);
            return;
        }

        self.question_prompt_answers[self.question_prompt_tab] = vec![answer];
        if question_prompt_is_single_select(prompts) {
            self.execute_action(Action::AllowPermission);
            return;
        }

        self.select_question_tab(
            (self.question_prompt_tab + 1)
                .min(question_prompt_tab_count(prompts).saturating_sub(1)),
        );
    }

    fn start_question_custom_edit(&mut self, permission_id: &str) {
        let Some(current) = self
            .question_prompt_custom
            .get(self.question_prompt_tab)
            .cloned()
        else {
            return;
        };
        self.question_answer_permission_id = Some(permission_id.to_string());
        self.question_prompt_editing = true;
        self.question_answer_buffer = current;
        self.question_answer_cursor = self.question_answer_char_count();
        self.question_answer_error = None;
    }

    fn commit_question_custom_answer(
        &mut self,
        permission_id: &str,
        prompts: &[QuestionPromptView],
    ) {
        let Some(prompt) = prompts.get(self.question_prompt_tab) else {
            return;
        };
        let answer = self.question_answer_buffer.trim().to_string();
        let previous = self
            .question_prompt_custom
            .get(self.question_prompt_tab)
            .cloned()
            .unwrap_or_default();
        self.question_prompt_editing = false;
        self.question_answer_error = None;

        if answer.is_empty() {
            self.clear_question_custom_answer(self.question_prompt_tab, &previous);
            return;
        }

        self.question_prompt_custom[self.question_prompt_tab] = answer.clone();
        if prompt.multiple {
            self.replace_question_custom_answer(self.question_prompt_tab, &previous, answer);
            return;
        }

        self.question_prompt_answers[self.question_prompt_tab] = vec![answer];
        if question_prompt_is_single_select(prompts) {
            self.execute_action(Action::AllowPermission);
            return;
        }

        self.select_question_tab(
            (self.question_prompt_tab + 1)
                .min(question_prompt_tab_count(prompts).saturating_sub(1)),
        );
        self.question_answer_permission_id = Some(permission_id.to_string());
    }

    fn toggle_question_answer(&mut self, index: usize, answer: &str) {
        let Some(values) = self.question_prompt_answers.get_mut(index) else {
            return;
        };
        if let Some(position) = values.iter().position(|value| value == answer) {
            values.remove(position);
        } else {
            values.push(answer.to_string());
        }
    }

    fn clear_question_custom_answer(&mut self, index: usize, previous: &str) {
        if let Some(value) = self.question_prompt_custom.get_mut(index) {
            value.clear();
        }
        if previous.is_empty() {
            return;
        }
        if let Some(values) = self.question_prompt_answers.get_mut(index) {
            values.retain(|value| value != previous);
        }
    }

    fn replace_question_custom_answer(&mut self, index: usize, previous: &str, answer: String) {
        let Some(values) = self.question_prompt_answers.get_mut(index) else {
            return;
        };
        values.retain(|value| value != previous);
        if !values.iter().any(|value| value == &answer) {
            values.push(answer);
        }
    }

    fn question_answer_char_count(&self) -> usize {
        self.question_answer_buffer.chars().count()
    }

    fn question_answer_cursor_byte_index(&self) -> usize {
        self.question_answer_buffer
            .char_indices()
            .nth(self.question_answer_cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.question_answer_buffer.len())
    }

    fn insert_question_answer_char(&mut self, c: char) {
        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_answer_buffer.insert(byte_idx, c);
        self.question_answer_cursor += 1;
        self.question_answer_error = None;
    }

    fn backspace_question_answer_char(&mut self) {
        if self.question_answer_cursor == 0 {
            return;
        }

        self.question_answer_cursor -= 1;
        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_answer_buffer.remove(byte_idx);
        self.question_answer_error = None;
    }

    fn delete_question_answer_char(&mut self) {
        if self.question_answer_cursor >= self.question_answer_char_count() {
            return;
        }

        let byte_idx = self.question_answer_cursor_byte_index();
        self.question_answer_buffer.remove(byte_idx);
        self.question_answer_error = None;
    }

    fn build_question_permission_reason(
        &mut self,
        permission: &ActivePermissionView,
    ) -> Option<String> {
        let prompts = permission.question_prompts.as_ref()?;
        match build_question_answers(prompts, &self.question_prompt_answers) {
            Ok(answers) => {
                self.question_answer_error = None;
                serde_json::to_string(&answers).ok()
            }
            Err(err) => {
                self.question_answer_error = Some(err);
                None
            }
        }
    }

    pub(crate) fn question_answer_preview(&self, permission_id: &str) -> String {
        if self.question_answer_permission_id.as_deref() != Some(permission_id) {
            return String::new();
        }

        if !self.question_prompt_editing {
            return self
                .question_prompt_custom
                .get(self.question_prompt_tab)
                .cloned()
                .unwrap_or_default();
        }

        let mut preview = self.question_answer_buffer.clone();
        let byte_idx = preview
            .char_indices()
            .nth(self.question_answer_cursor)
            .map(|(index, _)| index)
            .unwrap_or(preview.len());
        preview.insert(byte_idx, '█');
        preview
    }

    pub(crate) fn question_answer_error(&self, permission_id: &str) -> Option<&str> {
        (self.question_answer_permission_id.as_deref() == Some(permission_id))
            .then_some(self.question_answer_error.as_deref())
            .flatten()
    }

    pub(super) fn clear_question_answer_state(&mut self, permission_id: &str) {
        if self.question_answer_permission_id.as_deref() != Some(permission_id) {
            return;
        }

        self.question_answer_permission_id = None;
        self.question_prompt_tab = 0;
        self.question_prompt_selection = 0;
        self.question_prompt_answers.clear();
        self.question_prompt_custom.clear();
        self.question_prompt_editing = false;
        self.question_answer_buffer.clear();
        self.question_answer_cursor = 0;
        self.question_answer_error = None;
    }

    pub(super) fn send_permission_intent(
        &mut self,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
    ) {
        if self.submitted_permission_id.as_deref() == Some(permission_id.as_str()) {
            return;
        }

        self.emit_ui_intent(UiIntent::ResolvePermission {
            permission_id: permission_id.clone(),
            decision,
            reason,
        });
        self.submitted_permission_id = Some(permission_id);
    }
}

fn parse_question_prompts(kind: &str, summary: &str) -> Option<Vec<QuestionPromptView>> {
    if !kind.eq_ignore_ascii_case("question")
        && !kind.eq_ignore_ascii_case("ask")
        && !kind.eq_ignore_ascii_case("ask_user")
    {
        return None;
    }

    let value = serde_json::from_str::<serde_json::Value>(summary).ok()?;
    let questions = value.get("questions")?.as_array()?;
    let prompts = questions
        .iter()
        .map(|question| {
            Some(QuestionPromptView {
                question: question.get("question")?.as_str()?.to_string(),
                header: question.get("header")?.as_str()?.to_string(),
                options: question
                    .get("options")?
                    .as_array()?
                    .iter()
                    .map(|option| {
                        Some(QuestionOptionView {
                            label: option.get("label")?.as_str()?.to_string(),
                            description: option.get("description")?.as_str()?.to_string(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
                multiple: question
                    .get("multiple")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                custom: question
                    .get("custom")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(prompts)
}

fn build_question_answers(
    prompts: &[QuestionPromptView],
    current_answers: &[Vec<String>],
) -> Result<Vec<Vec<String>>, String> {
    let mut answers = Vec::with_capacity(prompts.len());

    for (index, prompt) in prompts.iter().enumerate() {
        let values = current_answers.get(index).cloned().unwrap_or_default();
        if values.is_empty() {
            return Err(format!(
                "Answer question {} ({}) before continuing.",
                index + 1,
                prompt.header
            ));
        }

        if !prompt.multiple && values.len() > 1 {
            return Err(format!(
                "Question {} ({}) accepts only one answer.",
                index + 1,
                prompt.header
            ));
        }

        answers.push(
            values
                .into_iter()
                .map(|value| {
                    prompt
                        .options
                        .iter()
                        .find(|option| option.label.eq_ignore_ascii_case(&value))
                        .map(|option| option.label.clone())
                        .unwrap_or_else(|| value.to_string())
                })
                .collect(),
        );
    }

    Ok(answers)
}

fn question_prompt_is_single_select(prompts: &[QuestionPromptView]) -> bool {
    prompts.len() == 1 && !prompts[0].multiple
}

fn question_prompt_tab_count(prompts: &[QuestionPromptView]) -> usize {
    if question_prompt_is_single_select(prompts) {
        1
    } else {
        prompts.len() + 1
    }
}

fn question_prompt_confirm_active(tab: usize, prompts: &[QuestionPromptView]) -> bool {
    !question_prompt_is_single_select(prompts) && tab >= prompts.len()
}

fn question_prompt_choice_count(prompt: &QuestionPromptView) -> usize {
    prompt.options.len() + usize::from(prompt.custom)
}

pub(super) fn permission_display_summary(permission: &ActivePermissionView) -> String {
    if permission.question_prompts.is_some() {
        "Question requested".to_string()
    } else {
        permission.summary.clone()
    }
}
