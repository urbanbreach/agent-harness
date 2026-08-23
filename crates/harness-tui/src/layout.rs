// allow: SIZE_OK — TUI layout math (frame plan + pane sizing)
use crate::UnwrapOrAbort;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
};

use crate::app::{AppState, Focus};
use crate::overlay::OverlayKind;
use crate::theme::{LiveShellLayout, Theme};
use crate::theme_tokens::{ViewportBreakpoint, DESIGN_TOKENS};

mod overlays;
mod permission;
mod surfaces;

use overlays::{
    command_palette_overlay_area, session_operator_overlay, slash_command_overlay_area,
};
pub(crate) use overlays::{completion_overlay_content_area, slash_command_overlay_content_area};
#[cfg(test)]
use overlays::{fork_selector_overlay_height, lifecycle_overlay_area};
pub(crate) use permission::{
    permission_detail_lines, permission_dock_geometry, permission_dock_measure,
    question_dock_geometry, question_dock_measure, question_label_column_width,
    question_option_visual, PermissionDockGeometry, PermissionDockMeasure, QuestionDockGeometry,
    QuestionDockMeasure, QUESTION_AUTO_SCROLL, QUESTION_OUTER_FOOTER_ROWS,
};
#[cfg(test)]
pub(crate) use surfaces::lifecycle_card_area;
#[cfg(test)]
use surfaces::split_secondary_surface;
pub(crate) use surfaces::{
    inset_rect, live_empty_state_area, pad_rect, permission_dock_layout,
    runtime_state_surface_area, runtime_state_surface_width, secondary_surface_layout,
    startup_shell_area, ControlDockLayout, EdgeInsets, HELP_MODAL_LAYOUT,
};

pub(crate) use surfaces::release_notes_modal_area;

pub(crate) fn centered_overlay_area(area: Rect, width: u16, height: u16) -> Rect {
    surfaces::centered_block_area(area, width, height)
}

const MIN_COMPOSER_LINES: u16 = 1;
const MAX_COMPOSER_LINES: u16 = 6;
const PROMPT_MIN_MAX_HEIGHT: u16 = 6;
const COMPOSER_VISIBLE_TEXT_CHROME: u16 = 6;
const LIVE_DETAILS_MIN_TRANSCRIPT_WIDTH: u16 = 48;
const TERMINAL_PANEL_MIN_TRANSCRIPT_HEIGHT: u16 = 7;
const TERMINAL_PANEL_MIN_HEIGHT: u16 = 5;
const TERMINAL_PANEL_MAX_HEIGHT: u16 = 12;
const DENSE_SESSION_MAX_WIDTH: u16 = 60;
const DENSE_SESSION_MAX_HEIGHT: u16 = 18;
const COMPACT_SESSION_MAX_WIDTH: u16 = 80;
const COMPACT_SESSION_MAX_HEIGHT: u16 = 24;
const LIVE_DOCK_AUTO_COMPACT_MAX_HEIGHT: u16 = 20;
const DENSE_SESSION_PALETTE_MAX_WIDTH: u16 = 46;
const STARTUP_COMPOSER_INSET_X: u16 = 2;
const STARTUP_BORDERED_COMPOSER_CHROME_ROWS: u16 = 2;
const STARTUP_COMPOSER_SPACER_ROWS: u16 = 1;
const STARTUP_FOOTER_ROWS: u16 = 2;
const SUBAGENT_FOOTER_ROWS: u16 = 3;
/// Spacer between the composer bottom border and the disclosure/footer row.
/// Present at all viewports wider than the dense (60-col) compact cutoff;
/// suppressed at ultra-compact sizes to maximize transcript space.
/// Measured live dock order, top to bottom: an optional reserved status row,
/// composer, composer/footer spacer, disclosure, and trailing blank margin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveDockRhythm {
    status_rows: u16,
    status_composer_spacer_rows: u16,
    composer_footer_spacer_rows: u16,
    disclosure_rows: u16,
    bottom_margin_rows: u16,
}

/// Breadcrumb top-margin rows: 1 at standard/compact viewports (>60 cols),
/// 0 at ultra-compact (≤60 cols) so the breadcrumb sits on row 1.
pub(crate) fn breadcrumb_top_margin(width: u16) -> u16 {
    if width <= DENSE_SESSION_MAX_WIDTH {
        0
    } else {
        DESIGN_TOKENS
            .breakpoints
            .all
            .iter()
            .find(|breakpoint| breakpoint.width > DENSE_SESSION_MAX_WIDTH)
            .map_or(0, |breakpoint| breakpoint.breadcrumb_top_margin)
    }
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
    let preferred = if width <= DENSE_SESSION_MAX_WIDTH {
        1
    } else {
        DESIGN_TOKENS
            .breakpoints
            .all
            .iter()
            .find(|breakpoint| breakpoint.width > DENSE_SESSION_MAX_WIDTH)
            .map_or(0, |breakpoint| breakpoint.composer_inset)
    };
    preferred.min(width.saturating_sub(4) / 2)
}

