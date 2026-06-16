use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
};

use crate::app::AppState;
use crate::overlay::OverlayKind;
use crate::theme::{LiveShellLayout, Theme};

mod overlays;
mod surfaces;

use overlays::{
    command_palette_overlay_area, session_operator_overlay, slash_command_overlay_area,
};
pub(crate) use overlays::{completion_overlay_content_area, slash_command_overlay_content_area};
#[cfg(test)]
use overlays::{fork_selector_overlay_height, lifecycle_overlay_area};
#[cfg(test)]
pub(crate) use surfaces::lifecycle_card_area;
pub(crate) use surfaces::{
    inset_rect, live_empty_state_area, pad_rect, permission_dock_layout,
    runtime_state_surface_area, runtime_state_surface_width, secondary_surface_layout,
    split_secondary_surface, startup_shell_area, ControlDockLayout, EdgeInsets,
};

const MIN_COMPOSER_LINES: u16 = 1;
const MAX_COMPOSER_LINES: u16 = 6;
const COMPOSER_VISIBLE_TEXT_CHROME: u16 = 5;
const LIVE_PROMPT_STANDARD_CHROME_ROWS: u16 = 4;
const LIVE_PROMPT_DENSE_CHROME_ROWS: u16 = 3;
const LIVE_PROMPT_MIN_HEIGHT: u16 = 5;
const DENSE_LIVE_PROMPT_MIN_HEIGHT: u16 = 4;
const LIVE_PERMISSION_PROMPT_MIN_HEIGHT: u16 = 11;
const LIVE_QUESTION_PROMPT_MIN_HEIGHT: u16 = 9;
const LIVE_DETAILS_MIN_TRANSCRIPT_WIDTH: u16 = 48;
const TERMINAL_PANEL_MIN_TRANSCRIPT_HEIGHT: u16 = 7;
const TERMINAL_PANEL_MIN_HEIGHT: u16 = 5;
const TERMINAL_PANEL_MAX_HEIGHT: u16 = 12;
pub(crate) const REVIEW_SURFACE_SPLIT_PERCENT: u16 = 34;
const DENSE_SESSION_MAX_WIDTH: u16 = 60;
const DENSE_SESSION_MAX_HEIGHT: u16 = 18;
const COMPACT_SESSION_MAX_WIDTH: u16 = 80;
const COMPACT_SESSION_MAX_HEIGHT: u16 = 24;
const DENSE_SESSION_PALETTE_MAX_WIDTH: u16 = 46;
const PERSISTENT_SIDEBAR_MIN_WIDTH: u16 = 121;
const STARTUP_COMPOSER_MAX_WIDTH: u16 = 75;
const STARTUP_LOGO_TO_COMPOSER_GAP: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionResponsiveMode {
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
    Persistent { width: u16 },
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
            shell_tokens.spacing.heights.footer.saturating_add(2)
        } else if app.startup_shell_visible() || app.replay_mode {
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
        } else {
            let footer_text_height = shell_tokens.spacing.heights.footer.min(footer.height);
            let footer_text_y = footer
                .y
                .saturating_add(footer.height.saturating_sub(footer_text_height) / 2);
            Rect::new(shell.x, footer_text_y, shell.width, footer_text_height)
        };

        let palette_overlay = matches!(
            app.overlay_stack().top(),
            Some(
                OverlayKind::CommandPalette
                    | OverlayKind::TogglesMenu
                    | OverlayKind::LineageBrowser
                    | OverlayKind::ForkSelector
            )
        )
        .then(|| command_palette_overlay_area(content, theme, shell_layout, session_contract, app))
        .flatten();

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
            palette_overlay,
            slash_overlay: None,
            wheel_hit_areas: WheelHitAreas::default(),
            session_contract,
        };

        let session = session_shell_layout(app, plan.shell, theme, shell_layout, session_contract);
        plan.live_anchor = session.live_anchor;
        plan.transcript = Some(session.transcript);
        plan.terminal_panel = session.terminal_panel;
        plan.operator_sidebar = session.operator_sidebar;
        plan.dock = Some(session.dock);
        plan.status = session.dock.status;
        plan.composer = Some(session.dock.composer);
        plan.disclosure = session.dock.disclosure;
        plan.details_overlay = session.operator_overlay;
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
            sidebar_mode: session_sidebar_mode(area, shell),
            palette_overlay_max_width: None,
            slash_overlay_max_width: None,
        },
        SessionResponsiveMode::Primary => SessionGeometryContract {
            header_mode: SessionHeaderMode::Hidden,
            footer_mode: SessionFooterMode::Standard,
            sidebar_mode: session_sidebar_mode(area, shell),
            palette_overlay_max_width: None,
            slash_overlay_max_width: None,
        },
    }
}

