use ratatui::layout::Rect;

use crate::shell_geometry::{layout_for_rect as shell_layout_for_rect, ShellRegions, ShellState};

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
        let mut panes = [DashboardPane::Roster; 4];
        let candidates = [
            DashboardPane::Roster,
            DashboardPane::Peek,
            DashboardPane::Reply,
            DashboardPane::Details,
        ];
        let visible = [self.roster, self.peek, self.reply, self.details];
        let mut index = 0;
        let mut count = 0;
        while index < 4 {
            if visible[index] {
                panes[count] = candidates[index];
                count += 1;
            }
            index += 1;
        }
        panes
    }

    pub const fn count(self) -> usize {
        self.roster as usize + self.peek as usize + self.reply as usize + self.details as usize
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

pub fn dashboard_viewport(root: Rect) -> Option<Rect> {
    if root.width < 32 || root.height < 8 {
        return None;
    }
    Some(root)
}

pub fn dashboard_content_viewport(root: Rect) -> Option<Rect> {
    let overlay = dashboard_viewport(root)?;
    let horizontal_inset = 2.min(overlay.width.saturating_sub(1));
    let content = Rect::new(
        overlay.x.saturating_add(horizontal_inset),
        overlay.y.saturating_add(1),
        overlay
            .width
            .saturating_sub(horizontal_inset.saturating_mul(2)),
        overlay.height.saturating_sub(2),
    );
    (content.width > 0 && content.height > 0).then_some(content)
}

impl DashboardLayout {
    pub fn visible_panes(&self) -> Vec<DashboardPane> {
        [
            (DashboardPane::Roster, self.visibility.roster),
            (DashboardPane::Peek, self.visibility.peek),
            (DashboardPane::Reply, self.visibility.reply),
            (DashboardPane::Details, self.visibility.details),
        ]
        .into_iter()
        .filter_map(|(pane, visible)| visible.then_some(pane))
        .collect()
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
    layout_with_reply_rows(viewport, shell_state, 1)
}

pub(super) fn layout_with_reply_rows(
    viewport: Rect,
    shell_state: ShellState,
    reply_rows: u16,
) -> DashboardLayout {
    let shell = shell_layout_for_rect(viewport, shell_state);
    let breakpoint = if viewport.width <= 60 {
        DashboardBreakpoint::Compact
    } else if viewport.width < 121 {
        DashboardBreakpoint::Standard
    } else {
        DashboardBreakpoint::Wide
    };
    // Reserve the roster first. A bottom peek may use at most 3/8 of the
    // viewport, and appears only when twelve list rows and useful content fit.
    let body = Rect::new(
        viewport.x,
        viewport.y.saturating_add(2),
        viewport.width,
        viewport.height.saturating_sub(4),
    );
    let reply_height = reply_rows.clamp(1, 5).saturating_add(2).min(body.height);
    let available = body.height.saturating_sub(reply_height);
    let candidate = available
        .saturating_sub(12)
        .min(viewport.height.saturating_mul(3) / 8);
    let peek_height = if candidate >= 8 { candidate } else { 0 };
    let roster = Rect::new(
        body.x,
        body.y,
        body.width,
        available.saturating_sub(peek_height),
    );
    let peek = Rect::new(body.x, roster.bottom(), body.width, peek_height);
    let reply = Rect::new(body.x, peek.bottom(), body.width, reply_height);
    let details = None;
    let visibility = DashboardPaneVisibility {
        roster: roster.height > 0,
        peek: peek.height > 0,
        reply: reply.height > 0,
        details: false,
    };
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

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && x >= rect.x
        && y >= rect.y
        && x < rect.right()
        && y < rect.bottom()
}