fn startup_composer_horizontal_inset(width: u16) -> u16 {
    if width <= DENSE_SESSION_MAX_WIDTH {
        0
    } else {
        composer_horizontal_inset(width)
    }
}

fn inset_composer_width(width: u16) -> u16 {
    width
        .saturating_sub(composer_horizontal_inset(width).saturating_mul(2))
        .max(1)
}

fn design_breakpoint(width: u16) -> ViewportBreakpoint {
    DESIGN_TOKENS
        .breakpoints
        .all
        .iter()
        .rev()
        .find(|breakpoint| width >= breakpoint.width)
        .copied()
        .unwrap_or(DESIGN_TOKENS.breakpoints.all[0])
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
pub(crate) struct SessionShellLayout {
    pub live_anchor: Option<Rect>,
    pub transcript: Rect,
    pub terminal_panel: Option<Rect>,
    pub operator_sidebar: Option<Rect>,
    pub operator_sidebar_compact_empty: bool,
    pub operator_overlay: Option<Rect>,
    pub activity: Option<Rect>,
    pub inspector: Option<Rect>,
    pub dock: ControlDockLayout,
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
        let shell_tokens = theme.token_families().live_shell;
        let shell_layout = theme.live_shell_layout(area.width, area.height);
        let session_contract = session_geometry_contract(area, shell_layout);
        let header_height = if hide_session_header(app, session_contract) {
            0
        } else {
            shell_tokens.spacing.heights.header
        };
        let subagent_footer_visible = subagent_footer_visible(app);
        let footer_height = if subagent_footer_visible {
            SUBAGENT_FOOTER_ROWS
        } else if app.startup_shell_visible() {
            STARTUP_FOOTER_ROWS
        } else if app.replay_mode {
            shell_tokens.spacing.heights.footer
        } else {
            0
        };

        let root_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Min(0),
                Constraint::Length(footer_height),
            ])
            .split(area);

        let content = root_chunks[1];
        let header = root_chunks[0];
        let footer = root_chunks[2];
        let shell = if app.replay_mode && !app.current_subagent_session_present() {
            content
        } else {
            centered_live_shell_area(content, shell_layout)
        };

        let header_text = if app.replay_mode {
            header
        } else {
            Rect::new(shell.x, header.y, shell.width, header.height)
        };
        let footer_text = if app.replay_mode {
            footer
        } else if app.startup_shell_visible() {
            let footer_text_height = shell_tokens.spacing.heights.footer.min(footer.height);
            Rect::new(shell.x, footer.y, shell.width, footer_text_height)
        } else {
            let footer_text_height = shell_tokens.spacing.heights.footer.min(footer.height);
            let footer_text_y = footer
                .y
                .saturating_add(footer.height.saturating_sub(footer_text_height) / 2);
            Rect::new(shell.x, footer_text_y, shell.width, footer_text_height)
        };

        let mut plan = Self {
            root: area,
            shell,
            header,
            header_text,
            content,
            live_anchor: None,
            transcript: None,
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

        let session = session_shell_layout(
            app,
            plan.shell,
            theme,
            shell_layout,
            session_contract,
            area.height,
        );
        plan.live_anchor = session.live_anchor;
        plan.transcript = Some(session.transcript);
        plan.terminal_panel = session.terminal_panel;
        plan.operator_sidebar = session.operator_sidebar;
        plan.dock = Some(session.dock);
        plan.status = session.dock.status;
        plan.composer = Some(session.dock.composer);
        plan.disclosure = session.dock.disclosure;
        plan.details_overlay = session.operator_overlay;
        plan.palette_overlay = matches!(
            app.overlay_stack().top(),
            Some(
                OverlayKind::CommandPalette
                    | OverlayKind::TogglesMenu
                    | OverlayKind::LineageBrowser
                    | OverlayKind::ForkSelector
            )
        )
        .then(|| {
            let overlay_area = Rect::new(
                plan.content.x,
                plan.content.y,
                plan.content.width,
                session.dock.composer.y.saturating_sub(plan.content.y),
            );
            command_palette_overlay_area(overlay_area, theme, shell_layout, session_contract, app)
        })
        .flatten();
        plan.slash_overlay = matches!(
            app.overlay_stack().top(),
            Some(OverlayKind::SlashCommands | OverlayKind::FileMentions)
        )
        .then(|| slash_command_overlay_area(session.dock.composer, theme, session_contract, app))
        .flatten();
        plan.wheel_hit_areas = WheelHitAreas {
            transcript: Some(session.transcript),
            terminal_panel: session.terminal_panel,
            overlay: (!session.operator_sidebar_compact_empty)
                .then_some(session.operator_sidebar.or(session.operator_overlay))
                .flatten(),
            inspector: session.inspector,
        };

        if app.replay_mode {
            plan.wheel_hit_areas = WheelHitAreas {
                transcript: Some(session.transcript),
                terminal_panel: session.terminal_panel,
                overlay: (!session.operator_sidebar_compact_empty)
                    .then_some(session.operator_sidebar.or(session.operator_overlay))
                    .flatten(),
                inspector: session.inspector,
            };
            return plan;
        }

        plan
    }
}

