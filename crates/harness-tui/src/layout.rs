use ratatui::layout::Rect;

use crate::app::AppState;
use crate::overlay::OverlayKind;
use crate::theme::{LiveShellLayout, Theme};
use crate::theme_tokens::DESIGN_TOKENS;

mod overlays;
mod permission;
mod session;
mod surfaces;

#[cfg(test)]
use overlays::fork_selector_overlay_height;
use overlays::{
    command_palette_overlay_area, session_operator_overlay, slash_command_overlay_area,
};
pub(crate) use overlays::{completion_overlay_content_area, slash_command_overlay_content_area};
pub(crate) use permission::{
    permission_detail_lines, permission_dock_geometry, permission_dock_measure,
    question_content_measure, question_dock_geometry, question_dock_measure,
    question_editor_viewport, question_label_column_width, question_option_visual,
    PermissionDockGeometry, PermissionDockMeasure, QuestionDockGeometry, QuestionDockMeasure,
    QUESTION_AUTO_SCROLL, QUESTION_OUTER_FOOTER_ROWS,
};
pub(crate) use surfaces::{
    inset_rect, live_empty_state_area, runtime_state_surface_area, runtime_state_surface_width,
    ControlDockLayout, HELP_MODAL_LAYOUT,
};

pub(crate) use surfaces::release_notes_modal_area;

pub(crate) fn centered_overlay_area(area: Rect, width: u16, height: u16) -> Rect {
    surfaces::centered_block_area(area, width, height)
}

const MAX_COMPOSER_LINES: u16 = 6;
const PROMPT_MIN_MAX_HEIGHT: u16 = 6;
const COMPOSER_VISIBLE_TEXT_CHROME: u16 = 6;
const DENSE_SESSION_MAX_WIDTH: u16 = 60;
const DENSE_SESSION_MAX_HEIGHT: u16 = 18;
const COMPACT_SESSION_MAX_WIDTH: u16 = 80;
const COMPACT_SESSION_MAX_HEIGHT: u16 = 24;
const LIVE_DOCK_AUTO_COMPACT_MAX_HEIGHT: u16 = 20;
const DENSE_SESSION_PALETTE_MAX_WIDTH: u16 = 46;
const STARTUP_COMPOSER_INSET_X: u16 = 2;
/// The native chat shell keeps its outer row at narrow widths too.
pub(crate) fn breadcrumb_top_margin(_width: u16) -> u16 {
    1
}

/// Total breadcrumb reserve rows (top margin + 1 breadcrumb text row).
pub(crate) fn breadcrumb_reserve_rows(width: u16) -> u16 {
    breadcrumb_top_margin(width).saturating_add(1)
}

/// Optional live-dock spacer: 0 while the live shell auto-compacts at
/// 20 rows or fewer, otherwise the canonical composer/footer spacer token.
pub(crate) fn composer_footer_spacer_rows(terminal_height: u16) -> u16 {
    if terminal_height <= LIVE_DOCK_AUTO_COMPACT_MAX_HEIGHT {
        0
    } else {
        DESIGN_TOKENS
            .breakpoints
            .all
            .iter()
            .map(|breakpoint| breakpoint.composer_footer_spacer)
            .max()
            .unwrap_or(0)
    }
}

pub(crate) fn live_turn_status_content_area(area: Rect, theme: &Theme) -> Rect {
    let inset = theme
        .token_families()
        .live_shell
        .spacing
        .rhythm
        .transcript_gutter_x
        .min(area.width);
    Rect::new(
        area.x.saturating_add(inset),
        area.y,
        area.width.saturating_sub(inset),
        area.height,
    )
}

pub(crate) fn composer_horizontal_inset(width: u16) -> u16 {
    STARTUP_COMPOSER_INSET_X.min(width.saturating_sub(4) / 2)
}

