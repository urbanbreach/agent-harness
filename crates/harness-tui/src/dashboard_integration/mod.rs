#![allow(
    clippy::module_name_repetitions,
    reason = "task 36 facade names the integrated dashboard state explicitly"
)]

mod focus;
mod input;
mod overlays;
mod responsive;
mod state;
mod surface;

pub use crate::dashboard_dispatch::{
    DashboardDispatch, DispatchAction, DispatchError, DispatchIntent,
};
pub use focus::{DashboardFocus, DashboardPane, FocusDirection};
pub use input::{
    DashboardInput, DashboardInputRouter, DashboardMouseContext, SearchContext, SearchState,
    ShortcutEntry, ShortcutHelp,
};
pub use overlays::{
    DashboardModal, DashboardModalKind, DashboardOverlayRoute, DashboardOverlayState,
};
pub use responsive::{
    dashboard_content_viewport, dashboard_viewport, layout_for_rect, DashboardBreakpoint,
    DashboardHooks, DashboardLayout, DashboardNotification, DashboardNotificationKind,
    DashboardPaneVisibility,
};
pub use state::{DashboardIntegrationError, DashboardIntegrationParts, DashboardReturnState};

use crate::app::Focus;
use crate::dashboard::{DashboardReadModel, SelectionKey};
use crate::dashboard_controls::{
    dispatch, ControlResult, DashboardCommand, DashboardControlError, DashboardControlState,
    DashboardVisual,
};
use crate::dashboard_details::{DashboardDetails, DetailsPaneFields};
use crate::dashboard_peek::{DashboardPeek, DashboardPeekView};
use crate::dashboard_roster::{
    layout_for_rect as roster_layout_for_rect, RosterHitMap, RosterState,
};
use crate::shell_geometry::ShellState;
use crate::transcript_identity::TranscriptFocus;
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use ratatui::layout::Rect;

pub struct DashboardIntegration {
    dashboard: DashboardReadModel,
    roster: RosterState,
    peek: DashboardPeek,
    details: Option<DashboardDetails>,
    controls: DashboardControlState,
    focus: DashboardFocus,
    layout: DashboardLayout,
    overlays: DashboardOverlayState,
    input: DashboardInputRouter,
    search: SearchState,
    help_visible: bool,
    hooks: DashboardHooks,
    return_state: Option<DashboardReturnState>,
}

impl DashboardIntegration {
    pub fn new(
        parts: DashboardIntegrationParts,
        viewport: Rect,
    ) -> Result<Self, DashboardIntegrationError> {
        if viewport.width == 0 || viewport.height == 0 {
            return Err(DashboardIntegrationError::InvalidViewport);
        }
        let mut peek = parts.peek;
        peek.sync_dashboard(&parts.dashboard)?;
        let layout = layout_for_rect(viewport, ShellState::Streaming);
        let mut integration = Self {
            dashboard: parts.dashboard,
            roster: parts.roster,
            peek,
            details: parts.details,
            controls: parts.controls,
            focus: DashboardFocus::new(DashboardPane::Roster),
            layout,
            overlays: DashboardOverlayState::new(),
            input: DashboardInputRouter::new(),
            search: SearchState::new(),
            help_visible: false,
            hooks: DashboardHooks::new(),
            return_state: None,
        };
        integration.reconcile_focus();
        Ok(integration)
    }

    pub fn handle(&mut self, input: DashboardInput) -> Result<(), DashboardIntegrationError> {
        match input {
            DashboardInput::Focus(direction) => {
                self.focus.traverse(direction, &self.layout.visible_panes());
            }
            DashboardInput::Select(key) => self.select(key)?,
            DashboardInput::ToggleGroup(group) => self.roster.toggle_fold(group),
            DashboardInput::Search(context) => self.begin_search(context),
            DashboardInput::SearchText(text) => self.input_search(&text)?,
            DashboardInput::Reply => self.focus.set(DashboardPane::Reply),
            DashboardInput::Move(pane, _) => self.focus.set(pane),
            DashboardInput::Scroll(pane, _) => self.focus.set(pane),
            DashboardInput::DetailsCycle(direction) => {
                let details = self
                    .details
                    .as_mut()
                    .ok_or(DashboardIntegrationError::DetailsUnavailable)?;
                details.cycle_related(direction)?;
                self.focus.set(DashboardPane::Details);
            }
            DashboardInput::DetailsBack => {
                let details = self
                    .details
                    .as_mut()
                    .ok_or(DashboardIntegrationError::DetailsUnavailable)?;
                details.back()?;
                self.focus.set(DashboardPane::Details);
            }
            DashboardInput::Help => self.help_visible = !self.help_visible,
            DashboardInput::ModalAction(_, _)
            | DashboardInput::ChromeAction(_, _)
            | DashboardInput::Unhandled => {}
        }
        Ok(())
    }

