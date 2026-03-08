use std::borrow::Cow;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::app::{
    ActivityEntry, ActivityStatus, AppState, Focus, RuntimeStateKind, Tab, ToolCallDisplayStatus,
};
use crate::keybindings::Action;
use crate::theme::{LiveShellLayout, Theme};

const MIN_COMPOSER_LINES: u16 = 1;
const MAX_COMPOSER_LINES: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelTarget {
    Transcript,
    Inspector,
}

#[derive(Debug, Clone, Copy, Default)]
struct WheelHitAreas {
    transcript: Option<Rect>,
    overlay: Option<Rect>,
    inspector: Option<Rect>,
}

pub fn render_app(frame: &mut Frame, app: &AppState) {
    let theme = Theme::default();
    let area = frame.area();
    let shell = theme.live_shell_layout(area.width, area.height);

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.chrome.canvas)),
        area,
    );

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(theme.live_shell.heights.header),
            Constraint::Min(0),
            Constraint::Length(theme.live_shell.heights.footer),
        ])
        .split(area);

    render_header(frame, app, main_chunks[0], &theme, shell);
    render_content(frame, app, main_chunks[1], &theme, shell);
    render_footer(frame, app, main_chunks[2], &theme, shell);

    if let Some((permission_id, summary)) = app.active_permission() {
        render_permission_modal(frame, &permission_id, &summary, &theme);
    }
}

fn render_header(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) {
    let run_id = app.run_id().unwrap_or("unknown");

    let header_text = if app.replay_mode {
        let session_path = app
            .session_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        format!(
            "Replay · {run_id} · {session_path} · {} ev",
            app.events.len()
        )
    } else {
        let harness_label = app
            .launch_mode_label()
            .map(|label| format!("Harness {label}"))
            .unwrap_or_else(|| "Harness".to_string());
        format!(
            "{harness_label} · {run_id} · {}/{} · {}",
            app.active_profile(),
            app.active_provider(),
            app.current_model_label()
        )
    };

    let style = Style::default()
        .fg(theme.chrome.header_fg)
        .bg(theme.chrome.header_bg);
    frame.render_widget(Block::default().style(style), area);
    frame.render_widget(
        Paragraph::new(header_text).style(style),
        live_shell_header_footer_area(app, area, shell),
    );
}

fn render_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) {
    if app.replay_mode {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(theme.live_shell.heights.tabs),
                Constraint::Min(0),
            ])
            .split(area);

        render_tabs(frame, app, chunks[0], theme);
        render_surface(frame, app, chunks[1], theme, shell);
    } else {
        render_surface(frame, app, area, theme, shell);
    }
}

/// Render the tab bar
fn render_tabs(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let titles: Vec<Line> = app
        .surface_registry()
        .iter()
        .enumerate()
        .map(|(i, surface)| {
            let style = if i == replay_tab_index(app.active_tab) {
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text.primary)
            };
            Line::from(Span::styled(surface.label, style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.chrome.border))
                .title("Tabs")
                .title_style(Style::default().fg(theme.chrome.title)),
        )
        .select(replay_tab_index(app.active_tab))
        .style(Style::default().fg(theme.chrome.border))
        .highlight_style(Style::default().fg(theme.text.accent));

    frame.render_widget(tabs, area);
}

fn replay_tab_index(active_tab: Tab) -> usize {
    match active_tab {
        Tab::Run | Tab::Details => 0,
        Tab::Events => 1,
        Tab::Diff => 2,
        Tab::Help => 3,
    }
}

fn render_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) {
    let area = if app.replay_mode {
        area
    } else {
        centered_live_shell_area(area, shell)
    };

    match app.active_tab {
        Tab::Run => {
            if app.replay_mode {
                render_run_workspace(frame, app, area, theme, shell)
            } else {
                render_live_session_surface(frame, app, area, theme, shell)
            }
        }
        Tab::Details => render_live_session_surface(frame, app, area, theme, shell),
        Tab::Events => render_events_tab(frame, app, area, theme),
        Tab::Diff => render_diff_tab(frame, app, area, theme),
        Tab::Help => render_help_tab(frame, app, area, theme),
    }
}

fn help_row(app: &AppState, action: Action, label: &str) -> String {
    format!("  {:<12} {label}", app.keymap.get_binding_str(action))
}

fn help_text(app: &AppState) -> String {
    let mut lines = vec![
        "Keyboard Shortcuts:".to_string(),
        String::new(),
        "Navigation:".to_string(),
        help_row(app, Action::MoveDown, "Move down in list"),
        help_row(app, Action::MoveUp, "Move up in list"),
        help_row(app, Action::FocusNext, "Cycle focus forward"),
        help_row(app, Action::FocusPrev, "Cycle focus backward"),
        help_row(app, Action::ToggleFollow, "Toggle follow mode"),
    ];

    if app.replay_mode {
        lines.extend([
            String::new(),
            "Replay surfaces:".to_string(),
            help_row(app, Action::TabRun, "Open conversation"),
            help_row(app, Action::TabEvents, "Open Events surface"),
            help_row(app, Action::TabDiff, "Open Diff surface"),
            help_row(app, Action::TabHelp, "Open Help surface"),
        ]);
    } else {
        lines.extend([
            String::new(),
            "Live surfaces:".to_string(),
            help_row(app, Action::TabRun, "Return to conversation"),
            help_row(app, Action::ToggleDetailsDrawer, "Toggle details drawer"),
            help_row(app, Action::TabEvents, "Open Events surface"),
            help_row(app, Action::TabDiff, "Open Diff surface"),
            help_row(app, Action::TabHelp, "Open Help surface"),
            String::new(),
            "Prompt (when focused):".to_string(),
            help_row(app, Action::SubmitPrompt, "Submit prompt"),
            help_row(app, Action::InsertNewline, "Insert newline"),
            help_row(app, Action::ClearPrompt, "Clear prompt"),
            help_row(app, Action::HistoryUp, "History up"),
            help_row(app, Action::HistoryDown, "History down"),
        ]);
    }

    lines.extend([
        String::new(),
        "Permission modal:".to_string(),
        help_row(app, Action::AllowPermission, "Allow permission"),
        help_row(app, Action::DenyPermission, "Deny permission"),
        help_row(app, Action::DismissModal, "Dismiss modal"),
        String::new(),
        "General:".to_string(),
        help_row(app, Action::Help, "Show this help"),
    ]);

    if app.replay_mode {
        lines.push(help_row(app, Action::Reload, "Reload session"));
    }

    lines.push(help_row(app, Action::Quit, "Quit"));
    lines.join("\n")
}