pub(crate) fn session_geometry_contract(
    area: Rect,
    shell: LiveShellLayout,
) -> SessionGeometryContract {
    match session_responsive_mode(area, shell) {
        SessionResponsiveMode::Dense => SessionGeometryContract {
            header_mode: SessionHeaderMode::Hidden,
            footer_mode: SessionFooterMode::Minimal,
            sidebar_mode: SessionSidebarMode::Hidden,
            palette_overlay_max_width: Some(DENSE_SESSION_PALETTE_MAX_WIDTH),
            slash_overlay_max_width: None,
        },
        SessionResponsiveMode::CompactMinimum => SessionGeometryContract {
            header_mode: SessionHeaderMode::Hidden,
            footer_mode: SessionFooterMode::Reduced,
            sidebar_mode: SessionSidebarMode::Overlay {
                width: shell.details_sidebar_width,
            },
            palette_overlay_max_width: None,
            slash_overlay_max_width: None,
        },
        SessionResponsiveMode::StandardMinimum => SessionGeometryContract {
            header_mode: SessionHeaderMode::Hidden,
            footer_mode: SessionFooterMode::Standard,
            sidebar_mode: SessionSidebarMode::Overlay {
                width: shell.details_sidebar_width,
            },
            palette_overlay_max_width: None,
            slash_overlay_max_width: None,
        },
        SessionResponsiveMode::Split => SessionGeometryContract {
            header_mode: SessionHeaderMode::Hidden,
            footer_mode: SessionFooterMode::Standard,
            sidebar_mode: SessionSidebarMode::Overlay {
                width: shell.details_sidebar_width,
            },
            palette_overlay_max_width: None,
            slash_overlay_max_width: None,
        },
        SessionResponsiveMode::Primary => SessionGeometryContract {
            header_mode: SessionHeaderMode::Hidden,
            footer_mode: SessionFooterMode::Standard,
            sidebar_mode: SessionSidebarMode::Overlay {
                width: shell.details_sidebar_width,
            },
            palette_overlay_max_width: None,
            slash_overlay_max_width: None,
        },
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

fn hide_session_header(app: &AppState, contract: SessionGeometryContract) -> bool {
    if app.startup_shell_visible() {
        return true;
    }
    // Permission/question docks keep the waiting-state headerless shell: they begin
    // at the breadcrumb row, not `run … · profile/provider · model`.
    app.review_surface().is_none()
        && matches!(contract.header_mode, SessionHeaderMode::Hidden)
        && (!app.replay_mode || app.current_subagent_session_present())
}

fn subagent_footer_visible(app: &AppState) -> bool {
    app.review_surface().is_none()
        && !app.startup_shell_visible()
        && app.active_permission().is_none()
        && app.current_subagent_session_present()
}

pub(crate) fn session_shell_layout(
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
    contract: SessionGeometryContract,
    terminal_height: u16,
) -> SessionShellLayout {
    let shell_tokens = theme.token_families().live_shell;
    let gap = shell_tokens.spacing.rhythm.surface_gap / 2;
    let min_transcript_width = shell
        .transcript_min_width
        .max(LIVE_DETAILS_MIN_TRANSCRIPT_WIDTH);
    let (content_column, operator_sidebar) =
        if app.startup_shell_visible() || app.current_subagent_session_present() {
            (area, None)
        } else if app.replay_mode {
            let sidebar_width = shell
                .details_sidebar_width
                .min(area.width.saturating_sub(1))
                .max(1);
            if area.width
                >= min_transcript_width
                    .saturating_add(gap)
                    .saturating_add(sidebar_width)
            {
                let content_width = area.width.saturating_sub(sidebar_width).saturating_sub(gap);
                let sidebar_x = area.x.saturating_add(content_width).saturating_add(gap);
                (
                    Rect::new(area.x, area.y, content_width, area.height),
                    Some(Rect::new(sidebar_x, area.y, sidebar_width, area.height)),
                )
            } else {
                (area, None)
            }
        } else {
            match contract.sidebar_mode {
                SessionSidebarMode::Overlay { .. } | SessionSidebarMode::Hidden => (area, None),
            }
        };
    let subagent_footer_visible = subagent_footer_visible(app);
    let composer_measure_area = Rect {
        width: inset_composer_width(content_column.width),
        ..content_column
    };
    let prompt_height = if subagent_footer_visible {
        0
    } else if app.replay_mode {
        shell_tokens.spacing.heights.prompt_block()
    } else if app.startup_shell_visible() {
        live_prompt_block_height(app, composer_measure_area, contract, shell, terminal_height)
    } else {
        let rhythm = live_dock_rhythm(
            app,
            contract,
            content_column.width,
            shell_tokens.spacing.heights.status,
            terminal_height,
        );
        live_prompt_block_height(app, composer_measure_area, contract, shell, terminal_height)
            .saturating_add(rhythm.status_composer_spacer_rows)
            .saturating_add(rhythm.composer_footer_spacer_rows)
            .saturating_add(rhythm.status_rows)
            .saturating_add(rhythm.bottom_margin_rows)
            .saturating_add(rhythm.disclosure_rows)
    };

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(prompt_height)])
        .split(content_column);

    let body = main_chunks[0];
    let dock = if app.replay_mode {
        control_dock_layout(main_chunks[1], None, main_chunks[1], None)
    } else {
        let shell_area = main_chunks[1];
        let rhythm = live_dock_rhythm(
            app,
            contract,
            content_column.width,
            shell_tokens.spacing.heights.status,
            terminal_height,
        );
        let disclosure_height = rhythm
            .disclosure_rows
            .min(shell_area.height.saturating_sub(1));
        if app.startup_shell_visible() {
            let composer_height = shell_area
                .height
                .saturating_sub(STARTUP_COMPOSER_SPACER_ROWS)
                .max(3)
                .min(shell_area.height);
            let composer = Rect {
                x: shell_area.x,
                y: shell_area.y,
                width: shell_area.width,
                height: composer_height,
            };
            control_dock_layout(shell_area, None, composer, None)
        } else {
            let composer_height = live_prompt_block_height(
                app,
                composer_measure_area,
                contract,
                shell,
                terminal_height,
            )
            .min(shell_area.height);
            let status_rows = rhythm
                .status_rows
                .min(shell_area.height.saturating_sub(composer_height));
            let composer_footer_spacer = rhythm
                .composer_footer_spacer_rows
                .min(shell_area.height.saturating_sub(status_rows));
            let status_composer_spacer = rhythm
                .status_composer_spacer_rows
                .min(shell_area.height.saturating_sub(status_rows));
            let bottom_margin = rhythm.bottom_margin_rows.min(
                shell_area.height.saturating_sub(
                    status_rows
                        .saturating_add(status_composer_spacer)
                        .saturating_add(composer_height)
                        .saturating_add(composer_footer_spacer)
                        .saturating_add(disclosure_height),
                ),
            );
            let shell_bottom = shell_area.y.saturating_add(shell_area.height);
            let disclosure_y = shell_bottom
                .saturating_sub(bottom_margin)
                .saturating_sub(disclosure_height);
            let composer_y = disclosure_y
                .saturating_sub(composer_footer_spacer)
                .saturating_sub(composer_height);
            let status_y = composer_y
                .saturating_sub(status_composer_spacer)
                .saturating_sub(status_rows);
            let composer = Rect::new(shell_area.x, composer_y, shell_area.width, composer_height);
            let disclosure = (disclosure_height > 0).then_some(Rect::new(
                shell_area.x,
                disclosure_y,
                shell_area.width,
                disclosure_height,
            ));
            let status = (status_rows > 0).then_some(Rect::new(
                shell_area.x,
                status_y,
                shell_area.width,
                status_rows,
            ));
            control_dock_layout(shell_area, status, composer, disclosure)
        }
    };
    let dock = if operator_sidebar.is_some() {
        dock_with_sidebar_gap(dock, shell_tokens.spacing.rhythm.transcript_gutter_x)
    } else {
        dock
    };
    if app.startup_shell_visible() {
        return SessionShellLayout {
            live_anchor: None,
            transcript: body,
            terminal_panel: None,
            operator_sidebar: None,
            operator_sidebar_compact_empty: false,
            operator_overlay: None,
            activity: None,
            inspector: None,
            dock: centered_startup_dock_layout(dock, area, body, shell, theme),
        };
    }

    let operator_sidebar_compact_empty =
        operator_sidebar.is_some() && !app.operator_rail_has_sections();
    let (transcript, terminal_panel) = terminal_panel_split(app, body, gap);
    let operator_overlay = if operator_sidebar.is_some() {
        None
    } else if app.details_drawer_open() {
        session_operator_overlay(body, contract)
    } else {
        None
    };

    let operator_frame = operator_sidebar.or(operator_overlay);
    let activity = None;
    let inspector = (!operator_sidebar_compact_empty)
        .then_some(operator_frame)
        .flatten();

    SessionShellLayout {
        live_anchor: None,
        transcript,
        terminal_panel,
        operator_sidebar,
        operator_sidebar_compact_empty,
        operator_overlay,
        activity,
        inspector,
        dock: dock_with_horizontal_inset(dock, content_column),
    }
}