fn inset_composer_width(width: u16) -> u16 {
    width
        .saturating_sub(composer_horizontal_inset(width).saturating_mul(2))
        .max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionResponsiveMode {
    Dense,
    CompactMinimum,
    StandardMinimum,
    Split,
    Primary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionHeaderMode {
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionFooterMode {
    Standard,
    Reduced,
    Minimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionSidebarMode {
    Overlay { width: u16 },
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionGeometryContract {
    pub header_mode: SessionHeaderMode,
    pub footer_mode: SessionFooterMode,
    pub sidebar_mode: SessionSidebarMode,
    pub palette_overlay_max_width: Option<u16>,
    pub slash_overlay_max_width: Option<u16>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WheelHitAreas {
    pub transcript: Option<Rect>,
    pub terminal_panel: Option<Rect>,
    pub overlay: Option<Rect>,
    pub inspector: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayoutPlan {
    pub root: Rect,
    pub shell: Rect,
    pub header: Rect,
    pub header_text: Rect,
    pub content: Rect,
    pub live_anchor: Option<Rect>,
    pub transcript: Option<Rect>,
    pub todo: Option<Rect>,
    pub(crate) model_prompt_notice: Option<Rect>,
    pub terminal_panel: Option<Rect>,
    pub operator_sidebar: Option<Rect>,
    pub dock: Option<ControlDockLayout>,
    pub status: Option<Rect>,
    pub composer: Option<Rect>,
    pub disclosure: Option<Rect>,
    pub footer: Rect,
    pub footer_text: Rect,
    pub details_overlay: Option<Rect>,
    pub palette_overlay: Option<Rect>,
    pub slash_overlay: Option<Rect>,
    pub wheel_hit_areas: WheelHitAreas,
    pub(crate) session_contract: SessionGeometryContract,
}

impl FrameLayoutPlan {
    pub fn for_app(app: &AppState, area: Rect) -> Self {
        let theme = app.theme();
        let heights = theme.token_families().live_shell.spacing.heights;
        let layout = theme.live_shell_layout(area.width, area.height);
        let session_contract = session_geometry_contract(area, layout);
        let startup = app.startup_shell_visible();
        let child = app.current_subagent_session_present();
        let child_footer = app.review_surface().is_none()
            && !startup
            && app.active_permission().is_none()
            && child;
        let header_height =
            if startup || (app.review_surface().is_none() && (!app.replay_mode || child)) {
                0
            } else {
                heights.header
            };
        let footer_height = if child_footer {
            3
        } else if startup {
            2
        } else if app.replay_mode {
            heights.footer
        } else {
            0
        };
        let [header, content, footer] = surfaces::split_rows(area, header_height, footer_height);
        let shell = if app.replay_mode && !child {
            content
        } else {
            centered_live_shell_area(content, layout)
        };
        let header_text = if app.replay_mode {
            header
        } else {
            Rect::new(shell.x, header.y, shell.width, header.height)
        };
        let text_height = heights.footer.min(footer.height);
        let text_y = if startup {
            footer.y
        } else {
            footer
                .y
                .saturating_add(footer.height.saturating_sub(text_height) / 2)
        };
        let footer_text = if app.replay_mode {
            footer
        } else {
            Rect::new(shell.x, text_y, shell.width, text_height)
        };
        let mut plan = Self {
            root: area,
            shell,
            header,
            header_text,
            content,
            live_anchor: None,
            transcript: None,
            todo: None,
            model_prompt_notice: None,
            terminal_panel: None,
            operator_sidebar: None,
            dock: None,
            status: None,
            composer: None,
            disclosure: None,
            footer,
            footer_text,
            details_overlay: None,
            palette_overlay: None,
            slash_overlay: None,
            wheel_hit_areas: WheelHitAreas::default(),
            session_contract,
        };
        session::project(app, &mut plan, layout, child_footer);
        match app.overlay_stack().top() {
            Some(
                OverlayKind::CommandPalette
                | OverlayKind::TogglesMenu
                | OverlayKind::LineageBrowser
                | OverlayKind::ForkSelector,
            ) => {
                plan.palette_overlay =
                    command_palette_overlay_area(area, theme, layout, session_contract, app);
            }
            Some(OverlayKind::SlashCommands | OverlayKind::FileMentions) => {
                plan.slash_overlay = plan.composer.and_then(|composer| {
                    slash_command_overlay_area(composer, theme, session_contract, app)
                });
            }
            _ => {}
        }
        if !app.replay_mode
            && !child
            && !app
                .activities
                .iter()
                .any(|entry| entry.user_message.is_some())
        {
            if let (Some(message), Some(transcript)) =
                (&app.model_prompt_notice, plan.transcript.as_mut())
            {
                let width = transcript.width.saturating_sub(4);
                let rows = crate::ui::wrap_completion_text(message, usize::from(width)).len();
                let height = u16::try_from(rows)
                    .unwrap_or(u16::MAX)
                    .min(transcript.height.saturating_sub(1));
                plan.model_prompt_notice = Some(Rect::new(
                    transcript.x.saturating_add(2),
                    transcript.bottom().saturating_sub(height),
                    width,
                    height,
                ));
                transcript.height = transcript.height.saturating_sub(height);
                plan.wheel_hit_areas.transcript = Some(*transcript);
            }
        }
        plan
    }
}

pub(crate) fn session_geometry_contract(
    area: Rect,
    shell: LiveShellLayout,
) -> SessionGeometryContract {
    let mode = session_responsive_mode(area, shell);
    SessionGeometryContract {
        header_mode: SessionHeaderMode::Hidden,
        footer_mode: match mode {
            SessionResponsiveMode::Dense => SessionFooterMode::Minimal,
            SessionResponsiveMode::CompactMinimum => SessionFooterMode::Reduced,
            _ => SessionFooterMode::Standard,
        },
        sidebar_mode: if mode == SessionResponsiveMode::Dense {
            SessionSidebarMode::Hidden
        } else {
            SessionSidebarMode::Overlay {
                width: shell.details_sidebar_width,
            }
        },
        palette_overlay_max_width: (mode == SessionResponsiveMode::Dense)
            .then_some(DENSE_SESSION_PALETTE_MAX_WIDTH),
        slash_overlay_max_width: None,
    }
}

pub fn session_responsive_mode(area: Rect, shell: LiveShellLayout) -> SessionResponsiveMode {
    if area.width <= DENSE_SESSION_MAX_WIDTH && area.height <= DENSE_SESSION_MAX_HEIGHT {
        return SessionResponsiveMode::Dense;
    }

    match shell.target {
        crate::theme::ShellGeometryTarget::Primary => SessionResponsiveMode::Primary,
        crate::theme::ShellGeometryTarget::Split => SessionResponsiveMode::Split,
        crate::theme::ShellGeometryTarget::Minimum => {
            if area.width <= COMPACT_SESSION_MAX_WIDTH && area.height <= COMPACT_SESSION_MAX_HEIGHT
            {
                SessionResponsiveMode::CompactMinimum
            } else {
                SessionResponsiveMode::StandardMinimum
            }
        }
    }
}

pub(crate) fn composer_input_height(text: &str, width: u16) -> u16 {
    composer_input_height_with_max_lines(text, width, MAX_COMPOSER_LINES)
}

pub(crate) fn startup_composer_input_height(text: &str, width: u16, terminal_height: u16) -> u16 {
    composer_input_height_with_max_lines(text, width, prompt_max_height(terminal_height))
}

fn composer_input_height_with_max_lines(text: &str, width: u16, max_lines: u16) -> u16 {
    let inner_width = usize::from(width.saturating_sub(COMPOSER_VISIBLE_TEXT_CHROME).max(1));
    let mut rows = 0;
    for mut line in text.split('\n') {
        loop {
            rows += 1;
            if rows == max_lines {
                return rows;
            }
            let (_, next) = crate::text::composer_row_end(line, inner_width);
            line = &line[next..];
            if line.is_empty() {
                break;
            }
        }
    }
    rows
}

fn prompt_max_height(terminal_height: u16) -> u16 {
    PROMPT_MIN_MAX_HEIGHT.max(terminal_height / 3)
}

pub(crate) fn todo_close_rect(area: Rect) -> Rect {
    Rect::new(area.right(), area.y.saturating_sub(1), 1, 1)
}

fn centered_live_shell_area(area: Rect, shell: LiveShellLayout) -> Rect {
    let max_width = area
        .width
        .saturating_sub(shell.content_margin_x.saturating_mul(2));
    if max_width == 0 {
        return area;
    }

    let width = match shell.target {
        crate::theme::ShellGeometryTarget::Minimum
            if area.width < crate::theme::ShellGeometry::SPLIT.width =>
        {
            max_width.min(shell.centered_content_width).max(1)
        }
        crate::theme::ShellGeometryTarget::Minimum
        | crate::theme::ShellGeometryTarget::Split
        | crate::theme::ShellGeometryTarget::Primary => max_width.max(1),
    };
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    Rect::new(x, area.y, width, area.height)
}

#[cfg(test)]
#[path = "layout_live_dock_test_fixtures.rs"]
mod live_dock_test_fixtures;
#[cfg(test)]
#[path = "layout_live_dock_tests.rs"]
mod live_dock_tests;
#[cfg(test)]
mod tests;