/// Render the Run workspace with 3-pane layout + prompt
fn render_run_workspace(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(theme.live_shell.heights.prompt_block()),
        ])
        .split(area);

    let pane_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(shell.activity_drawer_width),
            Constraint::Min(shell.transcript_min_width),
            Constraint::Length(shell.inspector_drawer_width),
        ])
        .split(main_chunks[0]);

    render_activity_pane(frame, app, pane_chunks[0], theme);
    render_transcript_pane(frame, app, pane_chunks[1], theme);
    render_inspector_pane(frame, app, pane_chunks[2], theme);
    render_prompt_pane(frame, app, main_chunks[1], theme);
}

fn render_live_session_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) {
    let prompt_block_height = live_prompt_block_height(app, area, theme);
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(theme.live_shell.heights.status),
            Constraint::Length(prompt_block_height),
        ])
        .split(area);

    render_transcript_pane(frame, app, main_chunks[0], theme);
    if app.details_drawer_open() {
        render_live_details_overlay(frame, app, main_chunks[0], theme, shell);
    }

    render_status_strip(frame, app, main_chunks[1], theme);
    render_prompt_pane(frame, app, main_chunks[2], theme);
}

fn render_details_drawer(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let drawer_chunks = details_drawer_areas(area);

    render_activity_pane(frame, app, drawer_chunks[0], theme);
    render_inspector_pane(frame, app, drawer_chunks[1], theme);
}

fn render_live_details_overlay(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) {
    let Some(overlay) = live_details_overlay_area(area, theme, shell) else {
        return;
    };

    frame.render_widget(Clear, overlay);
    render_details_drawer(frame, app, overlay, theme);
}

fn details_drawer_areas(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);
    [chunks[0], chunks[1]]
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

fn activity_surface_visible(app: &AppState) -> bool {
    (app.replay_mode && app.active_tab == Tab::Run) || app.details_drawer_open()
}

fn transcript_surface_focused(app: &AppState) -> bool {
    !app.replay_mode
        && app.active_tab == Tab::Run
        && app.focus == Focus::Details
        && !app.details_drawer_open()
}

fn live_secondary_surface_open(app: &AppState) -> bool {
    !app.replay_mode && matches!(app.active_tab, Tab::Events | Tab::Diff | Tab::Help)
}

fn panel_border_style(theme: &Theme, is_focused: bool) -> Style {
    let border = if is_focused {
        theme.chrome.focus_border
    } else {
        theme.chrome.border
    };
    Style::default().fg(border)
}

fn inset_area(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    let double_horizontal = horizontal.saturating_mul(2);
    let double_vertical = vertical.saturating_mul(2);
    Rect {
        x: area.x.saturating_add(horizontal),
        y: area.y.saturating_add(vertical),
        width: area.width.saturating_sub(double_horizontal),
        height: area.height.saturating_sub(double_vertical),
    }
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

fn live_shell_header_footer_area(app: &AppState, area: Rect, shell: LiveShellLayout) -> Rect {
    if app.replay_mode {
        area
    } else {
        centered_live_shell_area(area, shell)
    }
}

fn live_prompt_block_height(app: &AppState, area: Rect, theme: &Theme) -> u16 {
    let status_height = theme.live_shell.heights.status;
    let max_block_height = area.height.saturating_sub(status_height);
    composer_input_height(&app.prompt_buffer, area.width)
        .saturating_add(2)
        .min(max_block_height)
}

fn composer_input_height(text: &str, width: u16) -> u16 {
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

fn request_id_label(request_id: &str) -> Cow<'_, str> {
    if request_id.is_empty() {
        Cow::Borrowed("pending turn")
    } else {
        Cow::Borrowed(request_id)
    }
}

fn line_label(count: u16) -> &'static str {
    if count == 1 {
        "line"
    } else {
        "lines"
    }
}

fn runtime_state_color(kind: RuntimeStateKind, theme: &Theme) -> Color {
    match kind {
        RuntimeStateKind::Ready | RuntimeStateKind::Success => theme.status.success,
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming => theme.text.accent,
        RuntimeStateKind::Cancelled
        | RuntimeStateKind::PermissionBlocked
        | RuntimeStateKind::PermissionPending
        | RuntimeStateKind::Degraded => theme.status.warning,
        RuntimeStateKind::Failure | RuntimeStateKind::Disconnected => theme.status.error,
    }
}

fn render_status_strip(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let state = app.runtime_state();
    let base_style = Style::default()
        .fg(theme.chrome.footer_fg)
        .bg(theme.chrome.footer_bg);
    let status_line = Line::from(vec![
        Span::styled(
            state.kind.label(),
            Style::default()
                .fg(runtime_state_color(state.kind, theme))
                .bg(theme.chrome.footer_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", base_style),
        Span::styled(state.summary, base_style),
    ]);

    frame.render_widget(Paragraph::new(status_line).style(base_style), area);
}

/// Render the Activity pane (left)
fn render_activity_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);

    let border_style = panel_border_style(theme, is_focused);

    let title = format!(
        "Activity (j/k active{}{})",
        if app.follow_mode { ", follow" } else { "" },
        if is_focused { ", focused" } else { "" }
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.chrome.title));

    if app.activities.is_empty() {
        let empty = Paragraph::new("No activities yet")
            .block(block)
            .style(Style::default().fg(theme.text.primary));
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<Line> = app
        .activities
        .iter()
        .enumerate()
        .map(|(idx, activity)| {
            let is_selected = idx == app.selected_activity_index;
            let prefix = if is_selected { "> " } else { "  " };
            let status_icon = match activity.status {
                ActivityStatus::Streaming => theme.live_shell.glyphs.streaming,
                ActivityStatus::Done => theme.live_shell.glyphs.done,
                ActivityStatus::Error => theme.live_shell.glyphs.error,
            };
            let status_text = match activity.status {
                ActivityStatus::Streaming => "streaming…",
                ActivityStatus::Done => "done",
                ActivityStatus::Error => "error",
            };

            let style = if is_selected {
                Style::default()
                    .fg(theme.text.selected_fg)
                    .bg(theme.text.selected_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text.primary)
            };

            let model_display = if activity.model_id.is_empty() {
                "-"
            } else {
                &activity.model_id
            };
            let request_id = request_id_label(&activity.request_id);

            let content = format!(
                "{}{} {} {} {}",
                prefix, request_id, model_display, status_text, status_icon
            );
            Line::from(Span::styled(content, style))
        })
        .collect();

    let activity_list = Paragraph::new(Text::from(items))
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(activity_list, area);
}

