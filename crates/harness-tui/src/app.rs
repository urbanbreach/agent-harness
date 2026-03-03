use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent};
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::perm::PermissionDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Events,
    Output,
    Diff,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    List,
    Details,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionIntent {
    pub permission_id: String,
    pub decision: PermissionDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiIntent {
    ResolvePermission {
        permission_id: String,
        decision: PermissionDecision,
    },
    SubmitPrompt {
        text: String,
    },
    QuitRequested,
}

#[derive(Debug, Clone)]
struct PendingPermission {
    seq: u64,
    summary: String,
}

pub struct AppState {
    pub selected_event_index: usize,
    pub focus: Focus,
    pub follow_mode: bool,
    pub active_tab: Tab,
    pub events: Vec<EventEnvelopeV1>,
    pub should_quit: bool,
    pub replay_mode: bool,
    pub session_path: Option<PathBuf>,
    pub status_banner: Option<String>,
    pub details_scroll: u16,
    pub auto_exit_on_finish: bool,
    seen_seqs: BTreeSet<u64>,
    pending_permissions: BTreeMap<String, PendingPermission>,
    dismissed_permissions: BTreeSet<String>,
    submitted_permission_id: Option<String>,
    reload_requested: bool,
    run_terminal_seen: bool,
    on_permission_intent: Option<Arc<dyn Fn(PermissionIntent) + Send + Sync>>,
    pub prompt_buffer: String,
    pub prompt_cursor: usize,
    pub prompt_history: Vec<String>,
    pub prompt_history_index: Option<usize>,
    pub on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            selected_event_index: 0,
            focus: Focus::default(),
            follow_mode: true,
            active_tab: Tab::default(),
            events: Vec::new(),
            should_quit: false,
            replay_mode: false,
            session_path: None,
            status_banner: None,
            details_scroll: 0,
            auto_exit_on_finish: false,
            seen_seqs: BTreeSet::new(),
            pending_permissions: BTreeMap::new(),
            dismissed_permissions: BTreeSet::new(),
            submitted_permission_id: None,
            reload_requested: false,
            run_terminal_seen: false,
            on_permission_intent: None,
            prompt_buffer: String::new(),
            prompt_cursor: 0,
            prompt_history: Vec::new(),
            prompt_history_index: None,
            on_ui_intent: None,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_live(
        session_path: Option<PathBuf>,
        auto_exit_on_finish: bool,
        on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    ) -> Self {
        let mut state = Self::new();
        state.session_path = session_path;
        state.auto_exit_on_finish = auto_exit_on_finish;
        state.on_ui_intent = on_ui_intent;
        state
    }

    pub fn new_replay(session_path: PathBuf, events: Vec<EventEnvelopeV1>) -> Self {
        let mut state = Self::new();
        state.replay_mode = true;
        state.session_path = Some(session_path);
        state.replace_events(events);
        state
    }

    pub fn replace_events(&mut self, events: Vec<EventEnvelopeV1>) {
        self.events.clear();
        self.seen_seqs.clear();
        self.pending_permissions.clear();
        self.dismissed_permissions.clear();
        self.submitted_permission_id = None;
        self.run_terminal_seen = false;

        for event in events {
            self.ingest_event(event);
        }

        if self.events.is_empty() {
            self.selected_event_index = 0;
        } else {
            self.selected_event_index = self.selected_event_index.min(self.events.len() - 1);
        }
        self.details_scroll = 0;
        self.maybe_auto_exit();
    }

    pub fn ingest_event(&mut self, event: EventEnvelopeV1) {
        if self.seen_seqs.contains(&event.seq) {
            return;
        }

        self.seen_seqs.insert(event.seq);
        self.update_derived_state_for_event(&event);
        self.events.push(event);

        if self.follow_mode && !self.events.is_empty() {
            self.selected_event_index = self.events.len() - 1;
            self.details_scroll = 0;
        }

        self.maybe_auto_exit();
    }

    pub fn set_status_banner(&mut self, status: Option<String>) {
        self.status_banner = status;
    }

    pub fn selected_event(&self) -> Option<&EventEnvelopeV1> {
        self.events.get(self.selected_event_index)
    }

    pub fn run_id(&self) -> Option<&str> {
        self.events.first().map(|event| event.run_id.as_str())
    }

    pub fn active_permission(&self) -> Option<(String, String)> {
        self.pending_permissions
            .iter()
            .filter(|(permission_id, _)| !self.dismissed_permissions.contains(*permission_id))
            .min_by_key(|(_, pending)| pending.seq)
            .map(|(permission_id, pending)| (permission_id.clone(), pending.summary.clone()))
    }

    pub fn permission_submission_pending(&self, permission_id: &str) -> bool {
        self.submitted_permission_id.as_deref() == Some(permission_id)
    }

    pub fn take_reload_requested(&mut self) -> bool {
        let requested = self.reload_requested;
        self.reload_requested = false;
        requested
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.handle_modal_key(key.code) {
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.active_tab = Tab::Help,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::List => Focus::Details,
                    Focus::Details => Focus::Prompt,
                    Focus::Prompt => Focus::List,
                };
            }
            KeyCode::Char('1') => self.active_tab = Tab::Events,
            KeyCode::Char('2') => self.active_tab = Tab::Output,
            KeyCode::Char('3') => self.active_tab = Tab::Diff,
            KeyCode::Char('h') if self.active_tab != Tab::Help && self.focus != Focus::Prompt => {
                self.active_tab = Tab::Help
            }
            KeyCode::Char(' ') => self.follow_mode = !self.follow_mode,
            KeyCode::Char('r') if self.replay_mode => self.reload_requested = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.focus == Focus::List {
                    self.next_event();
                } else {
                    self.details_scroll = self.details_scroll.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus == Focus::List {
                    self.previous_event();
                } else {
                    self.details_scroll = self.details_scroll.saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                if self.focus == Focus::Prompt && !self.prompt_buffer.is_empty() {
                    let text = self.prompt_buffer.clone();
                    self.prompt_buffer.clear();
                    self.prompt_cursor = 0;
                    self.prompt_history.push(text.clone());
                    self.prompt_history_index = None;
                    if let Some(handler) = &self.on_ui_intent {
                        handler(UiIntent::SubmitPrompt { text });
                    }
                }
            }
            KeyCode::Char(c) => {
                if self.focus == Focus::Prompt {
                    self.prompt_buffer.insert(self.prompt_cursor, c);
                    self.prompt_cursor += 1;
                }
            }
            KeyCode::Backspace => {
                if self.focus == Focus::Prompt && self.prompt_cursor > 0 {
                    self.prompt_cursor -= 1;
                    self.prompt_buffer.remove(self.prompt_cursor);
                }
            }
            KeyCode::Esc => {
                if self.focus == Focus::Prompt {
                    self.prompt_buffer.clear();
                    self.prompt_cursor = 0;
                }
            }
            _ => {}
        }

        self.maybe_auto_exit();
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

    fn handle_modal_key(&mut self, key_code: KeyCode) -> bool {
        let Some((permission_id, _)) = self.active_permission() else {
            return false;
        };

        match key_code {
            KeyCode::Char('a') => {
                self.send_permission_intent(permission_id, PermissionDecision::Allow);
                true
            }
            KeyCode::Char('d') => {
                self.send_permission_intent(permission_id, PermissionDecision::Deny);
                true
            }
            KeyCode::Esc => {
                self.dismissed_permissions.insert(permission_id);
                self.maybe_auto_exit();
                true
            }
            _ => false,
        }
    }

    fn send_permission_intent(&mut self, permission_id: String, decision: PermissionDecision) {
        if self.submitted_permission_id.as_deref() == Some(permission_id.as_str()) {
            return;
        }

        if let Some(handler) = &self.on_ui_intent {
            handler(UiIntent::ResolvePermission {
                permission_id: permission_id.clone(),
                decision,
            });
        }
        self.submitted_permission_id = Some(permission_id);
    }

    fn update_derived_state_for_event(&mut self, event: &EventEnvelopeV1) {
        match &event.payload {
            EventV1::PermissionRequested(data) => {
                self.pending_permissions.insert(
                    data.permission_id.clone(),
                    PendingPermission {
                        seq: event.seq,
                        summary: data.summary.clone(),
                    },
                );
            }
            EventV1::PermissionResolved(data) => {
                self.pending_permissions.remove(&data.permission_id);
                self.dismissed_permissions.remove(&data.permission_id);
                if self.submitted_permission_id.as_deref() == Some(data.permission_id.as_str()) {
                    self.submitted_permission_id = None;
                }
            }
            EventV1::RunFinished(_) | EventV1::RunFailed(_) => {
                self.run_terminal_seen = true;
            }
            _ => {}
        }
    }

    fn maybe_auto_exit(&mut self) {
        if self.auto_exit_on_finish && self.run_terminal_seen && self.active_permission().is_none()
        {
            self.should_quit = true;
        }
    }
}
