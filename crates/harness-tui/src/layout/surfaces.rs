use ratatui::layout::Rect;

use crate::theme::Theme;

const LIVE_EMPTY_STATE_MIN_HEIGHT: u16 = 9;
const RUNTIME_STATE_SURFACE_HORIZONTAL_INSET: u16 = 6;
const RUNTIME_STATE_SURFACE_MAX_WIDTH: u16 = 68;
const RUNTIME_STATE_SURFACE_MIN_WIDTH: u16 = 32;
const RUNTIME_STATE_SURFACE_MIN_HEIGHT: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlDockLayout {
    pub shell: Rect,
    pub status: Option<Rect>,
    pub composer: Rect,
    pub disclosure: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HelpModalLayout {
    pub width_numerator: u32,
    pub width_denominator: u32,
    pub max_width: u16,
    pub min_width: u16,
    pub vertical_margin: u16,
}

pub(crate) const HELP_MODAL_LAYOUT: HelpModalLayout = HelpModalLayout {
    width_numerator: 7,
    width_denominator: 10,
    max_width: 80,
    min_width: 44,
    vertical_margin: 4,
};

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

pub(crate) fn centered_block_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}

pub(crate) fn release_notes_modal_area(area: Rect) -> Rect {
    let width = area
        .width
        .saturating_mul(80)
        .saturating_div(100)
        .clamp(44.min(area.width), 120.min(area.width));
    let compact = area.height <= 20;
    let height = if compact {
        area.height
    } else {
        area.height.saturating_sub(8)
    };
    centered_block_area(area, width, height)
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

/// Fixed header/footer rows around the remaining body; the footer wins at tiny heights.
/// Preserve raw horizontal fields, as vertical Layout does for saturated rectangles.
pub(super) fn split_rows(area: Rect, top: u16, bottom: u16) -> [Rect; 3] {
    let height = area.bottom().saturating_sub(area.y);
    let bottom = bottom.min(height);
    let top = top.min(height.saturating_sub(bottom));
    let middle = height.saturating_sub(top).saturating_sub(bottom);
    [
        Rect {
            height: top,
            ..area
        },
        Rect {
            y: area.y.saturating_add(top),
            height: middle,
            ..area
        },
        Rect {
            y: area.bottom().saturating_sub(bottom),
            height: bottom,
            ..area
        },
    ]
}

pub(super) fn reserve_sidebar_gap(dock: ControlDockLayout, gap: u16) -> ControlDockLayout {
    let shrink = |area: Rect| Rect {
        width: area.width.saturating_sub(gap),
        ..area
    };
    ControlDockLayout {
        status: dock.status.map(shrink),
        composer: shrink(dock.composer),
        disclosure: dock.disclosure.map(shrink),
        ..dock
    }
}

pub(super) fn inset_dock(dock: ControlDockLayout, area: Rect, startup: bool) -> ControlDockLayout {
    if dock.shell.width == 0 || dock.shell.height == 0 {
        return dock;
    }
    let inset = if startup && area.width <= super::DENSE_SESSION_MAX_WIDTH {
        0
    } else {
        super::composer_horizontal_inset(area.width)
    };
    if !startup && inset == 0 {
        return dock;
    }
    let width = if startup {
        area.width
    } else {
        dock.shell.width
    }
    .saturating_sub(inset.saturating_mul(2))
    .max(1);
    let x = if startup { area.x } else { dock.shell.x }.saturating_add(inset);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(dock.shell.height));
    let map = |band: Rect| {
        if startup {
            Rect::new(
                x,
                y.saturating_add(band.y.saturating_sub(dock.shell.y)),
                width,
                band.height,
            )
        } else {
            Rect { x, width, ..band }
        }
    };
    ControlDockLayout {
        shell: map(dock.shell),
        status: dock.status.map(map),
        composer: map(dock.composer),
        disclosure: dock.disclosure.map(map),
    }
}