/// Render the Transcript pane (center)
fn render_transcript_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = transcript_surface_focused(app);

    let border_style = panel_border_style(theme, is_focused);

    let title = if app.replay_mode {
        format!(
            "Transcript{}{}",
            if is_focused { " (focused)" } else { "" },
            if app.follow_mode { " (following)" } else { "" }
        )
    } else {
        format!(
            "Conversation{}{}",
            if is_focused { " (focused)" } else { "" },
            if app.follow_mode { " (following)" } else { "" }
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.chrome.title));

    let inner_area = inset_area(
        block.inner(area),
        theme.live_shell.rhythm.transcript_gutter_x,
        theme.live_shell.rhythm.transcript_gutter_y,
    );

    frame.render_widget(block, area);

    if live_empty_state_visible(app) {
        render_live_empty_state(frame, app, inner_area, theme);
        return;
    }

    let lines = build_transcript_lines(app, theme);
    let transcript_scroll = transcript_scroll_offset(app, &lines, inner_area);
    let content = Text::from(lines);

    frame.render_widget(
        Paragraph::new(content)
            .style(Style::default().fg(theme.text.primary))
            .scroll((transcript_scroll, 0))
            .wrap(Wrap { trim: false }),
        inner_area,
    );
}

pub fn hovered_wheel_target(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<WheelTarget> {
    let hit_areas = wheel_hit_areas(app, area);
    if hit_areas
        .inspector
        .is_some_and(|area| rect_contains(area, column, row))
    {
        return Some(WheelTarget::Inspector);
    }
    if hit_areas
        .overlay
        .is_some_and(|area| rect_contains(area, column, row))
    {
        return None;
    }
    hit_areas
        .transcript
        .filter(|area| rect_contains(*area, column, row))
        .map(|_| WheelTarget::Transcript)
}

fn wheel_hit_areas(app: &AppState, area: Rect) -> WheelHitAreas {
    if !matches!(app.active_tab, Tab::Run | Tab::Details) {
        return WheelHitAreas::default();
    }

    let theme = Theme::default();
    let shell = theme.live_shell_layout(area.width, area.height);
    let surface_area = if app.replay_mode {
        area
    } else {
        centered_live_shell_area(area, shell)
    };

    if app.replay_mode {
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(theme.live_shell.heights.prompt_block()),
            ])
            .split(surface_area);
        let pane_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(shell.activity_drawer_width),
                Constraint::Min(shell.transcript_min_width),
                Constraint::Length(shell.inspector_drawer_width),
            ])
            .split(main_chunks[0]);
        return WheelHitAreas {
            transcript: Some(pane_chunks[1]),
            overlay: None,
            inspector: Some(pane_chunks[2]),
        };
    }

    let prompt_block_height = live_prompt_block_height(app, surface_area, &theme);
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(theme.live_shell.heights.status),
            Constraint::Length(prompt_block_height),
        ])
        .split(surface_area);
    let overlay = app
        .details_drawer_open()
        .then(|| live_details_overlay_area(main_chunks[0], &theme, shell))
        .flatten();
    let inspector = overlay.map(|overlay| details_drawer_areas(overlay)[1]);

    WheelHitAreas {
        transcript: Some(main_chunks[0]),
        overlay,
        inspector,
    }
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn transcript_scroll_offset(app: &AppState, lines: &[Line<'static>], inner_area: Rect) -> u16 {
    let viewport_height = usize::from(inner_area.height);
    let viewport_width = usize::from(inner_area.width.max(1));
    if viewport_height == 0 {
        return 0;
    }

    let total_rows = transcript_visual_rows(lines, viewport_width);
    let max_scroll = total_rows.saturating_sub(viewport_height);
    if max_scroll == 0 {
        return 0;
    }

    if app.follow_mode {
        return u16::try_from(max_scroll).unwrap_or(u16::MAX);
    }

    let scroll_back = usize::from(app.transcript_scroll);
    let scroll = max_scroll.saturating_sub(scroll_back);
    u16::try_from(scroll).unwrap_or(u16::MAX)
}

fn transcript_visual_rows(lines: &[Line<'static>], viewport_width: usize) -> usize {
    lines
        .iter()
        .map(|line| {
            let width = line.width();
            if width == 0 {
                1
            } else {
                width.div_ceil(viewport_width)
            }
        })
        .sum()
}

fn live_empty_state_visible(app: &AppState) -> bool {
    !app.replay_mode && app.activities.is_empty() && app.transcript_pending_permissions().is_empty()
}

fn live_empty_state_mode_label<'a>(app: &AppState, theme: &'a Theme) -> Option<&'a str> {
    let mode = app.launch_mode_label()?.trim();
    if mode.eq_ignore_ascii_case("demo") {
        Some(theme.live_shell.empty_state.demo_mode_label)
    } else if mode.eq_ignore_ascii_case("mock") {
        Some(theme.live_shell.empty_state.mock_mode_label)
    } else {
        None
    }
}

fn render_live_empty_state(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut lines = Vec::new();
    if let Some(mode_label) = live_empty_state_mode_label(app, theme) {
        lines.push(Line::from(Span::styled(
            mode_label,
            Style::default()
                .fg(theme.status.warning)
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(Span::styled(
        theme.live_shell.empty_state.value_prop,
        Style::default()
            .fg(theme.text.primary)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    for example in theme.live_shell.empty_state.example_prompts {
        lines.push(Line::from(vec![
            Span::styled("› ", Style::default().fg(theme.text.accent)),
            Span::styled(example.prompt, Style::default().fg(theme.text.primary)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        [
            app.keymap.get_binding_label(Action::SubmitPrompt, "send"),
            app.keymap
                .get_binding_label(Action::InsertNewline, "newline"),
            format!(
                "{}/{} history",
                app.keymap.get_binding_str(Action::HistoryUp),
                app.keymap.get_binding_str(Action::HistoryDown)
            ),
        ]
        .join(" · "),
        Style::default().fg(theme.text.secondary),
    )));

    let height = u16::try_from(lines.len())
        .unwrap_or(area.height)
        .min(area.height);
    let width = area
        .width
        .min(theme.live_shell.empty_state.max_width)
        .max(1);
    let content_area = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(height)),
        width,
        height,
    );

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        content_area,
    );
}

fn build_transcript_lines(app: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for (index, activity) in app.activities.iter().enumerate() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        append_activity_lines(
            &mut lines,
            activity,
            index == app.selected_activity_index,
            theme,
        );
    }

    for (_permission_id, summary) in app.transcript_pending_permissions() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        append_pending_permission_lines(&mut lines, &summary, theme);
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Waiting for first turn…",
            Style::default().fg(theme.text.secondary),
        )));
    }

    lines
}

