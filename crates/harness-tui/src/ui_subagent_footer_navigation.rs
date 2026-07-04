use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::app::AppState;
use crate::keybindings::Action;
use crate::layout::FrameLayoutPlan;
use crate::theme::Theme;

use super::{display_width, muted_meta_style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubagentFooterTarget {
    Parent,
    Previous,
    Next,
}

pub(crate) fn subagent_footer_target_at(
    app: &AppState,
    frame_area: Rect,
    column: u16,
    row: u16,
) -> Option<SubagentFooterTarget> {
    if app.review_surface().is_some() || !app.current_subagent_session_present() {
        return None;
    }
    let plan = FrameLayoutPlan::for_app(app, frame_area);
    if plan.footer.height == 0 || plan.footer_text.height == 0 {
        return None;
    }
    if !rect_contains(plan.footer, column, row) {
        return None;
    }
    let nav_items = subagent_navigation_items(app);
    let layout = footer_content_layout(plan.footer, &nav_items)?;
    subagent_navigation_targets(&nav_items, layout.nav_area)
        .into_iter()
        .find(|item| rect_contains(item.rect, column, row))
        .map(|item| item.target)
}

#[derive(Debug, Clone)]
pub(super) struct SubagentNavigationItem {
    target: SubagentFooterTarget,
    label: &'static str,
    shortcut: String,
}

#[derive(Debug, Clone)]
struct SubagentNavigationTarget {
    target: SubagentFooterTarget,
    rect: Rect,
}

#[derive(Debug, Clone, Copy)]
struct FooterContentLayout {
    nav_area: Rect,
}

fn footer_content_layout(
    area: Rect,
    nav_items: &[SubagentNavigationItem],
) -> Option<FooterContentLayout> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let content_y = if area.height > 1 {
        area.y.saturating_add(1)
    } else {
        area.y
    };
    let content_x = area.x.saturating_add(2);
    let content_right_padding = 1;
    let used_left = content_x.saturating_sub(area.x);
    let content_width = area
        .width
        .saturating_sub(used_left)
        .saturating_sub(content_right_padding);
    if content_width == 0 {
        return None;
    }
    let content_area = Rect::new(content_x, content_y, content_width, 1);
    let nav_width = subagent_navigation_width(nav_items).min(usize::from(content_width));
    let nav_area = Rect::new(
        content_area
            .right()
            .saturating_sub(u16::try_from(nav_width).unwrap_or(u16::MAX)),
        content_area.y,
        u16::try_from(nav_width).unwrap_or(u16::MAX),
        1,
    );
    Some(FooterContentLayout { nav_area })
}

pub(super) fn subagent_navigation_items(app: &AppState) -> Vec<SubagentNavigationItem> {
    subagent_navigation_items_for_bindings(
        &app.keymap.get_binding_str(Action::SessionParent),
        &app.keymap.get_binding_str(Action::SessionChildCycleReverse),
        &app.keymap.get_binding_str(Action::SessionChildCycle),
    )
}

fn subagent_navigation_items_for_bindings(
    parent_shortcut: &str,
    previous_shortcut: &str,
    next_shortcut: &str,
) -> Vec<SubagentNavigationItem> {
    vec![
        SubagentNavigationItem {
            target: SubagentFooterTarget::Parent,
            label: "Parent",
            shortcut: parent_shortcut.to_string(),
        },
        SubagentNavigationItem {
            target: SubagentFooterTarget::Previous,
            label: "Prev",
            shortcut: previous_shortcut.to_string(),
        },
        SubagentNavigationItem {
            target: SubagentFooterTarget::Next,
            label: "Next",
            shortcut: next_shortcut.to_string(),
        },
    ]
}

pub(super) fn subagent_navigation_width(items: &[SubagentNavigationItem]) -> usize {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let separator_width = if index == 0 { 0 } else { 2 };
            separator_width + display_width(&subagent_navigation_item_text(item))
        })
        .sum()
}

pub(super) fn subagent_navigation_line(
    items: &[SubagentNavigationItem],
    hovered: Option<SubagentFooterTarget>,
    theme: &Theme,
    surface: ratatui::style::Color,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", Style::default().bg(surface)));
        }
        let item_hovered = hovered == Some(item.target);
        let background = if item_hovered {
            theme.surface.panel_elevated
        } else {
            surface
        };
        let label_style = Style::default()
            .fg(theme.text.primary)
            .bg(background)
            .add_modifier(if item_hovered {
                Modifier::BOLD | Modifier::UNDERLINED
            } else {
                Modifier::empty()
            });
        spans.push(Span::styled(item.label, label_style));
        spans.push(Span::styled(" ", Style::default().bg(background)));
        spans.push(Span::styled(
            item.shortcut.clone(),
            muted_meta_style(theme).bg(background),
        ));
    }
    Line::from(spans)
}

fn subagent_navigation_targets(
    items: &[SubagentNavigationItem],
    nav_area: Rect,
) -> Vec<SubagentNavigationTarget> {
    let mut x = nav_area.x;
    let mut targets = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            x = x.saturating_add(2);
        }
        let width = u16::try_from(display_width(&subagent_navigation_item_text(item)))
            .unwrap_or(u16::MAX)
            .min(nav_area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        targets.push(SubagentNavigationTarget {
            target: item.target,
            rect: Rect::new(x, nav_area.y, width, 1),
        });
        x = x.saturating_add(width);
    }
    targets
}

fn subagent_navigation_item_text(item: &SubagentNavigationItem) -> String {
    format!("{} {}", item.label, item.shortcut)
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    let position = Position::new(column, row);
    rect.contains(position)
}
