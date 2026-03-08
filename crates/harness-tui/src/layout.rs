use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{AppState, Tab};
use crate::theme::{LiveShellLayout, Theme};

const MIN_COMPOSER_LINES: u16 = 1;
const MAX_COMPOSER_LINES: u16 = 5;

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
pub struct FrameLayoutPlan {
    pub root: Rect,
    pub shell: Rect,
    pub header: Rect,
    pub header_text: Rect,
    pub content: Rect,
    pub transcript: Option<Rect>,
    pub status: Option<Rect>,
    pub composer: Option<Rect>,
    pub footer: Rect,
    pub footer_text: Rect,
    pub details_overlay: Option<Rect>,
    pub palette_overlay: Option<Rect>,
    pub permission_modal: Option<Rect>,
    pub wheel_hit_areas: WheelHitAreas,
}

impl FrameLayoutPlan {
    pub fn for_app(app: &AppState, area: Rect) -> Self {
        let theme = app.theme();
        let shell_layout = theme.live_shell_layout(area.width, area.height);
        let root_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(theme.live_shell.heights.header),
                Constraint::Min(0),
                Constraint::Length(theme.live_shell.heights.footer),
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
            Rect::new(shell.x, footer.y, shell.width, footer.height)
        };

        let permission_modal = app
            .active_permission()
            .and_then(|_| permission_modal_area(area, theme, shell_layout));
        let palette_overlay = app
            .palette_visible
            .then(|| command_palette_overlay_area(content, theme, shell_layout))
            .flatten();

        let mut plan = Self {
            root: area,
            shell,
            header,
            header_text,
            content,
            transcript: None,
            status: None,
            composer: None,
            footer,
            footer_text,
            details_overlay: None,
            palette_overlay,
            permission_modal,
            wheel_hit_areas: WheelHitAreas::default(),
        };

        if !matches!(app.active_tab, Tab::Run | Tab::Details) {
            return plan;
        }

        if app.replay_mode {
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(theme.live_shell.heights.prompt_block()),
                ])
                .split(plan.shell);
            let pane_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(shell_layout.activity_drawer_width),
                    Constraint::Min(shell_layout.transcript_min_width),
                    Constraint::Length(shell_layout.inspector_drawer_width),
                ])
                .split(main_chunks[0]);

            plan.transcript = Some(pane_chunks[1]);
            plan.composer = Some(main_chunks[1]);
            plan.wheel_hit_areas = WheelHitAreas {
                transcript: Some(pane_chunks[1]),
                overlay: None,
                inspector: Some(pane_chunks[2]),
            };
            return plan;
        }

        let prompt_block_height = live_prompt_block_height(app, plan.shell, theme);
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(theme.live_shell.heights.status),
                Constraint::Length(prompt_block_height),
            ])
            .split(plan.shell);

        plan.transcript = Some(main_chunks[0]);
        plan.status = Some(main_chunks[1]);
        plan.composer = Some(main_chunks[2]);
        plan.details_overlay = app
            .details_drawer_open()
            .then(|| live_details_overlay_area(main_chunks[0], theme, shell_layout))
            .flatten();
        plan.wheel_hit_areas = WheelHitAreas {
            transcript: Some(main_chunks[0]),
            overlay: plan.details_overlay,
            inspector: plan
                .details_overlay
                .map(|overlay| details_drawer_areas(overlay)[1]),
        };

        plan
    }
}

pub(crate) fn details_drawer_areas(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);
    [chunks[0], chunks[1]]
}

pub(crate) fn composer_input_height(text: &str, width: u16) -> u16 {
    let inner_width = usize::from(width.saturating_sub(2).max(1));
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

    let clamped_lines = wrapped_lines.clamp(
        usize::from(MIN_COMPOSER_LINES),
        usize::from(MAX_COMPOSER_LINES),
    );
    u16::try_from(clamped_lines).unwrap_or(MAX_COMPOSER_LINES)
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
    SecondarySurfaceLayout {
        shell: area,
        body: inset_rect(
            area,
            theme.live_shell.rhythm.surface_margin_x,
            theme.live_shell.rhythm.surface_margin_y,
        ),
    }
}

pub(crate) fn split_secondary_surface(area: Rect, left_percent: u16, gap: u16) -> [Rect; 2] {
    if area.width == 0 || area.height == 0 {
        return [area, Rect::new(area.x, area.y, 0, area.height)];
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

pub(crate) fn centered_block_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}

fn live_prompt_block_height(app: &AppState, area: Rect, theme: &Theme) -> u16 {
    let status_height = theme.live_shell.heights.status;
    let max_block_height = area.height.saturating_sub(status_height);
    composer_input_height(&app.prompt_buffer, area.width)
        .saturating_add(2)
        .min(max_block_height)
}

fn centered_live_shell_area(area: Rect, shell: LiveShellLayout) -> Rect {
    let max_width = area
        .width
        .saturating_sub(shell.content_margin_x.saturating_mul(2));
    if max_width == 0 {
        return area;
    }

    let width = max_width.min(shell.centered_content_width).max(1);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    Rect::new(x, area.y, width, area.height)
}

fn live_details_overlay_area(area: Rect, theme: &Theme, shell: LiveShellLayout) -> Option<Rect> {
    let overlay_width = overlay_width(area, shell);
    let overlay_height = overlay_height(area, theme);
    if overlay_width == 0 || overlay_height == 0 {
        return None;
    }

    let overlay_x = area
        .x
        .saturating_add(area.width.saturating_sub(overlay_width).saturating_sub(1));
    let overlay_y = area.y.saturating_add(1);
    Some(Rect::new(
        overlay_x,
        overlay_y,
        overlay_width,
        overlay_height,
    ))
}

fn overlay_width(area: Rect, shell: LiveShellLayout) -> u16 {
    shell
        .activity_drawer_width
        .max(shell.inspector_drawer_width)
        .max(shell.inspector_drawer_width.saturating_mul(2))
        .max(shell.activity_drawer_width.saturating_add(6))
        .min(area.width.saturating_sub(2))
}

fn overlay_height(area: Rect, theme: &Theme) -> u16 {
    let _ = theme;
    area.height.saturating_sub(1)
}

fn permission_modal_area(area: Rect, theme: &Theme, shell: LiveShellLayout) -> Option<Rect> {
    let horizontal_margin = theme.live_shell.rhythm.modal_margin.saturating_mul(2);
    let vertical_margin = theme.live_shell.rhythm.modal_margin.saturating_mul(2);
    let popup_width = shell
        .permission_modal_width
        .min(area.width.saturating_sub(horizontal_margin));
    let popup_height = theme
        .live_shell
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

fn command_palette_overlay_area(area: Rect, theme: &Theme, shell: LiveShellLayout) -> Option<Rect> {
    let horizontal_margin = theme.live_shell.rhythm.modal_margin.saturating_mul(2);
    let vertical_margin = theme.live_shell.rhythm.modal_margin.saturating_mul(2);
    let popup_width = shell
        .centered_content_width
        .min(area.width.saturating_sub(horizontal_margin));
    let popup_height = theme
        .live_shell
        .heights
        .permission_modal
        .saturating_add(theme.live_shell.heights.prompt_input)
        .saturating_add(theme.live_shell.heights.status)
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