fn append_activity_lines(
    lines: &mut Vec<Line<'static>>,
    activity: &ActivityEntry,
    is_selected: bool,
    theme: &Theme,
) {
    let header_style = if is_selected {
        Style::default()
            .fg(theme.text.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text.primary)
    };
    let meta_style = Style::default().fg(theme.text.secondary);

    if let Some(user_msg) = &activity.user_message {
        lines.push(Line::from(vec![
            Span::styled("› ", Style::default().fg(theme.text.accent)),
            Span::styled("user", header_style),
            Span::styled(
                format!(" · {}", request_id_label(&activity.request_id)),
                meta_style,
            ),
        ]));
        append_text_block(lines, &user_msg.text, theme.text.primary, "  ");
    }

    let (assistant_icon, assistant_color, assistant_status) = match activity.status {
        ActivityStatus::Streaming => (
            theme.live_shell.glyphs.streaming,
            theme.text.accent,
            "streaming…",
        ),
        ActivityStatus::Done => (theme.live_shell.glyphs.done, theme.status.success, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };
    let mut assistant_meta = vec![assistant_status.to_string()];
    if !activity.provider_id.is_empty() || !activity.model_id.is_empty() {
        assistant_meta.push(format!(
            "{}/{}",
            if activity.provider_id.is_empty() {
                "-"
            } else {
                activity.provider_id.as_str()
            },
            if activity.model_id.is_empty() {
                "-"
            } else {
                activity.model_id.as_str()
            }
        ));
    }
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", assistant_icon),
            Style::default().fg(assistant_color),
        ),
        Span::styled("assistant", header_style),
        Span::styled(format!(" · {}", assistant_meta.join(" · ")), meta_style),
    ]));

    if !activity.transcript_text.is_empty() {
        append_text_block(lines, &activity.transcript_text, theme.text.primary, "  ");
    } else if activity.status == ActivityStatus::Streaming {
        lines.push(Line::from(Span::styled(
            "  Waiting for response…",
            Style::default().fg(theme.text.secondary),
        )));
    }

    if let Some(error) = &activity.error_message {
        lines.push(Line::from(vec![
            Span::styled("  ↳ ", Style::default().fg(theme.status.error)),
            Span::styled(error.clone(), Style::default().fg(theme.status.error)),
        ]));
    }

    for tool_call in &activity.tool_calls {
        append_tool_call_lines(lines, tool_call, theme);
    }
}

fn append_tool_call_lines(
    lines: &mut Vec<Line<'static>>,
    tool_call: &crate::app::ToolCallEntry,
    theme: &Theme,
) {
    let (status_icon, status_color) = match tool_call.status {
        ToolCallDisplayStatus::PendingPermission => (
            theme.live_shell.glyphs.pending_permission,
            theme.status.warning,
        ),
        ToolCallDisplayStatus::Queued => (theme.live_shell.glyphs.queued, theme.text.secondary),
        ToolCallDisplayStatus::Running => (theme.live_shell.glyphs.running, theme.text.accent),
        ToolCallDisplayStatus::Succeeded => {
            (theme.live_shell.glyphs.succeeded, theme.status.success)
        }
        ToolCallDisplayStatus::Failed => (theme.live_shell.glyphs.failed, theme.status.error),
    };

    let mut row = vec![
        Span::styled("  ", Style::default().fg(theme.text.secondary)),
        Span::styled(
            format!("{} ", status_icon),
            Style::default().fg(status_color),
        ),
        Span::styled("tool ", Style::default().fg(theme.text.secondary)),
        Span::styled(
            tool_call.tool_id.clone(),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", tool_call.status),
            Style::default().fg(theme.text.secondary),
        ),
    ];

    if let Some(summary) = tool_call.transcript_summary() {
        row.push(Span::styled(
            format!(" · {}", summary),
            Style::default().fg(theme.text.secondary),
        ));
    }

    lines.push(Line::from(row));
}

fn append_pending_permission_lines(lines: &mut Vec<Line<'static>>, summary: &str, theme: &Theme) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", theme.live_shell.glyphs.pending_permission),
            Style::default().fg(theme.status.warning),
        ),
        Span::styled(
            "permission",
            Style::default()
                .fg(theme.status.warning)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · requested", Style::default().fg(theme.text.secondary)),
    ]));
    append_text_block(lines, summary, theme.text.primary, "  ");
}

fn append_text_block<'a>(
    lines: &mut Vec<Line<'a>>,
    text: &str,
    color: ratatui::style::Color,
    prefix: &str,
) {
    for line in text.lines() {
        let body = if line.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}{line}")
        };
        lines.push(Line::from(Span::styled(body, Style::default().fg(color))));
    }

    if text.is_empty() {
        lines.push(Line::from(Span::styled(
            prefix.to_string(),
            Style::default().fg(color),
        )));
    }
}