fn terminal_panel_split(app: &AppState, body: Rect, gap: u16) -> (Rect, Option<Rect>) {
    if !app.terminal_panel_visible() || app.startup_shell_visible() || body.width == 0 {
        return (body, None);
    }

    let actual_gap = gap.min(body.height.saturating_sub(2));
    let required_height = TERMINAL_PANEL_MIN_TRANSCRIPT_HEIGHT
        .saturating_add(actual_gap)
        .saturating_add(TERMINAL_PANEL_MIN_HEIGHT);
    if body.height < required_height {
        return (body, None);
    }

    let available_terminal_height = body
        .height
        .saturating_sub(actual_gap)
        .saturating_sub(TERMINAL_PANEL_MIN_TRANSCRIPT_HEIGHT);
    let terminal_height = body
        .height
        .saturating_div(3)
        .clamp(TERMINAL_PANEL_MIN_HEIGHT, TERMINAL_PANEL_MAX_HEIGHT)
        .min(available_terminal_height);
    let transcript_height = body
        .height
        .saturating_sub(actual_gap)
        .saturating_sub(terminal_height);
    let transcript = Rect::new(body.x, body.y, body.width, transcript_height);
    let terminal = Rect::new(
        body.x,
        body.y
            .saturating_add(transcript_height)
            .saturating_add(actual_gap),
        body.width,
        terminal_height,
    );

    (transcript, Some(terminal))
}

