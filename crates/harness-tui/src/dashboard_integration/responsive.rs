use ratatui::layout::Rect;

use crate::shell_geometry::{ShellRegions, ShellState, layout_for_rect as shell_layout_for_rect};

use super::focus::DashboardPane;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardBreakpoint {
    Compact,
    Standard,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardPaneVisibility {
    pub roster: bool,
    pub peek: bool,
    pub reply: bool,
    pub details: bool,
}

impl DashboardPaneVisibility {
    pub const fn visible(self) -> [DashboardPane; 4] {
        if self.details {
            [
                DashboardPane::Roster,
                DashboardPane::Peek,
                DashboardPane::Reply,
                DashboardPane::Details,
            ]
        } else {
            [
                DashboardPane::Roster,
                DashboardPane::Peek,
                DashboardPane::Reply,
                DashboardPane::Roster,
            ]
        }
    }

    pub const fn count(self) -> usize {
        if self.details { 4 } else { 3 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardLayout {
    pub viewport: Rect,
    pub shell: ShellRegions,
    pub breakpoint: DashboardBreakpoint,
    pub visibility: DashboardPaneVisibility,
    pub roster: Rect,
    pub peek: Rect,
    pub reply: Rect,
    pub details: Option<Rect>,
}

impl DashboardLayout {
    pub fn visible_panes(&self) -> Vec<DashboardPane> {
        let all = self.visibility.visible();
        all[..self.visibility.count()].to_vec()
    }

    pub fn pane_at(&self, x: u16, y: u16) -> Option<DashboardPane> {
        [
            (DashboardPane::Details, self.details),
            (DashboardPane::Roster, Some(self.roster)),
            (DashboardPane::Peek, Some(self.peek)),
            (DashboardPane::Reply, Some(self.reply)),
        ]
        .into_iter()
        .find_map(|(pane, rect)| rect.filter(|area| contains(*area, x, y)).map(|_| pane))
    }
}

pub fn layout_for_rect(viewport: Rect, shell_state: ShellState) -> DashboardLayout {
    let shell = shell_layout_for_rect(viewport, shell_state);
    let breakpoint = if viewport.width <= 60 {
        DashboardBreakpoint::Compact
    } else if viewport.width < 121 {
        DashboardBreakpoint::Standard
    } else {
        DashboardBreakpoint::Wide
    };
    let visibility = DashboardPaneVisibility {
        roster: true,
        peek: true,
        reply: true,
        details: breakpoint == DashboardBreakpoint::Wide,
    };
    let body = shell.transcript_viewport;
    let first = body.width / 3;
    let second = body.width / 3;
    let roster = Rect::new(body.x, body.y, first, body.height);
    let peek = Rect::new(body.x.saturating_add(first), body.y, second, body.height);
    let reply = Rect::new(
        body.x.saturating_add(first).saturating_add(second),
        body.y,
        body.width.saturating_sub(first).saturating_sub(second),
        body.height,
    );
    let details = visibility.details.then_some(centered_overlay(viewport));
    DashboardLayout {
        viewport,
        shell,
        breakpoint,
        visibility,
        roster,
        peek,
        reply,
        details,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardNotificationKind {
    TaskCompleted,
    PermissionPending,
    QuestionPending,
    SelectionChanged,
    Resized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardNotification {
    pub kind: DashboardNotificationKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardHooks {
    title: String,
    notifications: Vec<DashboardNotification>,
}

impl DashboardHooks {
    pub fn new() -> Self {
        Self {
            title: "Harness dashboard".to_string(),
            notifications: Vec::new(),
        }
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    pub fn notify(&mut self, kind: DashboardNotificationKind, message: impl Into<String>) {
        self.notifications.push(DashboardNotification {
            kind,
            message: message.into(),
        });
        while self.notifications.len() > 3 {
            self.notifications.remove(0);
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn notifications(&self) -> &[DashboardNotification] {
        &self.notifications
    }
}

impl Default for DashboardHooks {
    fn default() -> Self {
        Self::new()
    }
}

fn centered_overlay(viewport: Rect) -> Rect {
    let width = viewport.width.saturating_sub(8).min(72);
    let height = viewport.height.saturating_sub(4).min(32);
    Rect::new(
        viewport.x + viewport.width.saturating_sub(width) / 2,
        viewport.y + viewport.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && x >= rect.x
        && y >= rect.y
        && x < rect.right()
        && y < rect.bottom()
}