/// Render the Inspector pane (right)
fn render_inspector_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details && activity_surface_visible(app);
    let runtime_state = app.runtime_state();

    let border_style = panel_border_style(theme, is_focused);

    let title = format!("Inspector{}", if is_focused { " (focused)" } else { "" });

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.chrome.title));

    let content = if let Some(activity) = app.activities.get(app.selected_activity_index) {
        let mut lines = Vec::new();

        if let Some(detail) = runtime_state.detail.clone() {
            lines.push(Line::from(vec![
                Span::styled("Runtime: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    runtime_state.kind.label(),
                    Style::default()
                        .fg(runtime_state_color(runtime_state.kind, theme))
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                detail,
                Style::default().fg(theme.text.secondary),
            )));
            lines.push(Line::from(""));
        }

        append_section_header(&mut lines, "Activity metadata:", theme.text.primary);
        append_labeled_value(
            &mut lines,
            "  Request ID: ",
            request_id_label(&activity.request_id),
            theme.text.primary,
        );
        append_labeled_value(
            &mut lines,
            "  Provider: ",
            activity.provider_id.clone(),
            theme.text.primary,
        );
        append_labeled_value(
            &mut lines,
            "  Model: ",
            activity.model_id.clone(),
            theme.text.primary,
        );
        append_labeled_value(
            &mut lines,
            "  Status: ",
            activity.status.to_string(),
            match activity.status {
                crate::app::ActivityStatus::Error => theme.status.error,
                crate::app::ActivityStatus::Done => theme.status.success,
                _ => theme.text.primary,
            },
        );
        append_labeled_value(
            &mut lines,
            "  Sequences: ",
            format!("{}-{}", activity.first_seq, activity.last_seq),
            theme.text.primary,
        );

        if let Some(req_data) = &activity.request_data {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Request metadata:", theme.text.primary);
            append_detail_payload(
                &mut lines,
                "  Prompt summary:",
                &req_data.prompt_summary,
                theme.text.primary,
            );
            append_labeled_value(
                &mut lines,
                "  Request digest: ",
                req_data.request_digest.clone(),
                theme.text.secondary,
            );
            match serde_json::to_string_pretty(req_data) {
                Ok(json) => {
                    append_detail_payload(&mut lines, "  Raw request:", &json, theme.text.primary)
                }
                Err(_) => append_labeled_value(
                    &mut lines,
                    "  Raw request: ",
                    "[error serializing]",
                    theme.status.error,
                ),
            }
        }

        if !activity.permissions.is_empty() {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Permission context:", theme.text.primary);
            append_permission_details(&mut lines, &activity.permissions, theme, "  ");
        }

        if !activity.tool_calls.is_empty() {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Tool calls:", theme.text.primary);
            append_tool_call_details(&mut lines, &activity.tool_calls, theme);
        }

        if let Some(error) = &activity.error_message {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Runtime errors:", theme.status.error);
            append_detail_payload(&mut lines, "  Raw error:", error, theme.status.error);
        }

        Text::from(lines)
    } else if let Some(detail) = runtime_state.detail.clone() {
        Text::from(vec![
            Line::from(vec![
                Span::styled("Runtime: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    runtime_state.kind.label(),
                    Style::default()
                        .fg(runtime_state_color(runtime_state.kind, theme))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                detail,
                Style::default().fg(theme.text.secondary),
            )),
        ])
    } else {
        Text::from("No activity selected")
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(theme.text.primary))
        .scroll((app.details_scroll, 0))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn append_section_header<'a>(lines: &mut Vec<Line<'a>>, title: &str, color: Color) {
    lines.push(Line::from(Span::styled(
        title.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )));
}

fn append_labeled_value<'a>(
    lines: &mut Vec<Line<'a>>,
    label: &str,
    value: impl Into<String>,
    color: Color,
) {
    lines.push(Line::from(vec![
        Span::styled(
            label.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.into(), Style::default().fg(color)),
    ]));
}

fn permission_decision_label(decision: harness_core::event::PermissionDecision) -> &'static str {
    match decision {
        harness_core::event::PermissionDecision::Allow => "allow",
        harness_core::event::PermissionDecision::Deny => "deny",
    }
}

fn permission_status_style(
    permission: &crate::app::PermissionEntry,
    theme: &Theme,
) -> (&'static str, &'static str, Color) {
    match permission.resolved_decision {
        Some(harness_core::event::PermissionDecision::Allow) => (
            "allowed",
            theme.live_shell.glyphs.succeeded,
            theme.status.success,
        ),
        Some(harness_core::event::PermissionDecision::Deny) => {
            ("denied", theme.live_shell.glyphs.failed, theme.status.error)
        }
        None => (
            "pending",
            theme.live_shell.glyphs.pending_permission,
            theme.status.warning,
        ),
    }
}

fn append_permission_details<'a>(
    lines: &mut Vec<Line<'a>>,
    permissions: &'a [crate::app::PermissionEntry],
    theme: &Theme,
    indent: &str,
) {
    for permission in permissions {
        let (status_label, status_icon, status_color) = permission_status_style(permission, theme);

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{indent}{status_icon} "),
                Style::default().fg(status_color),
            ),
            Span::styled(
                permission.permission_id.clone(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" · {} · {}", permission.kind, status_label),
                Style::default().fg(theme.text.secondary),
            ),
        ]));
        append_labeled_value(
            lines,
            &format!("{indent}Summary: "),
            permission.summary.clone(),
            theme.text.primary,
        );
        if let Some(tool_call_id) = &permission.tool_call_id {
            append_labeled_value(
                lines,
                &format!("{indent}Tool call: "),
                tool_call_id.clone(),
                theme.text.secondary,
            );
        }
        append_labeled_value(
            lines,
            &format!("{indent}Request digest: "),
            permission.request_digest.clone(),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            &format!("{indent}Timeout: "),
            format!("{} ms", permission.timeout_ms),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            &format!("{indent}Default: "),
            permission_decision_label(permission.default_decision),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            &format!("{indent}Sequences: "),
            format!("{}-{}", permission.first_seq, permission.last_seq),
            theme.text.secondary,
        );
        if let Some(decision) = permission.resolved_decision {
            append_labeled_value(
                lines,
                &format!("{indent}Resolved: "),
                permission_decision_label(decision),
                status_color,
            );
        }
        if let Some(reason) = &permission.resolution_reason {
            append_detail_payload(
                lines,
                &format!("{indent}Reason:"),
                reason,
                theme.text.primary,
            );
        }
    }
}

fn append_tool_call_details<'a>(
    lines: &mut Vec<Line<'a>>,
    tool_calls: &'a [crate::app::ToolCallEntry],
    theme: &Theme,
) {
    for tool_call in tool_calls {
        let (status_icon, status_color) = match tool_call.status {
            ToolCallDisplayStatus::PendingPermission => (
                theme.live_shell.glyphs.pending_permission,
                theme.status.warning,
            ),
            ToolCallDisplayStatus::Queued => (theme.live_shell.glyphs.queued, theme.text.secondary),
            ToolCallDisplayStatus::Running => (theme.live_shell.glyphs.running, theme.text.accent),
            ToolCallDisplayStatus::Succeeded => {
                (theme.live_shell.glyphs.succeeded, theme.status.success)
            }
            ToolCallDisplayStatus::Failed => (theme.live_shell.glyphs.failed, theme.status.error),
        };

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", status_icon),
                Style::default().fg(status_color),
            ),
            Span::styled(
                tool_call.tool_id.clone(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" · {}", tool_call.status),
                Style::default().fg(theme.text.secondary),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Call ID: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                tool_call.tool_call_id.clone(),
                Style::default().fg(theme.text.secondary),
            ),
        ]));
        append_labeled_value(
            lines,
            "  Sequences: ",
            format!("{}-{}", tool_call.first_seq, tool_call.last_seq),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            "  Args digest: ",
            tool_call.args_digest.clone(),
            theme.text.secondary,
        );
        append_detail_payload(
            lines,
            "  Raw args:",
            &tool_call.args_summary,
            theme.text.primary,
        );

        if !tool_call.permissions.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Permission context:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            append_permission_details(lines, &tool_call.permissions, theme, "    ");
        }

        if let Some(output) = &tool_call.output_summary {
            if let Some(output_digest) = &tool_call.output_digest {
                append_labeled_value(
                    lines,
                    "  Output digest: ",
                    output_digest.clone(),
                    theme.text.secondary,
                );
            }
            let (label, color) = if tool_call.status == ToolCallDisplayStatus::Failed {
                ("  Raw error:", theme.status.error)
            } else {
                ("  Raw output:", theme.text.primary)
            };
            append_detail_payload(lines, label, output, color);
        }
    }
}

