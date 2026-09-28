// allow: SIZE_OK — TUI layout math (frame plan + pane sizing)
use crate::UnwrapOrAbort;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{AppState, Focus};
use crate::overlay::OverlayKind;
use crate::theme::{LiveShellLayout, Theme};
use crate::theme_tokens::DESIGN_TOKENS;

#[path = "/tmp/tui-frame-layout/legacy/layout/overlays.rs"]
mod overlays;
#[path = "/tmp/tui-frame-layout/legacy/layout/permission.rs"]
mod permission;
#[path = "/tmp/tui-frame-layout/legacy/layout/surfaces.rs"]
mod surfaces;

use overlays::{
    command_palette_overlay_area, session_operator_overlay, slash_command_overlay_area,
};
pub(crate) use overlays::{completion_overlay_content_area, slash_command_overlay_content_area};
#[cfg(test)]
use overlays::{fork_selector_overlay_height, lifecycle_overlay_area};
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
    pub todo: Option<Rect>,
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
        plan.todo = session.todo;
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
            command_palette_overlay_area(plan.root, theme, shell_layout, session_contract, app)
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

        if !app.current_subagent_session_present()
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
    // Live permission and question prompts own the composer slot.
    let permission_prompt = app.active_permission_view().filter(|_| !app.replay_mode);
    let question_inset = STARTUP_COMPOSER_INSET_X.min(content_column.width / 2);
    let question_width = content_column.width.saturating_sub(question_inset * 2);
    let prompt_height = if let Some(permission) = permission_prompt.as_ref() {
        if permission.question_prompts.is_some() {
            question_dock_measure(
                app,
                question_width,
                Rect::new(0, 0, area.width, terminal_height),
                permission,
            )
            .status_height
        } else {
            permission_dock_measure(app, question_width, terminal_height, permission)
                .height
                .saturating_add(QUESTION_OUTER_FOOTER_ROWS)
                .min(content_column.height)
        }
    } else if subagent_footer_visible {
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
    let dock = if permission_prompt.is_some() {
        let status = Rect {
            x: main_chunks[1].x.saturating_add(question_inset),
            width: question_width,
            ..main_chunks[1]
        };
        control_dock_layout(
            status,
            Some(status),
            Rect::new(status.x, status.bottom(), status.width, 0),
            None,
        )
    } else if app.replay_mode {
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
            todo: None,
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
    let (body, todo) = todo_pane_split(app, body, terminal_height);
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
        todo,
        terminal_panel,
        operator_sidebar,
        operator_sidebar_compact_empty,
        operator_overlay,
        activity,
        inspector,
        dock: if permission_prompt.is_some() {
            dock
        } else {
            dock_with_horizontal_inset(dock, content_column)
        },
    }
}

fn todo_pane_split(app: &AppState, body: Rect, terminal_height: u16) -> (Rect, Option<Rect>) {
    if !app.todo_pane.visible
        || app.transcript_viewer().is_some()
        || body.width < 6
        || body.height < 6
    {
        return (body, None);
    }
    let height = app
        .todo_pane
        .desired_height(terminal_height)
        .min(body.height.saturating_sub(5));
    let todo = Rect::new(
        body.x.saturating_add(2),
        body.y.saturating_add(3),
        body.width.saturating_sub(4),
        height,
    );
    let reserve = height.saturating_add(2);
    (
        Rect::new(
            body.x,
            body.y.saturating_add(reserve),
            body.width,
            body.height.saturating_sub(reserve),
        ),
        Some(todo),
    )
}

pub(crate) fn todo_close_rect(area: Rect) -> Rect {
    Rect::new(area.right(), area.y.saturating_sub(1), 1, 1)
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
        status_composer_spacer_rows: if status_rows > 0 || app.activities.is_empty() {
            outer_spacer_rows
        } else {
            0
        },
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
    permission_dock_measure(app, width, terminal_height, permission).height
}

fn live_prompt_block_height(
    app: &AppState,
    area: Rect,
    _contract: SessionGeometryContract,
    _shell: LiveShellLayout,
    terminal_height: u16,
) -> u16 {
    if let Some(state) = app.rewind.state.as_ref() {
        return crate::rewind_view::rewind_overlay_height(&state.phase, terminal_height)
            .min(area.height);
    }
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


#[path = "/tmp/tui-frame-layout/legacy/layout_live_dock_test_fixtures.rs"]
mod live_dock_test_fixtures;
include!("/tmp/tui-frame-layout/oracle-cases.rs");