pub(crate) fn composer_input_height(text: &str, width: u16) -> u16 {
    composer_input_height_with_max_lines(text, width, MAX_COMPOSER_LINES)
}

pub(crate) fn startup_composer_input_height(text: &str, width: u16, terminal_height: u16) -> u16 {
    composer_input_height_with_max_lines(text, width, prompt_max_height(terminal_height))
}

fn composer_input_height_with_max_lines(text: &str, width: u16, max_lines: u16) -> u16 {
    let inner_width = usize::from(width.saturating_sub(COMPOSER_VISIBLE_TEXT_CHROME).max(1));
    let wrapped_lines = if text.is_empty() {
        1
    } else {
        text.split('\n')
            .map(|line| word_wrapped_line_count(line, inner_width))
            .sum()
    };

    let clamped_lines =
        wrapped_lines.clamp(usize::from(MIN_COMPOSER_LINES), usize::from(max_lines));
    u16::try_from(clamped_lines).unwrap_or(max_lines)
}

fn prompt_max_height(terminal_height: u16) -> u16 {
    PROMPT_MIN_MAX_HEIGHT.max(terminal_height / 3)
}

fn display_width(text: &str) -> usize {
    Line::from(text.to_string()).width()
}

fn word_wrapped_line_count(line: &str, width: usize) -> usize {
    if line.is_empty() {
        return 1;
    }

    let chars = line
        .chars()
        .map(|ch| (ch, display_width(&ch.to_string()).max(1)))
        .collect::<Vec<_>>();
    let mut count = 0usize;
    let mut start = 0usize;
    while start < chars.len() {
        count += 1;
        let fit_end = word_wrap_fit_end(&chars, start, width.max(1));
        if fit_end >= chars.len() {
            break;
        }

        if let Some(break_at) = chars[start..fit_end]
            .iter()
            .rposition(|(ch, _)| ch.is_whitespace())
            .map(|offset| start + offset)
            .filter(|break_at| *break_at > start)
        {
            start = break_at + 1;
        } else if chars[fit_end].0.is_whitespace() {
            start = fit_end + 1;
        } else {
            start = fit_end.max(start + 1);
        }
    }

    count.max(1)
}

fn word_wrap_fit_end(chars: &[(char, usize)], start: usize, width: usize) -> usize {
    let mut used = 0usize;
    for (position, (_, char_width)) in chars.iter().enumerate().skip(start) {
        if position > start && used.saturating_add(*char_width) > width {
            return position;
        }
        used = used.saturating_add(*char_width);
    }
    chars.len()
}