fn append_detail_payload<'a>(lines: &mut Vec<Line<'a>>, label: &str, payload: &str, color: Color) {
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    append_text_block(lines, &format_detail_payload(payload), color, "    ");
}

fn format_detail_payload(payload: &str) -> String {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| trimmed.to_string()),
        Err(_) => trimmed.to_string(),
    }
}

/// Render the Prompt input pane (bottom)
fn render_prompt_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Prompt;
    let runtime_state = app.runtime_state();
    let composer_disabled = runtime_state.composer_disabled;

    let border_style = panel_border_style(theme, is_focused);

    let char_count = app.prompt_buffer.chars().count();
    let composer_lines = composer_input_height(&app.prompt_buffer, area.width);
    let title = if composer_disabled {
        format!(
            "Composer (disabled · {}){}",
            runtime_state.kind.label(),
            if is_focused { " (focused)" } else { "" }
        )
    } else {
        format!(
            "Composer ({} {} · {} chars){}",
            composer_lines,
            line_label(composer_lines),
            char_count,
            if is_focused { " (focused)" } else { "" }
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.chrome.title));

    let mut text = app.prompt_buffer.clone();
    if is_focused && !composer_disabled {
        let cursor_byte_pos = app
            .prompt_buffer
            .char_indices()
            .nth(app.prompt_cursor)
            .map(|(i, _)| i)
            .unwrap_or(app.prompt_buffer.len());
        text.insert(cursor_byte_pos, '█');
    }

    let (text, style) = if text.is_empty() {
        (
            runtime_state.composer_hint,
            Style::default().fg(theme.text.secondary),
        )
    } else if composer_disabled {
        // Show draft with secondary styling when disabled
        (text, Style::default().fg(theme.text.secondary))
    } else {
        (text, Style::default().fg(theme.text.primary))
    };

    let paragraph = Paragraph::new(text)
        .block(block)
        .style(style)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Render the Events tab (legacy 2-pane layout)
fn render_events_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_event_list(frame, app, chunks[0], theme);
    render_event_details(frame, app, chunks[1], theme);
}

/// Render the event list (left pane of Events tab)
fn render_event_list(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List;

    let border_style = panel_border_style(theme, is_focused);

    let follow_indicator = if app.follow_mode { ", follow" } else { "" };
    let title = format!("Events (j/k active{})", follow_indicator);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.chrome.title));

    if app.events.is_empty() {
        let empty = Paragraph::new("No events")
            .block(block)
            .style(Style::default().fg(theme.text.primary));
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<Line> = app
        .events
        .iter()
        .enumerate()
        .skip(app.events_trimmed_count)
        .map(|(idx, event)| {
            let display_idx = idx + 1;
            let is_selected = idx == app.selected_event_index;
            let prefix = if is_selected { ">" } else { " " };

            let style = if is_selected {
                Style::default()
                    .fg(theme.text.selected_fg)
                    .bg(theme.text.selected_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text.primary)
            };

            let event_type = format!("{:?}", event.payload)
                .split(':')
                .next()
                .unwrap_or("Unknown")
                .to_string();

            let content = format!("{:>5} {} {}", display_idx, prefix, event_type);
            Line::from(Span::styled(content, style))
        })
        .collect();

    let list = Paragraph::new(Text::from(items))
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(list, area);
}

/// Render event details (right pane of Events tab)
fn render_event_details(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details;

    let border_style = panel_border_style(theme, is_focused);

    let title = "Event details";

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.chrome.title));

    let content = if let Some(event) = app.selected_event() {
        match serde_json::to_string_pretty(event) {
            Ok(json) => json,
            Err(_) => "Error serializing event".to_string(),
        }
    } else {
        "No event selected".to_string()
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render the Diff tab
fn render_diff_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_event_list(frame, app, chunks[0], theme);

    // Right pane: diff viewer
    let is_focused = app.focus == Focus::Details;
    let border_style = panel_border_style(theme, is_focused);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title("Diff")
        .title_style(Style::default().fg(theme.chrome.title));

    let content = if let Some(path) = &app.session_path {
        // Try to find and display the diff artifact
        if let Some(event) = app.selected_event() {
            if let Some(diff_content) = load_diff_for_event(path, event) {
                diff_content
            } else {
                format!("diff artifact missing:\n{}", path.display())
            }
        } else {
            "Select an edit event to view diff".to_string()
        }
    } else {
        "No session loaded".to_string()
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, chunks[1]);
}

/// Load diff content for an event
fn load_diff_for_event(
    session_path: &std::path::Path,
    event: &harness_core::event::EventEnvelopeV1,
) -> Option<String> {
    use harness_core::event::EventV1;

    if let EventV1::EditApplied(data) = &event.payload {
        if let Some(diff_rel_path) = &data.diff_rel_path {
            let diff_path = session_path.join(diff_rel_path);
            return std::fs::read_to_string(&diff_path).ok();
        }
    }
    None
}