    pub fn handle_key(&mut self, event: KeyEvent) -> Result<(), DashboardIntegrationError> {
        if self.search.context.is_some() {
            match event.code {
                KeyCode::Char(character) => self.input_search(&character.to_string())?,
                KeyCode::Backspace => {
                    self.search.backspace();
                    self.apply_roster_search();
                }
                KeyCode::Esc => {
                    self.search.clear();
                    self.roster
                        .set_filter(crate::dashboard_roster::RosterFilter::new());
                }
                _ => {}
            }
            return Ok(());
        }
        let input = self
            .input
            .route_key(event, self.focus.current(), &self.overlays);
        self.handle(input)
    }

    pub fn handle_mouse(&mut self, event: MouseEvent) -> Result<(), DashboardIntegrationError> {
        let hit_map = self.roster_hit_map();
        let input = self.input.route_mouse(
            event,
            DashboardMouseContext {
                roster: &hit_map,
                layout: &self.layout,
                overlays: &self.overlays,
            },
        );
        self.handle(input)
    }

    pub fn dispatch_control(
        &self,
        command: DashboardCommand,
    ) -> Result<ControlResult, DashboardControlError> {
        dispatch(&self.controls, command)
    }

    pub fn dispatch_reply(
        &self,
        dispatch: &mut DashboardDispatch,
        action: DispatchAction,
    ) -> Result<DispatchIntent, DispatchError> {
        dispatch.dispatch(action)
    }

    pub fn dashboard(&self) -> &DashboardReadModel {
        &self.dashboard
    }

    pub fn peek_view(&self) -> Result<DashboardPeekView, DashboardIntegrationError> {
        self.peek.view().map_err(Into::into)
    }

    pub fn details_fields(&self) -> Result<DetailsPaneFields, DashboardIntegrationError> {
        self.details
            .as_ref()
            .ok_or(DashboardIntegrationError::DetailsUnavailable)?
            .fields()
            .map_err(Into::into)
    }

    pub fn controls_visual(&self) -> &DashboardVisual {
        &self.controls.visual
    }

    pub const fn help_visible(&self) -> bool {
        self.help_visible
    }

    pub fn set_focus(&mut self, pane: DashboardPane) {
        self.focus.set(pane);
        self.reconcile_focus();
    }

    pub fn resize(&mut self, viewport: Rect) -> Result<(), DashboardIntegrationError> {
        if viewport.width == 0 || viewport.height == 0 {
            return Err(DashboardIntegrationError::InvalidViewport);
        }
        self.layout = layout_for_rect(viewport, ShellState::Streaming);
        self.hooks
            .notify(DashboardNotificationKind::Resized, viewport_label(viewport));
        self.reconcile_focus();
        Ok(())
    }

    fn select(&mut self, key: SelectionKey) -> Result<(), DashboardIntegrationError> {
        if self.dashboard.row(key.as_str()).is_none() {
            return Err(DashboardIntegrationError::UnknownSelection(key));
        }
        self.roster.set_selected(Some(key.clone()));
        self.peek.select(&key)?;
        self.hooks
            .set_title(format!("Harness dashboard · {}", key.as_str()));
        self.hooks.notify(
            DashboardNotificationKind::SelectionChanged,
            format!("selected session: {}", key.as_str()),
        );
        Ok(())
    }

    fn reconcile_focus(&mut self) {
        let visible = self.layout.visible_panes();
        if !visible.contains(&self.focus.current()) {
            self.focus.set(visible[0]);
        }
    }
}

fn viewport_label(viewport: Rect) -> String {
    format!(
        "resized dashboard to {}x{}",
        viewport.width, viewport.height
    )
}
