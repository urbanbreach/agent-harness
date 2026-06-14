use ratatui::layout::Rect;

use super::{SessionGeometryContract, SessionSidebarMode};
use crate::app::AppState;
use crate::theme::{LiveShellLayout, Theme};

#[cfg(test)]
use crate::theme::LifecycleSurfaceLayout;

const COMMAND_PALETTE_WIDTH: u16 = 60;
const FORK_SELECTOR_WIDTH: u16 = 88;
const SLASH_COMMAND_OVERLAY_GAP_Y: u16 = 0;

pub(super) fn session_operator_overlay(
    body: Rect,
    contract: SessionGeometryContract,
) -> Option<Rect> {
    let SessionSidebarMode::Overlay { width } = contract.sidebar_mode else {
        return None;
    };

    if body.width < 24 || body.height < 8 {
        return None;
    }

    let width = width.min(body.width.saturating_sub(2)).max(1);
    let height = body.height.max(1);
    let x = body.x.saturating_add(body.width.saturating_sub(width));
    Some(Rect::new(x, body.y, width, height))
}

pub(super) fn command_palette_overlay_area(
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
    contract: SessionGeometryContract,
    app: &AppState,
) -> Option<Rect> {
    if app.overlay_state.fork_selector_visible {
        let popup_width = FORK_SELECTOR_WIDTH.min(area.width.saturating_sub(2));
        let popup_height = fork_selector_overlay_height(app, area.height);
        if popup_width == 0 || popup_height == 0 {
            return None;
        }

        let popup_x = area
            .x
            .saturating_add((area.width.saturating_sub(popup_width)) / 2);
        let popup_y = area.y.saturating_add(area.height / 4);
        return Some(Rect::new(popup_x, popup_y, popup_width, popup_height));
    }

    if !app.overlay_state.session_history_visible && !app.overlay_state.model_switcher_visible {
        let popup_width = COMMAND_PALETTE_WIDTH.min(area.width.saturating_sub(2));
        let popup_height = command_palette_overlay_height(app, area.height)
            .min(area.height.saturating_sub(area.height / 4));
        if popup_width == 0 || popup_height == 0 {
            return None;
        }

        let popup_x = area
            .x
            .saturating_add((area.width.saturating_sub(popup_width)) / 2);
        let popup_y = area.y.saturating_add(area.height / 4);
        return Some(Rect::new(popup_x, popup_y, popup_width, popup_height));
    }

    let shell_tokens = theme.token_families().live_shell;
    let horizontal_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let vertical_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let popup_width = command_palette_overlay_width(shell, app)
        .min(contract.palette_overlay_max_width.unwrap_or(u16::MAX))
        .min(area.width.saturating_sub(horizontal_margin));
    let popup_height = command_palette_overlay_height(app, area.height)
        .min(area.height.saturating_sub(vertical_margin));

    if popup_width == 0 || popup_height == 0 {
        return None;
    }

    let popup_x = area
        .x
        .saturating_add((area.width.saturating_sub(popup_width)) / 2);
    let popup_y = area
        .y
        .saturating_add((area.height.saturating_sub(popup_height)) / 2);
    Some(Rect::new(popup_x, popup_y, popup_width, popup_height))
}

fn command_palette_overlay_width(shell: LiveShellLayout, app: &AppState) -> u16 {
    if app.overlay_state.session_history_visible || app.overlay_state.model_switcher_visible {
        shell.centered_content_width
    } else {
        COMMAND_PALETTE_WIDTH
    }
}

fn command_palette_overlay_height(app: &AppState, terminal_height: u16) -> u16 {
    const COMMAND_OVERLAY_ROWS: u16 = 6;
    const SESSION_HISTORY_OVERLAY_ROWS: u16 = 8;
    const MODEL_SWITCHER_OVERLAY_ROWS: u16 = 6;
    const MAX_LIST_ROWS: usize = 7;

    if app.overlay_state.session_history_visible {
        let max_height_rows = usize::from(terminal_height.saturating_div(2).saturating_sub(6));
        let history_rows = app
            .session_history_visual_row_count()
            .clamp(1, max_height_rows.max(1));
        let history_rows = u16::try_from(history_rows).unwrap_or(u16::MAX);
        SESSION_HISTORY_OVERLAY_ROWS.saturating_add(history_rows)
    } else if app.overlay_state.model_switcher_visible {
        let model_rows = app
            .model_switcher_visual_row_count()
            .clamp(1, MAX_LIST_ROWS + 2);
        let model_rows = u16::try_from(model_rows).unwrap_or(u16::MAX);
        MODEL_SWITCHER_OVERLAY_ROWS.saturating_add(model_rows)
    } else if app.overlay_state.toggles_menu_visible {
        let max_height_rows = usize::from(terminal_height.saturating_div(2).saturating_sub(6));
        let toggle_rows = toggles_menu_visible_rows(app)
            .max(1)
            .min(max_height_rows.max(1));
        let toggle_rows = u16::try_from(toggle_rows).unwrap_or(u16::MAX);
        COMMAND_OVERLAY_ROWS.saturating_add(toggle_rows)
    } else if app.composer.stash_dialog_visible() {
        let max_height_rows = usize::from(terminal_height.saturating_div(2).saturating_sub(6));
        let stash_rows = app
            .prompt_stash_visual_row_count()
            .min(max_height_rows.max(1));
        let stash_rows = u16::try_from(stash_rows).unwrap_or(u16::MAX);
        COMMAND_OVERLAY_ROWS.saturating_add(stash_rows)
    } else if app.composer.queued_prompt_dialog_visible() {
        let max_height_rows = usize::from(terminal_height.saturating_div(2).saturating_sub(6));
        let queued_rows = app
            .queued_prompts_visual_row_count()
            .min(max_height_rows.max(1));
        let queued_rows = u16::try_from(queued_rows).unwrap_or(u16::MAX);
        COMMAND_OVERLAY_ROWS.saturating_add(queued_rows)
    } else {
        let max_height_rows = usize::from(terminal_height.saturating_div(2).saturating_sub(6));
        let command_rows = command_palette_visible_rows(app)
            .max(1)
            .min(max_height_rows.max(1));
        let command_rows = u16::try_from(command_rows).unwrap_or(u16::MAX);
        COMMAND_OVERLAY_ROWS.saturating_add(command_rows)
    }
}

