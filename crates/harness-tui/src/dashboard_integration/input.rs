use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::dashboard::{DashboardGroupKey, SelectionKey};
use crate::dashboard_roster::{RosterHitMap, RosterHitTarget};
use crate::keybindings::{Action, KeyMap};
use crate::overlay::OverlayKind;

use super::focus::{DashboardPane, FocusDirection};
use super::overlays::{DashboardModalKind, DashboardOverlayRoute, DashboardOverlayState};
use super::responsive::DashboardLayout;
use crate::dashboard_details::CycleDirection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchContext {
    Roster,
    Details,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchState {
    pub context: Option<SearchContext>,
    pub query: String,
}

impl SearchState {
    pub const fn new() -> Self {
        Self {
            context: None,
            query: String::new(),
        }
    }

    pub fn begin(&mut self, context: SearchContext) {
        self.context = Some(context);
        self.query.clear();
    }

    pub fn input(&mut self, text: &str) {
        self.query.push_str(text);
    }

    pub fn backspace(&mut self) {
        let _ = self.query.pop();
    }

    pub fn clear(&mut self) {
        self.context = None;
        self.query.clear();
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardInput {
    Focus(FocusDirection),
    Search(SearchContext),
    SearchText(String),
    Help,
    Reply,
    Select(SelectionKey),
    ToggleGroup(DashboardGroupKey),
    Scroll(DashboardPane, i16),
    Move(DashboardPane, i8),
    DetailsCycle(CycleDirection),
    DetailsBack,
    ModalAction(DashboardModalKind, Action),
    ChromeAction(OverlayKind, Action),
    Unhandled,
}

#[derive(Debug, Clone)]
pub struct DashboardInputRouter {
    keymap: KeyMap,
}

impl DashboardInputRouter {
    pub fn new() -> Self {
        Self {
            keymap: KeyMap::with_defaults(),
        }
    }

    pub fn route_key(
        &self,
        event: KeyEvent,
        pane: DashboardPane,
        overlays: &DashboardOverlayState,
    ) -> DashboardInput {
        match overlays.route(pane) {
            DashboardOverlayRoute::Modal(modal) => {
                return self.modal_key(event, modal);
            }
            DashboardOverlayRoute::Chrome(chrome) => {
                let action = self
                    .keymap
                    .get_action(&event)
                    .unwrap_or(Action::DismissModal);
                return DashboardInput::ChromeAction(chrome, action);
            }
            DashboardOverlayRoute::Pane(_) => {}
        }
        if event.code == KeyCode::Tab && event.modifiers == KeyModifiers::NONE {
            return DashboardInput::Focus(FocusDirection::Forward);
        }
        if matches!(event.code, KeyCode::BackTab)
            || (event.code == KeyCode::Tab && event.modifiers == KeyModifiers::SHIFT)
        {
            return DashboardInput::Focus(FocusDirection::Backward);
        }
        if event.code == KeyCode::Char('/') {
            return DashboardInput::Search(search_context(pane));
        }
        if event.code == KeyCode::Char('f') && event.modifiers == KeyModifiers::CONTROL {
            return DashboardInput::Search(search_context(pane));
        }
        match self.keymap.get_action(&event) {
            Some(Action::Help) => DashboardInput::Help,
            Some(Action::SubmitPrompt) if pane == DashboardPane::Reply => DashboardInput::Reply,
            Some(Action::MoveDown) => DashboardInput::Move(pane, 1),
            Some(Action::MoveUp) => DashboardInput::Move(pane, -1),
            Some(Action::ScrollUp) => DashboardInput::Scroll(pane, -1),
            Some(Action::ScrollDown) => DashboardInput::Scroll(pane, 1),
            Some(Action::ToggleFollow) => DashboardInput::Scroll(pane, 0),
            Some(Action::SessionChildCycle) if pane == DashboardPane::Details => {
                DashboardInput::DetailsCycle(CycleDirection::Next)
            }
            Some(Action::SessionChildCycleReverse) if pane == DashboardPane::Details => {
                DashboardInput::DetailsCycle(CycleDirection::Previous)
            }
            Some(Action::CloseReviewSurface) if pane == DashboardPane::Details => {
                DashboardInput::DetailsBack
            }
            Some(Action::Char(character)) => DashboardInput::SearchText(character.to_string()),
            Some(action) => DashboardInput::ChromeAction(OverlayKind::DetailsDrawer, action),
            None => DashboardInput::Unhandled,
        }
    }

    pub fn route_mouse(
        &self,
        event: MouseEvent,
        context: DashboardMouseContext<'_>,
    ) -> DashboardInput {
        let pane = context
            .layout
            .pane_at(event.column, event.row)
            .unwrap_or(DashboardPane::Roster);
        match context.overlays.route(pane) {
            DashboardOverlayRoute::Modal(modal) => {
                return DashboardInput::ModalAction(modal, Action::DismissModal);
            }
            DashboardOverlayRoute::Chrome(chrome) => {
                return DashboardInput::ChromeAction(chrome, Action::DismissModal);
            }
            DashboardOverlayRoute::Pane(_) => {}
        }
        if pane == DashboardPane::Roster {
            if let Some(target) = context.roster.hit_test(event.column, event.row) {
                return match (event.kind, target) {
                    (MouseEventKind::Down(MouseButton::Left), RosterHitTarget::Row(key)) => {
                        DashboardInput::Select(key)
                    }
                    (MouseEventKind::Down(MouseButton::Left), RosterHitTarget::Group(group)) => {
                        DashboardInput::ToggleGroup(group)
                    }
                    (MouseEventKind::ScrollUp, _) => DashboardInput::Scroll(pane, -1),
                    (MouseEventKind::ScrollDown, _) => DashboardInput::Scroll(pane, 1),
                    _ => DashboardInput::Focus(FocusDirection::Forward),
                };
            }
        }
        match event.kind {
            MouseEventKind::ScrollUp => DashboardInput::Scroll(pane, -1),
            MouseEventKind::ScrollDown => DashboardInput::Scroll(pane, 1),
            MouseEventKind::Down(MouseButton::Left) => {
                DashboardInput::Focus(FocusDirection::Forward)
            }
            _ => DashboardInput::Unhandled,
        }
    }

    pub fn help(&self, pane: DashboardPane) -> ShortcutHelp {
        let mut entries = vec![
            ShortcutEntry {
                key: "Tab".to_string(),
                action: "focus next".to_string(),
            },
            ShortcutEntry {
                key: "Shift+Tab".to_string(),
                action: "focus previous".to_string(),
            },
            ShortcutEntry {
                key: "/".to_string(),
                action: "search".to_string(),
            },
            ShortcutEntry {
                key: self.keymap.get_binding_str(Action::Help),
                action: "help".to_string(),
            },
        ];
        let action = match pane {
            DashboardPane::Roster => Action::MoveDown,
            DashboardPane::Peek => Action::ToggleFollow,
            DashboardPane::Reply => Action::SubmitPrompt,
            DashboardPane::Details => Action::SessionChildCycle,
        };
        entries.push(ShortcutEntry {
            key: self.keymap.get_binding_str(action),
            action: pane_action_label(pane).to_string(),
        });
        ShortcutHelp { pane, entries }
    }

    fn modal_key(&self, event: KeyEvent, modal: DashboardModalKind) -> DashboardInput {
        let action = self
            .keymap
            .get_action(&event)
            .unwrap_or(Action::DismissModal);
        DashboardInput::ModalAction(modal, action)
    }
}

impl Default for DashboardInputRouter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DashboardMouseContext<'a> {
    pub roster: &'a RosterHitMap,
    pub layout: &'a DashboardLayout,
    pub overlays: &'a DashboardOverlayState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutHelp {
    pub pane: DashboardPane,
    pub entries: Vec<ShortcutEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutEntry {
    pub key: String,
    pub action: String,
}

fn search_context(pane: DashboardPane) -> SearchContext {
    match pane {
        DashboardPane::Details => SearchContext::Details,
        DashboardPane::Roster | DashboardPane::Peek | DashboardPane::Reply => SearchContext::Roster,
    }
}

fn pane_action_label(pane: DashboardPane) -> &'static str {
    match pane {
        DashboardPane::Roster => "move selection",
        DashboardPane::Peek => "toggle follow",
        DashboardPane::Reply => "submit reply",
        DashboardPane::Details => "cycle related session",
    }
}