fn render_help_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Help")
        .title_style(Style::default().fg(theme.chrome.title));

    let paragraph = Paragraph::new(help_text(app))
        .block(block)
        .style(Style::default().fg(theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_footer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    shell: LiveShellLayout,
) {
    let state = app.runtime_state();
    let separator = " ".repeat(theme.live_shell.rhythm.status_separator as usize);
    let hint_text = if app.replay_mode {
        [
            app.keymap.get_binding_label(Action::FocusNext, "nav"),
            app.keymap.get_binding_label(Action::TabRun, "convo"),
            app.keymap.get_binding_label(Action::TabEvents, "events"),
            app.keymap.get_binding_label(Action::TabDiff, "diff"),
            app.keymap.get_binding_label(Action::TabHelp, "help"),
            app.keymap.get_binding_label(Action::Reload, "reload"),
            app.keymap.get_binding_label(Action::Quit, "quit"),
        ]
        .join(&separator)
    } else if live_secondary_surface_open(app) {
        [
            app.keymap.get_binding_label(Action::TabRun, "convo"),
            app.keymap
                .get_binding_label(Action::ToggleDetailsDrawer, "details"),
            app.keymap.get_binding_label(Action::TabEvents, "events"),
            app.keymap.get_binding_label(Action::TabDiff, "diff"),
            app.keymap.get_binding_label(Action::TabHelp, "help"),
            app.keymap.get_binding_label(Action::FocusNext, "focus"),
            app.keymap.get_binding_label(Action::Quit, "quit"),
        ]
        .join(&separator)
    } else {
        let details_hint = if app.details_drawer_open() {
            app.keymap
                .get_binding_label(Action::ToggleDetailsDrawer, "close")
        } else {
            app.keymap
                .get_binding_label(Action::ToggleDetailsDrawer, "details")
        };
        if state.composer_disabled {
            [
                details_hint,
                app.keymap.get_binding_label(Action::TabEvents, "events"),
                app.keymap.get_binding_label(Action::TabDiff, "diff"),
                app.keymap.get_binding_label(Action::TabHelp, "help"),
                app.keymap.get_binding_label(Action::Quit, "quit"),
            ]
            .join(&separator)
        } else {
            [
                app.keymap.get_binding_label(Action::SubmitPrompt, "send"),
                app.keymap.get_binding_label(Action::InsertNewline, "nl"),
                details_hint,
                app.keymap.get_binding_label(Action::TabEvents, "events"),
                app.keymap.get_binding_label(Action::TabDiff, "diff"),
                app.keymap.get_binding_label(Action::TabHelp, "help"),
                app.keymap.get_binding_label(Action::Quit, "quit"),
            ]
            .join(&separator)
        }
    };

    let style = Style::default()
        .fg(theme.chrome.footer_fg)
        .bg(theme.chrome.footer_bg);
    frame.render_widget(Block::default().style(style), area);
    frame.render_widget(
        Paragraph::new(hint_text).style(style),
        live_shell_header_footer_area(app, area, shell),
    );
}

/// Render the permission modal
fn render_permission_modal(frame: &mut Frame, _permission_id: &str, summary: &str, theme: &Theme) {
    let area = frame.area();
    let shell = theme.live_shell_layout(area.width, area.height);
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

    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_rect = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the modal
    frame.render_widget(Clear, popup_rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.chrome.modal_border))
        .title("Permission Requested")
        .title_style(Style::default().fg(theme.chrome.title));

    let content = format!(
        "{}\n\n[a]llow  [d]eny  [esc]dismiss",
        summary
            .chars()
            .take(popup_width as usize - 4)
            .collect::<String>()
    );

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(
            Style::default()
                .fg(theme.text.primary)
                .bg(theme.chrome.modal_bg),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, popup_rect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LaunchMetadata;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
        PermissionResolvedEvent, ProviderRequestStartedEvent, ToolCallFinishedEvent,
        ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
        SCHEMA_VERSION,
    };

    fn render_debug(app: &AppState, width: u16, height: u16) -> String {
        use ratatui::{backend::TestBackend, Terminal};

        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| render_app(frame, app))
            .expect("draw frame");
        format!("{:?}", terminal.backend().buffer())
    }

    fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_{seq:04}"),
            seq,
            run_id: "run_ui_tests".to_string(),
            mono_ms: seq,
            ts: Some("2026-02-03T12:00:00Z".to_string()),
            actor: EventActor::new(ActorKind::System, Some("ui-tests".to_string())),
            correlation_id: Some(request_id.to_string()),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn rect_center(area: Rect) -> (u16, u16) {
        (
            area.x.saturating_add(area.width.saturating_sub(1) / 2),
            area.y.saturating_add(area.height.saturating_sub(1) / 2),
        )
    }

    fn transcript_debug(app: &AppState) -> String {
        build_transcript_lines(app, &Theme::default())
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn theme_provides_default_colors() {
        let theme = Theme::default();
        assert!(matches!(
            theme.chrome.canvas,
            ratatui::style::Color::Rgb(_, _, _)
        ));
    }

    #[test]
    fn live_header_uses_actual_launch_metadata() {
        let mut app = AppState::new_live(None, false, None);
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Harness Demo"));
        assert!(debug.contains("deep/proxy · gpt-5.4"));
        assert!(!debug.contains("default/default"));
    }

    #[test]
    fn footer_hints_follow_keymap_overrides() {
        let mut app = AppState::new_live(None, false, None);
        app.apply_keybindings(
            [
                ("submit_prompt".to_string(), "ctrl+s".to_string()),
                ("insert_newline".to_string(), "ctrl+j".to_string()),
                ("toggle_details_drawer".to_string(), "d".to_string()),
                ("tab_help".to_string(), "g".to_string()),
                ("quit".to_string(), "x".to_string()),
            ]
            .into_iter()
            .collect(),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Ctrl+s send"));
        assert!(debug.contains("Ctrl+j nl"));
        assert!(debug.contains("d details"));
        assert!(debug.contains("g help"));
        assert!(debug.contains("x quit"));
        assert!(!debug.contains("q quit"));
    }

    #[test]
    fn live_empty_state_labels_explicit_demo_or_mock_mode() {
        let mut demo = AppState::new_live(None, false, None);
        demo.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
        );

        let demo_debug = render_debug(&demo, 100, 24);
        assert!(demo_debug.contains("Demo mode · mock provider"));

        let mut mock = AppState::new_live(None, false, None);
        mock.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
        );

        let mock_debug = render_debug(&mock, 100, 24);
        assert!(mock_debug.contains("Mock mode · mock provider"));
    }

    #[test]
    fn help_surface_lists_active_bindings() {
        let mut app = AppState::new_live(None, false, None);
        app.active_tab = Tab::Help;
        app.apply_keybindings(
            [
                ("tab_events".to_string(), "e".to_string()),
                ("tab_diff".to_string(), "f".to_string()),
                ("tab_help".to_string(), "g".to_string()),
                ("toggle_follow".to_string(), "z".to_string()),
                ("submit_prompt".to_string(), "ctrl+s".to_string()),
                ("insert_newline".to_string(), "ctrl+j".to_string()),
            ]
            .into_iter()
            .collect(),
        );

        let debug = render_debug(&app, 100, 30);
        assert!(debug.contains("z"));
        assert!(debug.contains("Toggle follow mode"));
        assert!(debug.contains("e"));
        assert!(debug.contains("Open Events surface"));
        assert!(debug.contains("f"));
        assert!(debug.contains("Open Diff surface"));
        assert!(debug.contains("g"));
        assert!(debug.contains("Open Help surface"));
        assert!(debug.contains("Ctrl+s"));
        assert!(debug.contains("Submit prompt"));
        assert!(debug.contains("Ctrl+j"));
        assert!(debug.contains("Insert newline"));
        assert!(!debug.contains("4 / h"));
    }

    #[test]
    fn wheel_target_hits_transcript_when_hovered() {
        let app = AppState::new_live(None, false, None);
        let area = Rect::new(0, 0, 140, 40);
        let hit_areas = wheel_hit_areas(&app, area);
        let transcript = hit_areas.transcript.expect("transcript area");
        let (column, row) = rect_center(transcript);

        assert_eq!(
            hovered_wheel_target(&app, area, column, row),
            Some(WheelTarget::Transcript)
        );
    }

    #[test]
    fn wheel_target_hits_inspector_inside_live_overlay() {
        let mut app = AppState::new_live(None, false, None);
        app.active_tab = Tab::Details;

        let area = Rect::new(0, 0, 140, 40);
        let hit_areas = wheel_hit_areas(&app, area);
        let inspector = hit_areas.inspector.expect("inspector area");
        let (column, row) = rect_center(inspector);

        assert_eq!(
            hovered_wheel_target(&app, area, column, row),
            Some(WheelTarget::Inspector)
        );
    }

    #[test]
    fn wheel_target_excludes_activity_portion_of_live_overlay() {
        let mut app = AppState::new_live(None, false, None);
        app.active_tab = Tab::Details;

        let area = Rect::new(0, 0, 140, 40);
        let hit_areas = wheel_hit_areas(&app, area);
        let overlay = hit_areas.overlay.expect("overlay area");

        assert_eq!(
            hovered_wheel_target(
                &app,
                area,
                overlay.x.saturating_add(1),
                overlay.y.saturating_add(1),
            ),
            None
        );
    }

    #[test]
    fn inspector_shows_tool_call_detail_for_selected_activity() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_detail",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_tool_detail".to_string(),
                text: "Read the file".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_detail",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_detail".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read src/lib.rs and report the first 20 lines".to_string(),
                request_digest: "digest-tool-detail-request".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_detail",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_tool_detail".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs","start_line":1,"limit":20}"#.to_string(),
                args_digest: "digest-tool-detail-args".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_tool_detail",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_tool_detail".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            5,
            "req_tool_detail",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_tool_detail".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(
                    r#"{"lines":["use std::path::PathBuf;","use std::sync::Arc;"]}"#.to_string(),
                ),
                output_digest: Some("digest-tool-detail-output".to_string()),
            }),
        ));

        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('i')));

        let request_debug = render_debug(&app, 140, 40);
        assert!(request_debug.contains("Request metadata:"));
        assert!(request_debug.contains("digest-tool-detail-request"));

        let inspector_screens = (0..32)
            .map(|scroll| {
                app.details_scroll = scroll;
                render_debug(&app, 140, 40)
            })
            .collect::<Vec<_>>();
        let tool_debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("Raw args:"))
            .expect("tool detail section should be reachable via scroll");
        assert!(tool_debug.contains("Tool calls:"));
        assert!(tool_debug.contains("Raw args:"));
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("src/lib.rs") || debug.contains("\"path\":")),
            "expected inspector to expose tool args path"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("digest-tool-detail-args")),
            "expected inspector to expose args digest"
        );

        let output_debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("Raw output:"))
            .expect("tool output section should be reachable via scroll");
        assert!(output_debug.contains("Raw output:"));
        assert!(output_debug.contains("digest-tool-detail-output"));
        assert!(
            inspector_screens.iter().any(
                |debug| debug.contains("\"lines\"") || debug.contains("use std::path::PathBuf")
            ),
            "expected inspector to expose raw output payload"
        );
    }

    #[test]
    fn permission_detail_remains_available_outside_modal() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_permission_detail",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_permission_detail".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Apply the edit".to_string(),
                request_digest: "digest-permission-detail-request".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_permission_detail",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_permission_detail".to_string(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: r#"{"path":"demo.txt","ops":[{"Replace":{"line":2}}]}"#.to_string(),
                args_digest: "digest-permission-detail-args".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_permission_detail",
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_permission_detail".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tc_permission_detail".to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-permission-detail".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        ));

        app.handle_key(key(KeyCode::Esc));
        app.ingest_event(envelope(
            4,
            "req_permission_detail",
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_permission_detail".to_string(),
                decision: harness_core::event::PermissionDecision::Deny,
                reason: Some("operator denied in test".to_string()),
            }),
        ));
        assert_eq!(app.activities.len(), 1);
        assert_eq!(app.activities[0].tool_calls.len(), 1);
        assert_eq!(app.activities[0].tool_calls[0].permissions.len(), 1);
        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('i')));

        let inspector_screens = (0..40)
            .map(|scroll| {
                app.details_scroll = scroll;
                render_debug(&app, 140, 40)
            })
            .collect::<Vec<_>>();
        let debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("Permission context:"))
            .expect("permission detail section should be reachable via scroll");
        assert!(debug.contains("Permission context:"));
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("perm_permission_detail")),
            "expected inspector to expose permission id"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("digest-permission-detail")),
            "expected inspector to expose permission digest"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("Default: deny")),
            "expected inspector to expose default decision"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("Resolved: deny")),
            "expected inspector to expose resolved decision"
        );

        let reason_debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("operator denied in test"))
            .expect("permission reason should be reachable via scroll");
        assert!(reason_debug.contains("operator denied in test"));
    }

    #[test]
    fn transcript_tool_rows_keep_status_but_not_raw_json_dump() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_compact",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_tool_compact".to_string(),
                text: "Read the file".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_compact",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_compact".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-tool-compact".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_compact",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_compact".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#.to_string(),
                args_digest: "digest-tool-compact-args".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_tool_compact",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_compact".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            5,
            "req_tool_compact",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_compact".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("12 lines read".to_string()),
                output_digest: Some("digest-tool-compact-output".to_string()),
            }),
        ));

        let transcript = transcript_debug(&app);
        assert!(transcript.contains("tool fs.read · succeeded · 12 lines read"));
        assert!(!transcript.contains(r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#));
        assert!(!transcript.contains("args {"));
        assert_eq!(
            format_detail_payload(r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#),
            "{\n  \"limit\": 20,\n  \"path\": \"src/lib.rs\",\n  \"start_line\": 42\n}"
        );
    }

    #[test]
    fn failed_tool_rows_still_surface_error_summary() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_error",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_error".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Run the command".to_string(),
                request_digest: "digest-tool-error".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_error",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_error".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false","cwd":"/tmp/demo"}"#.to_string(),
                args_digest: "digest-tool-error-args".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_error",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_error".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_tool_error",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_error".to_string(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: permission denied".to_string()),
                output_digest: None,
            }),
        ));

        let transcript = transcript_debug(&app);
        assert!(
            transcript.contains("tool shell.run · failed · exit code: 1 stderr: permission denied")
        );
        assert!(!transcript.contains(r#"{"cmd":"false","cwd":"/tmp/demo"}"#));
        assert!(!transcript.contains("args {"));
    }
}
