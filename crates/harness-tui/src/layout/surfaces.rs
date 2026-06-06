use ratatui::layout::Rect;

use crate::theme::{LifecycleCardTokens, ShellGeometryTarget, Theme};

const LIVE_EMPTY_STATE_MIN_HEIGHT: u16 = 9;
const SECONDARY_STACK_MAX_WIDTH: u16 = 72;
const SECONDARY_STACK_MIN_HEIGHT: u16 = 18;
const RUNTIME_STATE_SURFACE_HORIZONTAL_INSET: u16 = 6;
const RUNTIME_STATE_SURFACE_MAX_WIDTH: u16 = 68;
const RUNTIME_STATE_SURFACE_MIN_WIDTH: u16 = 32;
const RUNTIME_STATE_SURFACE_MIN_HEIGHT: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SecondarySurfaceLayout {
    pub shell: Rect,
    pub body: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlDockLayout {
    pub shell: Rect,
    pub status: Option<Rect>,
    pub composer: Rect,
    pub disclosure: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EdgeInsets {
    pub left: u16,
    pub right: u16,
    pub top: u16,
    pub bottom: u16,
}

impl EdgeInsets {
    pub(crate) const ZERO: Self = Self {
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
    };

    pub(crate) const fn new(left: u16, right: u16, top: u16, bottom: u16) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PermissionDockLayout {
    pub rail_width: u16,
    pub tray_height: u16,
    pub shell_padding: EdgeInsets,
    pub body_padding: EdgeInsets,
    pub tray_padding: EdgeInsets,
    pub header_gap: u16,
    pub stacked_hint_min_width: u16,
    pub stacked_hint_min_action_width: u16,
}

const PERMISSION_DOCK_RAIL_WIDTH: u16 = 1;
const PERMISSION_DOCK_TRAY_HEIGHT: u16 = 3;
const QUESTION_PERMISSION_DOCK_TRAY_HEIGHT: u16 = 2;
const PERMISSION_DOCK_STACKED_HINT_MIN_WIDTH: u16 = 80;
const PERMISSION_DOCK_STACKED_HINT_MIN_ACTION_WIDTH: u16 = 20;

pub(crate) fn permission_dock_layout(area: Rect, is_question: bool) -> PermissionDockLayout {
    let tray_height = if is_question {
        QUESTION_PERMISSION_DOCK_TRAY_HEIGHT.min(area.height)
    } else if area.height >= PERMISSION_DOCK_TRAY_HEIGHT {
        PERMISSION_DOCK_TRAY_HEIGHT
    } else {
        1
    };

    if is_question {
        PermissionDockLayout {
            rail_width: PERMISSION_DOCK_RAIL_WIDTH,
            tray_height,
            shell_padding: EdgeInsets::new(1, 1, 0, 0),
            body_padding: EdgeInsets::ZERO,
            tray_padding: EdgeInsets::new(1, 1, 0, 0),
            header_gap: 0,
            stacked_hint_min_width: PERMISSION_DOCK_STACKED_HINT_MIN_WIDTH,
            stacked_hint_min_action_width: PERMISSION_DOCK_STACKED_HINT_MIN_ACTION_WIDTH,
        }
    } else {
        PermissionDockLayout {
            rail_width: PERMISSION_DOCK_RAIL_WIDTH,
            tray_height,
            shell_padding: EdgeInsets::new(1, 3, 1, 1),
            body_padding: EdgeInsets::new(1, 0, 0, 0),
            tray_padding: EdgeInsets::new(2, 3, 1, 1),
            header_gap: 1,
            stacked_hint_min_width: PERMISSION_DOCK_STACKED_HINT_MIN_WIDTH,
            stacked_hint_min_action_width: PERMISSION_DOCK_STACKED_HINT_MIN_ACTION_WIDTH,
        }
    }
}

pub(crate) fn inset_rect(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    let double_horizontal = horizontal.saturating_mul(2);
    let double_vertical = vertical.saturating_mul(2);
    Rect {
        x: area.x.saturating_add(horizontal),
        y: area.y.saturating_add(vertical),
        width: area.width.saturating_sub(double_horizontal),
        height: area.height.saturating_sub(double_vertical),
    }
}

pub(crate) fn pad_rect(area: Rect, insets: EdgeInsets) -> Rect {
    let horizontal = insets.left.saturating_add(insets.right);
    let vertical = insets.top.saturating_add(insets.bottom);
    Rect::new(
        area.x.saturating_add(insets.left.min(area.width)),
        area.y.saturating_add(insets.top.min(area.height)),
        area.width.saturating_sub(horizontal),
        area.height.saturating_sub(vertical),
    )
}

pub(crate) fn secondary_surface_layout(area: Rect, theme: &Theme) -> SecondarySurfaceLayout {
    let shell_tokens = theme.token_families().live_shell;
    SecondarySurfaceLayout {
        shell: area,
        body: inset_rect(
            area,
            shell_tokens.spacing.rhythm.surface_margin_x,
            shell_tokens.spacing.rhythm.surface_margin_y,
        ),
    }
}

pub(crate) fn split_secondary_surface(area: Rect, left_percent: u16, gap: u16) -> [Rect; 2] {
    if area.width == 0 || area.height == 0 {
        return [area, Rect::new(area.x, area.y, 0, area.height)];
    }

    if area.width <= SECONDARY_STACK_MAX_WIDTH && area.height >= SECONDARY_STACK_MIN_HEIGHT {
        return split_secondary_surface_vertically(area, left_percent, gap);
    }

    if area.width <= 1 {
        return [
            area,
            Rect::new(area.x.saturating_add(area.width), area.y, 0, area.height),
        ];
    }

    let actual_gap = gap.min(area.width.saturating_sub(2));
    let available_width = area.width.saturating_sub(actual_gap);
    let left_width = available_width
        .saturating_mul(left_percent)
        .saturating_div(100)
        .clamp(1, available_width.saturating_sub(1));
    let right_x = area.x.saturating_add(left_width).saturating_add(actual_gap);
    let right_width = available_width.saturating_sub(left_width);

    [
        Rect::new(area.x, area.y, left_width, area.height),
        Rect::new(right_x, area.y, right_width, area.height),
    ]
}

fn split_secondary_surface_vertically(area: Rect, top_percent: u16, gap: u16) -> [Rect; 2] {
    if area.height <= 1 {
        return [
            area,
            Rect::new(area.x, area.y.saturating_add(area.height), area.width, 0),
        ];
    }

    let actual_gap = gap.min(area.height.saturating_sub(2));
    let available_height = area.height.saturating_sub(actual_gap);
    let top_height = available_height
        .saturating_mul(top_percent)
        .saturating_div(100)
        .clamp(1, available_height.saturating_sub(1));
    let bottom_y = area.y.saturating_add(top_height).saturating_add(actual_gap);
    let bottom_height = available_height.saturating_sub(top_height);

    [
        Rect::new(area.x, area.y, area.width, top_height),
        Rect::new(area.x, bottom_y, area.width, bottom_height),
    ]
}

pub(crate) fn centered_block_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}

pub(crate) fn startup_shell_area(area: Rect, theme: &Theme) -> Rect {
    let lifecycle = theme.lifecycle_surface_layout(area.width, area.height);
    stretch_split_startup_card_to_shell(
        area,
        lifecycle_card_area(area, theme, lifecycle.startup_card),
        lifecycle.target,
    )
}

fn stretch_split_startup_card_to_shell(
    area: Rect,
    card_area: Rect,
    target: ShellGeometryTarget,
) -> Rect {
    if !matches!(target, ShellGeometryTarget::Split) {
        return card_area;
    }

    Rect::new(area.x, card_area.y, area.width, card_area.height)
}

pub(crate) fn lifecycle_card_area(area: Rect, theme: &Theme, card: LifecycleCardTokens) -> Rect {
    let shell_tokens = theme.token_families().live_shell;
    let horizontal_margin = shell_tokens
        .spacing
        .rhythm
        .surface_margin_x
        .saturating_mul(2);
    let vertical_margin = shell_tokens
        .spacing
        .rhythm
        .surface_margin_y
        .saturating_mul(2);
    let width = area
        .width
        .saturating_sub(horizontal_margin)
        .min(card.width)
        .max(1);
    let height = area
        .height
        .saturating_sub(vertical_margin)
        .min(card.height)
        .max(1);
    let centered = centered_block_area(area, width, height);
    let available_height = area.height.saturating_sub(height);
    let downward_bias = available_height.div_ceil(8).min(1);
    Rect::new(
        centered.x,
        centered.y.saturating_add(downward_bias),
        centered.width,
        centered.height,
    )
}

pub(crate) fn live_empty_state_area(area: Rect, theme: &Theme) -> Rect {
    let lifecycle = theme.lifecycle_surface_layout(area.width, area.height);
    let shell_tokens = theme.token_families().live_shell;
    let horizontal_margin = shell_tokens
        .spacing
        .rhythm
        .surface_margin_x
        .saturating_mul(2);
    let max_width = area.width.saturating_sub(horizontal_margin).max(1);
    let width = max_width
        .min(
            lifecycle
                .post_run_card
                .width
                .max(shell_tokens.copy.empty_state.max_width.saturating_add(8)),
        )
        .max(1);
    let height = lifecycle
        .post_run_card
        .height
        .max(LIVE_EMPTY_STATE_MIN_HEIGHT)
        .min(area.height)
        .max(1);
    centered_block_area(area, width, height)
}

pub(crate) fn runtime_state_surface_width(area: Rect) -> Option<u16> {
    let width = area
        .width
        .saturating_sub(RUNTIME_STATE_SURFACE_HORIZONTAL_INSET)
        .min(RUNTIME_STATE_SURFACE_MAX_WIDTH);
    (width >= RUNTIME_STATE_SURFACE_MIN_WIDTH).then_some(width)
}

pub(crate) fn runtime_state_surface_area(area: Rect, width: u16, body_height: u16) -> Option<Rect> {
    let height = area
        .height
        .saturating_sub(4)
        .min(body_height.saturating_add(3));
    if height < body_height.saturating_add(3) || height < RUNTIME_STATE_SURFACE_MIN_HEIGHT {
        return None;
    }

    Some(Rect::new(
        area.x
            .saturating_add((area.width.saturating_sub(width)) / 2),
        area.y
            .saturating_add((area.height.saturating_sub(height)) / 2),
        width,
        height,
    ))
}