fn live_dock_rhythm(
    app: &AppState,
    contract: SessionGeometryContract,
    width: u16,
    status_row_height: u16,
    terminal_height: u16,
) -> LiveDockRhythm {
    let disclosure_rows = control_dock_disclosure_rows(app, contract);
    let active_permission = app.active_permission_view();
    let status_rows = if let Some(permission) = active_permission.as_ref() {
        permission_prompt_block_height(
            app,
            inset_composer_width(width),
            terminal_height,
            permission,
        )
    } else if app.live_turn_status_visible() {
        status_row_height
    } else {
        0
    };
    let outer_spacer_rows = composer_footer_spacer_rows(terminal_height);

    LiveDockRhythm {
        status_rows,
        status_composer_spacer_rows: outer_spacer_rows,
        composer_footer_spacer_rows: outer_spacer_rows,
        disclosure_rows,
        bottom_margin_rows: outer_spacer_rows,
    }
}

fn permission_prompt_block_height(
    app: &AppState,
    width: u16,
    terminal_height: u16,
    permission: &crate::app::ActivePermissionView,
) -> u16 {
    if permission.question_prompts.is_some() {
        return question_dock_measure(app, width, terminal_height, permission).status_height;
    }

    permission_dock_measure(app, width, terminal_height, permission).height
}

fn live_prompt_block_height(
    app: &AppState,
    area: Rect,
    _contract: SessionGeometryContract,
    _shell: LiveShellLayout,
    terminal_height: u16,
) -> u16 {
    let max_block_height = area.height;
    let startup_shell = app.startup_shell_visible();

    if startup_shell {
        let input_height =
            startup_composer_input_height(&app.composer.prompt_buffer, area.width, terminal_height)
                .max(1);
        return input_height
            .saturating_add(STARTUP_BORDERED_COMPOSER_CHROME_ROWS)
            .saturating_add(STARTUP_COMPOSER_SPACER_ROWS)
            .min(max_block_height)
            .max(3 + STARTUP_COMPOSER_SPACER_ROWS);
    }

    if app.focus != crate::app::Focus::Prompt
        && app.composer.prompt_buffer.is_empty()
        && app.active_permission_view().is_none()
        && !app.completed_session_shell_active()
    {
        return 1.min(max_block_height);
    }

    let input_height = composer_input_height(&app.composer.prompt_buffer, area.width).max(1);
    input_height
        .saturating_add(STARTUP_BORDERED_COMPOSER_CHROME_ROWS)
        .min(max_block_height)
        .max(3)
}

fn control_dock_disclosure_rows(app: &AppState, _contract: SessionGeometryContract) -> u16 {
    if app.replay_mode
        || app.startup_shell_visible()
        || app.review_surface().is_some()
        || app.active_permission_view().is_some()
    {
        return 0;
    }

    1
}

fn control_dock_layout(
    shell: Rect,
    status: Option<Rect>,
    composer: Rect,
    disclosure: Option<Rect>,
) -> ControlDockLayout {
    ControlDockLayout {
        shell,
        status,
        composer,
        disclosure,
    }
}

fn dock_with_sidebar_gap(dock: ControlDockLayout, gap_width: u16) -> ControlDockLayout {
    fn reserve_gap(area: Rect, gap_width: u16) -> Rect {
        Rect {
            width: area.width.saturating_sub(gap_width.min(area.width)),
            ..area
        }
    }

    ControlDockLayout {
        status: dock.status.map(|status| reserve_gap(status, gap_width)),
        composer: reserve_gap(dock.composer, gap_width),
        disclosure: dock
            .disclosure
            .map(|disclosure| reserve_gap(disclosure, gap_width)),
        ..dock
    }
}

fn dock_with_horizontal_inset(dock: ControlDockLayout, area: Rect) -> ControlDockLayout {
    if dock.shell.width == 0 || dock.shell.height == 0 {
        return dock;
    }

    let inset = composer_horizontal_inset(area.width);
    if inset == 0 {
        return dock;
    }

    let width = dock
        .shell
        .width
        .saturating_sub(inset.saturating_mul(2))
        .max(1);
    let x = dock.shell.x.saturating_add(inset);
    let map_band = |band: Rect| Rect {
        x,
        y: band.y,
        width,
        height: band.height,
    };

    control_dock_layout(
        Rect {
            x,
            y: dock.shell.y,
            width,
            height: dock.shell.height,
        },
        dock.status.map(map_band),
        map_band(dock.composer),
        dock.disclosure.map(map_band),
    )
}

