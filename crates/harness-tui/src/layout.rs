use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::AppState;
use crate::overlay::OverlayKind;
use crate::theme::{LifecycleCardTokens, LiveShellLayout, ShellGeometryTarget, Theme};

#[cfg(test)]
use crate::theme::LifecycleSurfaceLayout;

const MIN_COMPOSER_LINES: u16 = 1;
const MAX_COMPOSER_LINES: u16 = 6;
const COMPOSER_VISIBLE_TEXT_CHROME: u16 = 5;
const LIVE_PROMPT_STANDARD_CHROME_ROWS: u16 = 4;
const LIVE_PROMPT_DENSE_CHROME_ROWS: u16 = 3;
const LIVE_PROMPT_MIN_HEIGHT: u16 = 5;
const DENSE_LIVE_PROMPT_MIN_HEIGHT: u16 = 4;
const LIVE_EMPTY_STATE_MIN_HEIGHT: u16 = 9;
const LIVE_DETAILS_MIN_TRANSCRIPT_WIDTH: u16 = 48;
#[allow(dead_code)]
const LIVE_DETAILS_MIN_TRANSCRIPT_HEIGHT: u16 = 8;
const SECONDARY_STACK_MAX_WIDTH: u16 = 72;
const SECONDARY_STACK_MIN_HEIGHT: u16 = 18;
pub(crate) const REVIEW_SURFACE_SPLIT_PERCENT: u16 = 34;
const DENSE_SESSION_MAX_WIDTH: u16 = 60;
const DENSE_SESSION_MAX_HEIGHT: u16 = 18;
const COMPACT_SESSION_MAX_WIDTH: u16 = 80;
const COMPACT_SESSION_MAX_HEIGHT: u16 = 24;
const DENSE_SESSION_PALETTE_MAX_WIDTH: u16 = 46;
const DENSE_SESSION_SLASH_MAX_WIDTH: u16 = 32;
const PERSISTENT_SIDEBAR_MIN_WIDTH: u16 = 121;
const STARTUP_COMPOSER_MAX_WIDTH: u16 = 75;
const RUNTIME_STATE_SURFACE_HORIZONTAL_INSET: u16 = 6;
const RUNTIME_STATE_SURFACE_MAX_WIDTH: u16 = 68;
const RUNTIME_STATE_SURFACE_MIN_WIDTH: u16 = 32;
const RUNTIME_STATE_SURFACE_MIN_HEIGHT: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionResponsiveMode {
    Dense,
    CompactMinimum,
    StandardMinimum,
    Split,
    Primary,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionHeaderMode {
    Hidden,
    Standard,
    Compact,
    Minimal,
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
    pub overlay: Option<Rect>,
    pub inspector: Option<Rect>,
}

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
pub(crate) struct SessionShellLayout {
    pub live_anchor: Option<Rect>,
    pub transcript: Rect,
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
    pub operator_sidebar: Option<Rect>,
    pub dock: Option<ControlDockLayout>,
    pub status: Option<Rect>,
    pub composer: Option<Rect>,
    pub disclosure: Option<Rect>,
    pub footer: Rect,
    pub footer_text: Rect,
    pub details_overlay: Option<Rect>,
    pub palette_overlay: Option<Rect>,
    pub permission_modal: Option<Rect>,
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
        let footer_height = if app.replay_mode {
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
        let shell = if app.replay_mode {
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

        let permission_modal = app
            .active_permission()
            .and_then(|_| permission_modal_area(area, theme, shell_layout));
        let palette_overlay =
            matches!(app.overlay_stack().top(), Some(OverlayKind::CommandPalette))
                .then(|| {
                    command_palette_overlay_area(
                        content,
                        theme,
                        shell_layout,
                        session_contract,
                        app,
                    )
                })
                .flatten();

        let mut plan = Self {
            root: area,
            shell,
            header,
            header_text,
            content,
            live_anchor: None,
            transcript: None,
            operator_sidebar: None,
            dock: None,
            status: None,
            composer: None,
            disclosure: None,
            footer,
            footer_text,
            details_overlay: None,
            palette_overlay,
            permission_modal,
            wheel_hit_areas: WheelHitAreas::default(),
            session_contract,
        };

        let session = session_shell_layout(app, plan.shell, theme, shell_layout, session_contract);
        plan.live_anchor = session.live_anchor;
        plan.transcript = Some(session.transcript);
        plan.operator_sidebar = session.operator_sidebar;
        plan.dock = Some(session.dock);
        plan.status = session.dock.status;
        plan.composer = Some(session.dock.composer);
        plan.disclosure = session.dock.disclosure;
        plan.details_overlay = session.operator_overlay;
        plan.wheel_hit_areas = WheelHitAreas {
            transcript: Some(session.transcript),
            overlay: (!session.operator_sidebar_compact_empty)
                .then_some(session.operator_sidebar.or(session.operator_overlay))
                .flatten(),
            inspector: session.inspector,
        };

        if app.replay_mode {
            plan.wheel_hit_areas = WheelHitAreas {
                transcript: Some(session.transcript),
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
            slash_overlay_max_width: Some(DENSE_SESSION_SLASH_MAX_WIDTH),
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

fn operator_sidebar_has_rail_sections(app: &AppState) -> bool {
    !app.operator_sidebar_pending_permission_lines().is_empty()
        || !app.operator_sidebar_todo_lines().is_empty()
        || !app.operator_sidebar_modified_files().is_empty()
}

fn hide_session_header(app: &AppState, contract: SessionGeometryContract) -> bool {
    !app.replay_mode
        && app.review_surface().is_none()
        && matches!(contract.header_mode, SessionHeaderMode::Hidden)
        && !app.startup_shell_visible()
        && app.active_permission().is_none()
}

pub(crate) fn session_shell_layout(
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
    contract: SessionGeometryContract,
) -> SessionShellLayout {
    let shell_tokens = theme.token_families().live_shell;
    let gap = shell_tokens.spacing.rhythm.surface_gap.max(1);
    let min_transcript_width = shell
        .transcript_min_width
        .max(LIVE_DETAILS_MIN_TRANSCRIPT_WIDTH);
    let (content_column, operator_sidebar) = if app.startup_shell_visible() {
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
    let prompt_height = if app.replay_mode {
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
        let status_height = shell_tokens.spacing.heights.status.min(shell_area.height);
        if status_height == 0 || shell_area.height <= status_height {
            control_dock_layout(shell_area, None, shell_area, None)
        } else {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(status_height)])
                .split(shell_area);
            control_dock_layout(shell_area, Some(rows[1]), rows[0], None)
        }
    };
    if app.startup_shell_visible() {
        return SessionShellLayout {
            live_anchor: None,
            transcript: body,
            operator_sidebar: None,
            operator_sidebar_compact_empty: false,
            operator_overlay: None,
            activity: None,
            inspector: None,
            dock: centered_startup_dock_layout(dock, area, body, shell, theme),
        };
    }

    let operator_sidebar_compact_empty =
        operator_sidebar.is_some() && !operator_sidebar_has_rail_sections(app);
    let transcript = body;
    let operator_overlay = if operator_sidebar.is_some() {
        None
    } else if app.replay_mode || app.details_drawer_open() {
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
        operator_sidebar,
        operator_sidebar_compact_empty,
        operator_overlay,
        activity,
        inspector,
        dock,
    }
}

pub(crate) fn composer_input_height(text: &str, width: u16) -> u16 {
    let inner_width = usize::from(width.saturating_sub(COMPOSER_VISIBLE_TEXT_CHROME).max(1));
    let max_lines = MAX_COMPOSER_LINES;
    let wrapped_lines = if text.is_empty() {
        1
    } else {
        text.split('\n')
            .map(|line| {
                let char_count = line.chars().count();
                char_count.max(1).div_ceil(inner_width)
            })
            .sum()
    };

    let clamped_lines =
        wrapped_lines.clamp(usize::from(MIN_COMPOSER_LINES), usize::from(max_lines));
    u16::try_from(clamped_lines).unwrap_or(max_lines)
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

fn live_prompt_block_height(
    app: &AppState,
    area: Rect,
    contract: SessionGeometryContract,
    _shell: LiveShellLayout,
) -> u16 {
    let max_block_height = area.height;
    let dense_footer = matches!(contract.footer_mode, SessionFooterMode::Minimal);
    let min_height = if dense_footer {
        DENSE_LIVE_PROMPT_MIN_HEIGHT
    } else {
        LIVE_PROMPT_MIN_HEIGHT
    };
    let min_height = if app.startup_shell_visible() {
        min_height.max(LIVE_PROMPT_MIN_HEIGHT)
    } else {
        min_height
    };
    let chrome_rows = if dense_footer {
        LIVE_PROMPT_DENSE_CHROME_ROWS
    } else {
        LIVE_PROMPT_STANDARD_CHROME_ROWS
    };
    let disclosure_rows = u16::from(app.startup_shell_visible());
    let natural_height = composer_input_height(&app.prompt_buffer, area.width)
        .saturating_add(chrome_rows)
        .saturating_add(disclosure_rows)
        .max(min_height);

    natural_height.min(max_block_height)
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
    let target_y = startup_card
        .y
        .saturating_add(startup_card.height)
        .saturating_sub(1);
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

#[allow(dead_code)]
fn live_details_drawer_layout(
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) -> (Rect, Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return (area, None);
    }

    let gap = theme
        .token_families()
        .live_shell
        .spacing
        .rhythm
        .surface_gap
        .max(1);
    let sidebar_width = shell
        .details_sidebar_width
        .min(area.width.saturating_sub(1));
    let min_transcript_width = shell
        .transcript_min_width
        .max(LIVE_DETAILS_MIN_TRANSCRIPT_WIDTH);

    if area.width
        >= min_transcript_width
            .saturating_add(gap)
            .saturating_add(sidebar_width)
    {
        let transcript_width = area.width.saturating_sub(sidebar_width).saturating_sub(gap);
        let sidebar_x = area.x.saturating_add(transcript_width).saturating_add(gap);
        return (
            Rect::new(area.x, area.y, transcript_width, area.height),
            Some(Rect::new(sidebar_x, area.y, sidebar_width, area.height)),
        );
    }

    let min_transcript_height = if area.height <= 12 {
        4
    } else {
        LIVE_DETAILS_MIN_TRANSCRIPT_HEIGHT
    };
    let max_details_height = area
        .height
        .saturating_sub(gap.saturating_add(min_transcript_height));
    if max_details_height == 0 {
        return (area, None);
    }

    let details_height = if area.height <= 12 {
        max_details_height.clamp(5, 7)
    } else {
        area.height
            .saturating_div(3)
            .clamp(8, 10)
            .min(max_details_height)
    };
    let transcript_height = area
        .height
        .saturating_sub(details_height)
        .saturating_sub(gap);
    let details_y = area.y.saturating_add(transcript_height).saturating_add(gap);

    (
        Rect::new(area.x, area.y, area.width, transcript_height),
        Some(Rect::new(area.x, details_y, area.width, details_height)),
    )
}

fn session_operator_overlay(body: Rect, contract: SessionGeometryContract) -> Option<Rect> {
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

fn permission_modal_area(area: Rect, theme: &Theme, shell: LiveShellLayout) -> Option<Rect> {
    let shell_tokens = theme.token_families().live_shell;
    let horizontal_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let vertical_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let popup_width = shell
        .permission_modal_width
        .min(area.width.saturating_sub(horizontal_margin));
    let popup_height = shell_tokens
        .spacing
        .heights
        .permission_modal
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

fn command_palette_overlay_area(
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
    contract: SessionGeometryContract,
    app: &AppState,
) -> Option<Rect> {
    let shell_tokens = theme.token_families().live_shell;
    let horizontal_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let vertical_margin = shell_tokens.spacing.rhythm.modal_margin.saturating_mul(2);
    let popup_width = command_palette_overlay_width(shell, app)
        .min(contract.palette_overlay_max_width.unwrap_or(u16::MAX))
        .min(area.width.saturating_sub(horizontal_margin));
    let popup_height =
        command_palette_overlay_height(app).min(area.height.saturating_sub(vertical_margin));

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
    if app.session_history_visible {
        shell.centered_content_width
    } else {
        shell.permission_modal_width
    }
}

fn command_palette_overlay_height(app: &AppState) -> u16 {
    const OVERLAY_FRAME_ROWS: u16 = 2;
    const MAX_LIST_ROWS: usize = 6;

    let body_rows = if app.session_history_visible {
        let history_rows = app.session_history_filtered.len().clamp(1, MAX_LIST_ROWS);
        let history_rows = u16::try_from(history_rows).unwrap_or(u16::MAX);
        u16::from(app.continue_disabled_banner.is_some())
            .saturating_add(1)
            .saturating_add(1)
            .saturating_add(history_rows)
    } else {
        let command_rows = app
            .palette_filtered
            .iter()
            .fold((0usize, None), |(rows, last_section), command| {
                let section = crate::Action::palette_command_section(command.as_str());
                let header_rows = usize::from(section != last_section);
                (rows.saturating_add(header_rows).saturating_add(1), section)
            })
            .0
            .clamp(1, MAX_LIST_ROWS);
        let command_rows = u16::try_from(command_rows).unwrap_or(u16::MAX);
        1u16.saturating_add(command_rows)
    };

    body_rows.saturating_add(OVERLAY_FRAME_ROWS)
}

#[cfg(test)]
fn lifecycle_overlay_area(
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
        let theme = Theme::default();

        let mut palette = AppState::new_startup(Vec::new(), None);
        palette.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));

        let plan = FrameLayoutPlan::for_app(&palette, Rect::new(0, 0, 100, 30));
        let overlay = plan.palette_overlay.expect("startup palette overlay");
        let shell = theme.live_shell_layout(100, 30);

        assert_eq!(overlay.width, shell.permission_modal_width);
        assert_eq!(overlay.height, 9);
    }
}