fn session_sidebar_mode(area: Rect, shell: LiveShellLayout) -> SessionSidebarMode {
    if area.width >= PERSISTENT_SIDEBAR_MIN_WIDTH {
        SessionSidebarMode::Persistent {
            width: shell.details_sidebar_width.max(1),
        }
    } else {
        SessionSidebarMode::Overlay {
            width: shell.details_sidebar_width.max(1),
        }
    }
}

fn session_responsive_mode(area: Rect, shell: LiveShellLayout) -> SessionResponsiveMode {
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
    app.review_surface().is_none()
        && matches!(contract.header_mode, SessionHeaderMode::Hidden)
        && !app.startup_shell_visible()
        && app.active_permission().is_none()
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
                SessionSidebarMode::Persistent { width } => {
                    let sidebar_width = width.min(area.width.saturating_sub(1)).max(1);
                    if area.width
                        >= min_transcript_width
                            .saturating_add(gap)
                            .saturating_add(sidebar_width)
                    {
                        let content_width =
                            area.width.saturating_sub(sidebar_width).saturating_sub(gap);
                        let sidebar_x = area.x.saturating_add(content_width).saturating_add(gap);
                        (
                            Rect::new(area.x, area.y, content_width, area.height),
                            Some(Rect::new(sidebar_x, area.y, sidebar_width, area.height)),
                        )
                    } else {
                        (area, None)
                    }
                }
                SessionSidebarMode::Overlay { .. } | SessionSidebarMode::Hidden => (area, None),
            }
        };
    let subagent_footer_visible = subagent_footer_visible(app);
    let prompt_height = if subagent_footer_visible {
        0
    } else if app.replay_mode {
        shell_tokens.spacing.heights.prompt_block()
    } else {
        live_prompt_block_height(app, content_column, contract, shell)
            .saturating_add(shell_tokens.spacing.heights.status)
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
        let disclosure_height =
            control_dock_disclosure_rows(app, contract).min(shell_area.height.saturating_sub(1));
        if disclosure_height == 0 {
            control_dock_layout(shell_area, None, shell_area, None)
        } else {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(disclosure_height)])
                .split(shell_area);
            let disclosure = (disclosure_height > 0).then_some(rows[1]);
            control_dock_layout(shell_area, None, rows[0], disclosure)
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
    } else if (app.replay_mode && !app.current_subagent_session_present())
        || app.details_drawer_open()
    {
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
        dock,
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
    let inner_width = usize::from(width.saturating_sub(COMPOSER_VISIBLE_TEXT_CHROME).max(1));
    let max_lines = MAX_COMPOSER_LINES;
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

fn wrapped_text_rows(text: &str, width: usize) -> u16 {
    if width == 0 {
        return 1;
    }

    let rows = text
        .split('\n')
        .map(|line| line.chars().count().max(1).div_ceil(width))
        .sum::<usize>()
        .max(1);
    u16::try_from(rows).unwrap_or(u16::MAX)
}

fn permission_prompt_block_height(
    app: &AppState,
    width: u16,
    permission: &crate::app::ActivePermissionView,
) -> u16 {
    let inner_width = usize::from(width.saturating_sub(6).max(1));

    if let Some(prompts) = permission.question_prompts.as_ref() {
        let mut rows = 5u16;
        for prompt in prompts {
            rows = rows
                .saturating_add(wrapped_text_rows(
                    &format!("[{}] {}", prompt.header, prompt.question),
                    inner_width,
                ))
                .saturating_add(u16::try_from(prompt.options.len()).unwrap_or(u16::MAX))
                .saturating_add(2);
        }

        rows = rows
            .saturating_add(wrapped_text_rows(
                &app.question_answer_preview(&permission.permission_id),
                inner_width,
            ))
            .saturating_add(u16::from(
                app.question_answer_error(&permission.permission_id)
                    .is_some(),
            ))
            .saturating_add(3);

        return rows.clamp(LIVE_QUESTION_PROMPT_MIN_HEIGHT, 16);
    }

    let draft_rows = wrapped_text_rows(app.prompt_buffer.as_str(), inner_width).min(2);
    LIVE_PERMISSION_PROMPT_MIN_HEIGHT
        .saturating_add(draft_rows.saturating_sub(1))
        .clamp(LIVE_PERMISSION_PROMPT_MIN_HEIGHT, 12)
}

fn live_prompt_block_height(
    app: &AppState,
    area: Rect,
    contract: SessionGeometryContract,
    _shell: LiveShellLayout,
) -> u16 {
    let max_block_height = area.height;
    let dense_footer = matches!(contract.footer_mode, SessionFooterMode::Minimal);
    let startup_shell = app.startup_shell_visible();
    let min_height = if startup_shell {
        LIVE_PROMPT_MIN_HEIGHT
    } else if dense_footer {
        DENSE_LIVE_PROMPT_MIN_HEIGHT.saturating_sub(1)
    } else {
        LIVE_PROMPT_MIN_HEIGHT.saturating_sub(1)
    };
    if !startup_shell {
        if let Some(permission) = app.active_permission_view() {
            return permission_prompt_block_height(app, area.width, &permission)
                .max(if permission.question_prompts.is_some() {
                    LIVE_QUESTION_PROMPT_MIN_HEIGHT
                } else {
                    LIVE_PERMISSION_PROMPT_MIN_HEIGHT
                })
                .min(max_block_height);
        }
    }

    let chrome_rows = if startup_shell {
        if dense_footer {
            LIVE_PROMPT_DENSE_CHROME_ROWS
        } else {
            LIVE_PROMPT_STANDARD_CHROME_ROWS
        }
    } else if dense_footer {
        LIVE_PROMPT_DENSE_CHROME_ROWS
    } else {
        LIVE_PROMPT_STANDARD_CHROME_ROWS
    };
    let disclosure_rows = u16::from(startup_shell);
    let natural_height = composer_input_height(&app.prompt_buffer, area.width)
        .saturating_add(chrome_rows)
        .saturating_add(disclosure_rows)
        .max(min_height);

    natural_height.min(max_block_height)
}

fn control_dock_disclosure_rows(app: &AppState, contract: SessionGeometryContract) -> u16 {
    if app.replay_mode || app.active_permission_view().is_some() || app.review_surface().is_some() {
        return 0;
    }

    if app.startup_shell_visible() && matches!(contract.footer_mode, SessionFooterMode::Minimal) {
        return 0;
    }

    let _ = contract;

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

fn centered_startup_dock_layout(
    dock: ControlDockLayout,
    area: Rect,
    transcript: Rect,
    shell: LiveShellLayout,
    theme: &Theme,
) -> ControlDockLayout {
    if dock.shell.width == 0 || dock.shell.height == 0 {
        return dock;
    }

    let width = dock
        .shell
        .width
        .min(shell.centered_content_width)
        .clamp(1, STARTUP_COMPOSER_MAX_WIDTH);
    if width == dock.shell.width {
        return dock;
    }

    let x = dock
        .shell
        .x
        .saturating_add(dock.shell.width.saturating_sub(width) / 2);
    let shell_height = dock.shell.height;
    let startup_card = startup_shell_area(transcript, theme);
    let logo_height = if startup_card.width < 40 { 1 } else { 3 };
    let logo_top_gap = startup_card.height.saturating_sub(logo_height) / 2;
    let target_y = startup_card
        .y
        .saturating_add(logo_top_gap)
        .saturating_add(logo_height)
        .saturating_add(STARTUP_LOGO_TO_COMPOSER_GAP);
    let max_y = area
        .y
        .saturating_add(area.height.saturating_sub(shell_height));
    let y = target_y.min(max_y);
    let shell_rect = Rect::new(x, y, width, shell_height);
    let shell_origin_y = dock.shell.y;
    let map_band = |band: Rect| {
        Rect::new(
            x,
            y.saturating_add(band.y.saturating_sub(shell_origin_y)),
            width.min(band.width),
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
        crate::theme::ShellGeometryTarget::Minimum => {
            max_width.min(shell.centered_content_width).max(1)
        }
        crate::theme::ShellGeometryTarget::Split => max_width.max(1),
        crate::theme::ShellGeometryTarget::Primary => max_width.max(1),
    };
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    Rect::new(x, area.y, width, area.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_secondary_surface_stacks_vertically_in_narrow_tall_windows() {
        let [top, bottom] = split_secondary_surface(Rect::new(0, 0, 60, 24), 40, 1);

        assert_eq!(top, Rect::new(0, 0, 60, 9));
        assert_eq!(bottom, Rect::new(0, 10, 60, 14));
    }

    #[test]
    fn split_secondary_surface_keeps_horizontal_layout_when_width_allows() {
        let [left, right] = split_secondary_surface(Rect::new(0, 0, 100, 24), 40, 1);

        assert_eq!(left, Rect::new(0, 0, 39, 24));
        assert_eq!(right, Rect::new(40, 0, 60, 24));
    }

    #[test]
    fn lifecycle_overlay_stays_centered_in_minimum_geometry() {
        let theme = Theme::default();
        let overlay = lifecycle_overlay_area(
            Rect::new(0, 1, 80, 22),
            &theme,
            theme.lifecycle_surface_layout(80, 22),
        )
        .expect("minimum lifecycle overlay");

        assert_eq!(overlay, Rect::new(2, 6, 76, 11));
    }

    #[test]
    fn replay_session_layout_never_reserves_live_anchor() {
        let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), Vec::new());
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 100, 30));

        assert!(plan.live_anchor.is_none());
    }

    #[test]
    fn quiet_overlays_remain_centered_after_dock_merge() {
        let theme = Theme::default();

        let mut palette = AppState::new_live(None, false, None);
        palette.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        let palette_plan = FrameLayoutPlan::for_app(&palette, Rect::new(0, 0, 100, 30));
        let palette_overlay = palette_plan.palette_overlay.expect("palette overlay");
        let expected = command_palette_overlay_area(
            palette_plan.content,
            &theme,
            theme.live_shell_layout(100, 30),
            palette_plan.session_contract,
            &palette,
        )
        .expect("centered palette overlay");
        assert_eq!(palette_overlay, expected);
    }

    #[test]
    fn startup_palette_overlay_prefers_compact_modal_dimensions() {
        let mut palette = AppState::new_startup(Vec::new(), None);
        palette.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));

        let plan = FrameLayoutPlan::for_app(&palette, Rect::new(0, 0, 100, 30));
        let overlay = plan.palette_overlay.expect("startup palette overlay");

        assert_eq!(overlay, Rect::new(20, 8, 60, 14));
    }

    #[test]
    fn fork_selector_overlay_uses_reference_large_dialog_geometry() {
        let mut app = AppState::new_live(None, false, None);
        app.open_fork_selector();

        assert!(app.fork_selector_visible);
        assert_eq!(app.overlay_stack().top(), Some(OverlayKind::ForkSelector));

        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 100, 40));
        let overlay = plan.palette_overlay.expect("fork selector overlay");

        assert_eq!(overlay, Rect::new(6, 10, 88, 7));
        assert_eq!(
            overlay.height,
            fork_selector_overlay_height(&app, plan.content.height)
        );
        assert_eq!(
            overlay.y,
            plan.content.y.saturating_add(plan.content.height / 4)
        );
    }
}