fn centered_startup_dock_layout(
    dock: ControlDockLayout,
    area: Rect,
    _transcript: Rect,
    _shell: LiveShellLayout,
    _theme: &Theme,
) -> ControlDockLayout {
    if dock.shell.width == 0 || dock.shell.height == 0 {
        return dock;
    }

    let inset = startup_composer_horizontal_inset(area.width);
    let width = area.width.saturating_sub(inset.saturating_mul(2)).max(1);
    let x = area.x.saturating_add(inset);
    let shell_height = dock.shell.height;
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(shell_height));
    let shell_rect = Rect::new(x, y, width, shell_height);
    let shell_origin_y = dock.shell.y;
    let map_band = |band: Rect| {
        Rect::new(
            x,
            y.saturating_add(band.y.saturating_sub(shell_origin_y)),
            width.min(band.width.max(width)),
            band.height,
        )
    };

    control_dock_layout(
        shell_rect,
        dock.status.map(map_band),
        map_band(dock.composer),
        dock.disclosure.map(map_band),
    )
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
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn split_secondary_surface_stacks_vertically_in_narrow_tall_windows() {
        // arrange
        // act
        // assert
        let [top, bottom] = split_secondary_surface(Rect::new(0, 0, 60, 24), 40, 1);

        assert_eq!(top, Rect::new(0, 0, 60, 9));
        assert_eq!(bottom, Rect::new(0, 10, 60, 14));
    }

    #[test]
    fn split_secondary_surface_keeps_horizontal_layout_when_width_allows() {
        // arrange
        // act
        // assert
        let [left, right] = split_secondary_surface(Rect::new(0, 0, 100, 24), 40, 1);

        assert_eq!(left, Rect::new(0, 0, 39, 24));
        assert_eq!(right, Rect::new(40, 0, 60, 24));
    }

    #[test]
    fn lifecycle_overlay_stays_centered_in_minimum_geometry() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let overlay = lifecycle_overlay_area(
            Rect::new(0, 1, 80, 22),
            &theme,
            theme.lifecycle_surface_layout(80, 22),
        )
        .unwrap_or_abort();

        assert_eq!(overlay, Rect::new(2, 6, 76, 11));
    }

    #[test]
    fn replay_session_layout_never_reserves_live_anchor() {
        // arrange
        // act
        // assert
        let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), Vec::new());
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 100, 30));

        assert!(plan.live_anchor.is_none());
    }

    #[test]
    fn startup_composer_input_height_uses_harness_terminal_scaled_cap() {
        // arrange
        // act
        // assert
        let text = "line\n".repeat(20);

        assert_eq!(startup_composer_input_height(&text, 75, 18), 6);
        assert_eq!(startup_composer_input_height(&text, 75, 48), 16);
    }

    #[test]
    fn startup_dock_is_bottom_aligned_with_horizontal_inset() {
        // arrange
        // act
        // assert
        let app = AppState::new_startup(Vec::new(), None);
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 120, 32));
        let dock = plan.dock.unwrap_or_abort();

        assert_eq!(
            dock.shell.x,
            plan.shell.x.saturating_add(STARTUP_COMPOSER_INSET_X)
        );
        assert_eq!(
            dock.shell.width,
            plan.shell
                .width
                .saturating_sub(STARTUP_COMPOSER_INSET_X.saturating_mul(2))
        );
        assert_eq!(
            dock.shell.y,
            plan.shell
                .y
                .saturating_add(plan.shell.height.saturating_sub(dock.shell.height)),
            "startup composer docks to the bottom of the content shell"
        );
        assert_eq!(
            dock.shell.height,
            3 + STARTUP_COMPOSER_SPACER_ROWS,
            "single-line startup dock is 3-row composer plus spacer"
        );
        assert_eq!(
            dock.composer.height, 3,
            "single-line startup composer content is 3 rows"
        );
    }

    #[test]
    fn live_post_turn_dock_keeps_horizontal_inset_matching_freeze() {
        // arrange
        // act
        // assert
        // Given: live shell (not startup) at freeze-primary 120×40
        let app = AppState::new_live(None, false, None);
        assert!(
            !app.startup_shell_visible(),
            "new_live must use post-startup dock path"
        );

        // When: layout is planned
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 120, 40));
        let dock = plan.dock.unwrap_or_abort();

        // Then: composer dock matches freeze lead=2 inset (h1-stream-probe / run1-draft)
        assert_eq!(
            dock.shell.x,
            plan.shell.x.saturating_add(STARTUP_COMPOSER_INSET_X),
            "live dock shell.x must keep freeze horizontal inset"
        );
        assert_eq!(
            dock.shell.width,
            plan.shell
                .width
                .saturating_sub(STARTUP_COMPOSER_INSET_X.saturating_mul(2)),
            "live dock shell.width must keep freeze horizontal inset"
        );
        assert_eq!(
            dock.composer.x, dock.shell.x,
            "live composer band must share shell inset"
        );
        assert_eq!(
            dock.composer.width, dock.shell.width,
            "live composer band must share shell width"
        );
    }

    #[test]
    fn quiet_overlays_remain_centered_after_dock_merge() {
        // arrange
        // act
        // assert
        let theme = Theme::default();

        let mut palette = AppState::new_live(None, false, None);
        palette.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        let palette_plan = FrameLayoutPlan::for_app(&palette, Rect::new(0, 0, 100, 30));
        let palette_overlay = palette_plan.palette_overlay.unwrap_or_abort();
        let composer = palette_plan.composer.unwrap_or_abort();
        let overlay_area = Rect::new(
            palette_plan.content.x,
            palette_plan.content.y,
            palette_plan.content.width,
            composer.y.saturating_sub(palette_plan.content.y),
        );
        let expected = command_palette_overlay_area(
            overlay_area,
            &theme,
            theme.live_shell_layout(100, 30),
            palette_plan.session_contract,
            &palette,
        )
        .unwrap_or_abort();
        assert_eq!(palette_overlay, expected);
    }

    #[test]
    fn startup_palette_overlay_prefers_compact_modal_dimensions() {
        // arrange
        // act
        // assert
        let mut palette = AppState::new_startup(Vec::new(), None);
        palette.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));

        let plan = FrameLayoutPlan::for_app(&palette, Rect::new(0, 0, 100, 30));
        let overlay = plan.palette_overlay.unwrap_or_abort();

        assert_eq!(overlay.width, 60);
        assert_eq!(overlay.x, 20);
        assert_eq!(overlay.y, 4);
        assert_eq!(overlay.height, 19);
        assert!(overlay.bottom() <= plan.composer.unwrap_or_abort().y);
    }

    #[test]
    fn live_session_operator_sidebar_is_none_at_all_widths_including_wide() {
        // arrange
        let app = AppState::new_live(None, false, None);
        for (width, height) in [
            (120u16, 40u16),
            (100, 30),
            (80, 24),
            (79, 24),
            (80, 23),
            (60, 20),
            (121, 40),
            (140, 36),
            (160, 40),
        ] {
            // act
            let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
            // assert
            assert!(
                plan.operator_sidebar.is_none(),
                "operator_sidebar must be None at {width}x{height}; got {:?}",
                plan.operator_sidebar
            );
            assert!(
                plan.details_overlay.is_none(),
                "details_overlay must be None at {width}x{height}; got {:?}",
                plan.details_overlay
            );
            assert!(
                plan.wheel_hit_areas.inspector.is_none(),
                "inspector hit area must be None at {width}x{height}; got {:?}",
                plan.wheel_hit_areas.inspector
            );
            assert!(
                plan.wheel_hit_areas.overlay.is_none(),
                "overlay hit area must be None at {width}x{height}; got {:?}",
                plan.wheel_hit_areas.overlay
            );
        }
    }

    #[test]
    fn live_details_drawer_uses_secondary_overlay_not_primary_sidebar() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.live_details_drawer_open = true;
        for (width, height) in [(160u16, 40u16), (120, 40), (100, 30)] {
            // act
            let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
            // assert
            assert!(
                plan.operator_sidebar.is_none(),
                "details drawer must not allocate primary operator_sidebar at {width}x{height}"
            );
            assert!(
                plan.details_overlay.is_some(),
                "details drawer must expose secondary overlay at {width}x{height}"
            );
        }
    }

    #[test]
    fn live_session_transcript_and_composer_span_full_shell_width() {
        // arrange
        let app = AppState::new_live(None, false, None);
        for (width, height) in [
            (120u16, 40u16),
            (100, 30),
            (80, 24),
            (79, 24),
            (80, 23),
            (60, 20),
            (121, 40),
            (160, 40),
        ] {
            // act
            let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
            let transcript = plan
                .transcript
                .unwrap_or_else(|| panic!("transcript must exist at {width}x{height}"));
            let composer = plan
                .composer
                .unwrap_or_else(|| panic!("composer must exist at {width}x{height}"));
            // assert
            assert_eq!(
                transcript.width, plan.shell.width,
                "transcript must span full shell width at {width}x{height}; transcript={transcript:?} shell={:?}",
                plan.shell
            );
            let expected_inset = if width <= DENSE_SESSION_MAX_WIDTH {
                1
            } else {
                STARTUP_COMPOSER_INSET_X
            };
            assert_eq!(
                composer.x,
                plan.shell.x.saturating_add(expected_inset),
                "composer must use freeze-matched horizontal inset at {width}x{height}; composer={composer:?} shell={:?}",
                plan.shell
            );
            assert_eq!(
                composer.width,
                plan.shell.width.saturating_sub(expected_inset.saturating_mul(2)),
                "composer must use freeze-matched width at {width}x{height}; composer={composer:?} shell={:?}",
                plan.shell
            );
            assert_eq!(
                transcript.x, plan.shell.x,
                "transcript must share shell left edge at {width}x{height}"
            );
            assert!(
                transcript.y + transcript.height <= composer.y,
                "transcript must sit above composer at {width}x{height}; transcript={transcript:?} composer={composer:?}"
            );
        }
    }
}
