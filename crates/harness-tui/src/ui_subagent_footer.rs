use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::{AppState, SubagentFooterTarget, SubagentSessionInfo};
use crate::keybindings::Action;
use crate::layout::FrameLayoutPlan;
use crate::theme::Theme;

use super::{display_width, muted_meta_style};

#[derive(Debug, Clone, Copy)]
struct SubagentFooterHitLayout {
    row: u16,
    parent: (u16, u16),
    previous: (u16, u16),
    next: (u16, u16),
}

pub(crate) fn subagent_footer_mouse_target(
    app: &AppState,
    frame_area: Rect,
    column: u16,
    row: u16,
) -> Option<SubagentFooterTarget> {
    if app.review_surface().is_some() || !app.current_subagent_session_present() {
        return None;
    }

    let plan = FrameLayoutPlan::for_app(app, frame_area);
    let layout = subagent_footer_hit_layout(app, plan.footer)?;
    if row != layout.row {
        return None;
    }

    if range_contains(layout.parent, column) {
        Some(SubagentFooterTarget::Parent)
    } else if range_contains(layout.previous, column) {
        Some(SubagentFooterTarget::Previous)
    } else if range_contains(layout.next, column) {
        Some(SubagentFooterTarget::Next)
    } else {
        None
    }
}

fn range_contains((start, end): (u16, u16), value: u16) -> bool {
    value >= start && value < end
}

fn subagent_footer_hit_layout(app: &AppState, area: Rect) -> Option<SubagentFooterHitLayout> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let content_y = if area.height >= 3 {
        area.y.saturating_add(1)
    } else {
        area.y
    };
    let content_x = area.x.saturating_add(3);
    let used_left = content_x.saturating_sub(area.x);
    let content_width = area.width.saturating_sub(used_left).saturating_sub(1);
    if content_width == 0 {
        return None;
    }

    let content_area = Rect::new(content_x, content_y, content_width, 1);
    let parent_key = app.keymap.get_binding_str(Action::SessionParent);
    let previous_key = app.keymap.get_binding_str(Action::SessionChildCycleReverse);
    let next_key = app.keymap.get_binding_str(Action::SessionChildCycle);
    let parent_width = display_width(&format!("Parent {parent_key}"));
    let previous_width = display_width(&format!("Prev {previous_key}"));
    let next_width = display_width(&format!("Next {next_key}"));
    let gap_width = display_width("  ");
    let nav_width = parent_width
        .saturating_add(gap_width)
        .saturating_add(previous_width)
        .saturating_add(gap_width)
        .saturating_add(next_width)
        .min(usize::from(content_area.width));
    if nav_width == 0 {
        return None;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(u16::try_from(nav_width).unwrap_or(u16::MAX)),
        ])
        .split(content_area);
    let nav_x = columns[1].x;
    let parent_start = nav_x;
    let parent_end = parent_start.saturating_add(u16::try_from(parent_width).unwrap_or(u16::MAX));
    let previous_start = parent_end.saturating_add(u16::try_from(gap_width).unwrap_or(0));
    let previous_end =
        previous_start.saturating_add(u16::try_from(previous_width).unwrap_or(u16::MAX));
    let next_start = previous_end.saturating_add(u16::try_from(gap_width).unwrap_or(0));
    let next_end = next_start.saturating_add(u16::try_from(next_width).unwrap_or(u16::MAX));
    let nav_end = columns[1].x.saturating_add(columns[1].width);

    Some(SubagentFooterHitLayout {
        row: content_y,
        parent: (parent_start.min(nav_end), parent_end.min(nav_end)),
        previous: (previous_start.min(nav_end), previous_end.min(nav_end)),
        next: (next_start.min(nav_end), next_end.min(nav_end)),
    })
}

pub(super) fn render_subagent_footer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    _text_area: Rect,
    theme: &Theme,
    info: &SubagentSessionInfo,
) {
    let surface = theme.surface.panel;
    let style = Style::default().bg(surface);
    frame.render_widget(Block::default().style(style), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let rail_style = Style::default().fg(theme.border.subtle).bg(surface);
    for row in area.y..area.y.saturating_add(area.height) {
        frame.buffer_mut()[(area.x, row)]
            .set_symbol("┃")
            .set_style(rail_style);
    }

    let content_y = if area.height >= 3 {
        area.y.saturating_add(1)
    } else {
        area.y
    };
    let content_x = area.x.saturating_add(3);
    let content_right_padding = 1;
    let used_left = content_x.saturating_sub(area.x);
    let content_width = area
        .width
        .saturating_sub(used_left)
        .saturating_sub(content_right_padding);
    if content_width == 0 {
        return;
    }
    let content_area = Rect::new(content_x, content_y, content_width, 1);

    let parent_key = app.keymap.get_binding_str(Action::SessionParent);
    let previous_key = app.keymap.get_binding_str(Action::SessionChildCycleReverse);
    let next_key = app.keymap.get_binding_str(Action::SessionChildCycle);
    let nav_text = format!("Parent {parent_key}  Prev {previous_key}  Next {next_key}");
    let nav_width = nav_text
        .chars()
        .count()
        .min(usize::from(content_area.width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(u16::try_from(nav_width).unwrap_or(u16::MAX)),
        ])
        .split(content_area);

    let mut left_spans = vec![Span::styled(
        info.label.clone(),
        Style::default()
            .fg(theme.text.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    )];
    if info.total > 0 {
        left_spans.push(Span::styled(
            format!(" ({} of {})", info.index, info.total),
            muted_meta_style(theme).bg(surface),
        ));
    }
    if let Some(usage) = info.usage.as_deref() {
        left_spans.push(Span::styled(" ", Style::default().bg(surface)));
        left_spans.push(Span::styled(
            usage.to_string(),
            muted_meta_style(theme).bg(surface),
        ));
    }

    if columns[0].width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(left_spans)).style(style),
            columns[0],
        );
    }
    if columns[1].width > 0 {
        let hover = app.hovered_subagent_footer_target();
        let parent_surface =
            subagent_footer_button_surface(hover, SubagentFooterTarget::Parent, theme);
        let previous_surface =
            subagent_footer_button_surface(hover, SubagentFooterTarget::Previous, theme);
        let next_surface = subagent_footer_button_surface(hover, SubagentFooterTarget::Next, theme);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Parent ",
                    Style::default().fg(theme.text.primary).bg(parent_surface),
                ),
                Span::styled(parent_key, muted_meta_style(theme).bg(parent_surface)),
                Span::styled(
                    "  Prev ",
                    Style::default().fg(theme.text.primary).bg(previous_surface),
                ),
                Span::styled(previous_key, muted_meta_style(theme).bg(previous_surface)),
                Span::styled(
                    "  Next ",
                    Style::default().fg(theme.text.primary).bg(next_surface),
                ),
                Span::styled(next_key, muted_meta_style(theme).bg(next_surface)),
            ]))
            .style(style)
            .alignment(Alignment::Right),
            columns[1],
        );
    }
}

fn subagent_footer_button_surface(
    hover: Option<SubagentFooterTarget>,
    target: SubagentFooterTarget,
    theme: &Theme,
) -> Color {
    if hover == Some(target) {
        theme.surface.panel_elevated
    } else {
        theme.surface.panel
    }
}