pub(super) fn fork_selector_overlay_height(app: &AppState, terminal_height: u16) -> u16 {
    const DIALOG_SELECT_CHROME_ROWS: u16 = 6;

    let max_rows = usize::from(terminal_height.saturating_div(2).saturating_sub(6)).max(1);
    let rows = app.fork_selector_view_model().rows.len().clamp(1, max_rows);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX);
    DIALOG_SELECT_CHROME_ROWS.saturating_add(rows)
}

pub(super) fn slash_command_overlay_area(
    composer: Rect,
    theme: &Theme,
    _contract: SessionGeometryContract,
    app: &AppState,
) -> Option<Rect> {
    if !(app.overlay_state.slash_visible || app.overlay_state.file_mention_visible)
        || composer.width == 0
        || composer.y == 0
    {
        return None;
    }

    let body_width = composer.width.saturating_sub(1);
    let input_padding = theme
        .live_shell
        .rhythm
        .composer_padding_x
        .min(body_width.saturating_sub(1));
    let input_x = composer.x.saturating_add(1).saturating_add(input_padding);
    let input_width = body_width.saturating_sub(input_padding.saturating_mul(2));
    if input_width == 0 {
        return None;
    }

    let popup_width = input_width.max(1);
    let popup_height = slash_command_overlay_height(app)
        .min(composer.y.saturating_sub(SLASH_COMMAND_OVERLAY_GAP_Y));
    if popup_height == 0 {
        return None;
    }

    let popup_y = composer
        .y
        .saturating_sub(SLASH_COMMAND_OVERLAY_GAP_Y)
        .saturating_sub(popup_height);
    Some(Rect::new(input_x, popup_y, popup_width, popup_height))
}

pub(crate) fn slash_command_overlay_content_area(overlay: Rect) -> Rect {
    completion_overlay_content_area(overlay)
}

pub(crate) fn completion_overlay_content_area(overlay: Rect) -> Rect {
    overlay
}

fn slash_command_overlay_height(app: &AppState) -> u16 {
    const MAX_ROWS: usize = 10;

    let len = if app.overlay_state.file_mention_visible {
        app.file_mention_entries.len()
    } else {
        app.slash_filtered.len()
    };
    let rows = len.clamp(1, MAX_ROWS);
    u16::try_from(rows).unwrap_or(u16::MAX)
}

fn command_palette_visible_rows(app: &AppState) -> usize {
    app.palette_filtered
        .iter()
        .fold((0usize, None), |(rows, last_section), command| {
            let section = crate::Action::palette_command_section(command.as_str());
            let section_rows = if section.is_some() && section != last_section {
                if last_section.is_some() {
                    2
                } else {
                    1
                }
            } else {
                0
            };
            (rows.saturating_add(section_rows).saturating_add(1), section)
        })
        .0
}

fn toggles_menu_visible_rows(app: &AppState) -> usize {
    app.toggle_menu_rows()
        .iter()
        .fold((0usize, None), |(rows, last_section), toggle| {
            let section = Some(toggle.section);
            let section_rows = if section != last_section {
                if last_section.is_some() {
                    2
                } else {
                    1
                }
            } else {
                0
            };
            (rows.saturating_add(section_rows).saturating_add(1), section)
        })
        .0
}

#[cfg(test)]
pub(super) fn lifecycle_overlay_area(
    area: Rect,
    theme: &Theme,
    lifecycle: LifecycleSurfaceLayout,
) -> Option<Rect> {
    let shell_tokens = theme.token_families().live_shell;
    let horizontal_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let vertical_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let popup_width = lifecycle
        .overlay
        .width
        .min(area.width.saturating_sub(horizontal_margin));
    let popup_height = lifecycle
        .overlay
        .height
        .min(area.height.saturating_sub(vertical_margin));

    if popup_width == 0 || popup_height == 0 {
        return None;
    }

    let popup_x = area
        .x
        .saturating_add((area.width.saturating_sub(popup_width)) / 2);
    let popup_y = area
        .y
        .saturating_add((area.height.saturating_sub(popup_height)) / 2);
    Some(Rect::new(popup_x, popup_y, popup_width, popup_height))
}
