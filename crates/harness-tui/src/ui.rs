use std::borrow::Cow;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

use harness_core::event::EventV1;

use crate::app::{ActivityEntry, ActivityStatus, AppState, Focus, Tab, ToolCallDisplayStatus};
use crate::theme::{LiveShellLayout, Theme};

const MIN_COMPOSER_LINES: u16 = 2;
const MAX_COMPOSER_LINES: u16 = 6;

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

    render_header(frame, app, main_chunks[0], &theme);
    render_content(frame, app, main_chunks[1], &theme, shell);
    render_footer(frame, app, main_chunks[2], &theme);

    if let Some((permission_id, summary)) = app.active_permission() {
        render_permission_modal(frame, &permission_id, &summary, &theme);
    }
}

fn render_header(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let run_id = app.run_id().unwrap_or("unknown");
    let profile = "default"; // TODO: get from config
    let provider = "default";
    let model = app
        .activities
        .back()
        .map(|a| a.model_id.as_str())
        .unwrap_or("-");

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
        format!("Harness · {run_id} · {profile}/{provider} · {model}")
    };

    let header = Paragraph::new(header_text).style(
        Style::default()
            .fg(theme.chrome.header_fg)
            .bg(theme.chrome.header_bg),
    );
    frame.render_widget(header, area);
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
        Tab::Help => render_help_tab(frame, area, theme),
    }
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
    let prompt_block_height = live_prompt_block_height(app, area);
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
    let drawer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);

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
    let overlay_width = overlay_width(area, shell);
    let overlay_height = overlay_height(area, theme);
    if overlay_width == 0 || overlay_height == 0 {
        return;
    }

    let overlay_x = area
        .x
        .saturating_add(area.width.saturating_sub(overlay_width).saturating_sub(1));
    let overlay_y = area.y.saturating_add(1);
    let overlay = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay);
    render_details_drawer(frame, app, overlay, theme);
}

fn overlay_width(area: Rect, shell: LiveShellLayout) -> u16 {
    shell
        .activity_drawer_width
        .max(shell.inspector_drawer_width)
        .max(shell.activity_drawer_width.saturating_add(6))
        .min(area.width.saturating_sub(2))
}

fn overlay_height(area: Rect, theme: &Theme) -> u16 {
    area.height
        .saturating_sub(2)
        .min(theme.live_shell.heights.prompt_block().saturating_add(7))
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

fn live_prompt_block_height(app: &AppState, area: Rect) -> u16 {
    let status_height = 1_u16;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompactStateKind {
    Ready,
    Sending,
    Streaming,
    Cancelled,
    Error,
    Degraded,
    Disconnected,
}

impl CompactStateKind {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Sending => "Sending",
            Self::Streaming => "Streaming",
            Self::Cancelled => "Cancelled",
            Self::Error => "Error",
            Self::Degraded => "Degraded",
            Self::Disconnected => "Disconnected",
        }
    }

    fn color(self, theme: &Theme) -> Color {
        match self {
            Self::Ready => theme.status.success,
            Self::Sending | Self::Streaming => theme.text.accent,
            Self::Cancelled | Self::Degraded => theme.status.warning,
            Self::Error | Self::Disconnected => theme.status.error,
        }
    }
}

#[derive(Debug, Clone)]
struct CompactState {
    kind: CompactStateKind,
    summary: String,
    detail: Option<String>,
    composer_disabled: bool,
}

fn compact_state(app: &AppState) -> CompactState {
    if let Some(state) = status_banner_state(app) {
        return state;
    }

    if let Some(event) = app.events.last() {
        if let EventV1::TaskCancelled(cancelled) = &event.payload {
            let summary = if cancelled.reason.trim().is_empty() {
                "current work cancelled".to_string()
            } else {
                format!("current work cancelled · {}", cancelled.reason)
            };
            return CompactState {
                kind: CompactStateKind::Cancelled,
                summary,
                detail: Some(cancelled.reason.clone()).filter(|reason| !reason.trim().is_empty()),
                composer_disabled: false,
            };
        }
    }

    if let Some(activity) = app.activities.back() {
        let summary = activity_status_summary(app, activity);
        return match activity.status {
            ActivityStatus::Streaming if activity.transcript_text.is_empty() => CompactState {
                kind: CompactStateKind::Sending,
                summary: format!("{summary} · waiting for first tokens"),
                detail: None,
                composer_disabled: false,
            },
            ActivityStatus::Streaming => CompactState {
                kind: CompactStateKind::Streaming,
                summary: format!("{summary} · receiving output"),
                detail: None,
                composer_disabled: false,
            },
            ActivityStatus::Done => CompactState {
                kind: CompactStateKind::Ready,
                summary,
                detail: None,
                composer_disabled: false,
            },
            ActivityStatus::Error => CompactState {
                kind: CompactStateKind::Error,
                summary: format!("{summary} · inspect transcript"),
                detail: activity.error_message.clone(),
                composer_disabled: false,
            },
        };
    }

    CompactState {
        kind: CompactStateKind::Ready,
        summary: if app.replay_mode {
            format!("{} events loaded", app.events.len())
        } else {
            "prompt ready".to_string()
        },
        detail: None,
        composer_disabled: false,
    }
}

fn status_banner_state(app: &AppState) -> Option<CompactState> {
    let banner = app.status_banner.as_deref()?;
    let lower = banner.to_ascii_lowercase();
    let composer_disabled = app.prompt_bootstrap_disabled();

    if lower.contains("disconnected") {
        return Some(CompactState {
            kind: CompactStateKind::Disconnected,
            summary: if composer_disabled {
                "live event stream unavailable · reopen TUI".to_string()
            } else {
                "live event stream lost · reopen TUI or inspect the saved session".to_string()
            },
            detail: Some(banner.to_string()),
            composer_disabled,
        });
    }

    if lower.contains("lagged") || lower.contains("replaying") {
        return Some(CompactState {
            kind: CompactStateKind::Degraded,
            summary: if composer_disabled {
                format!("{banner} · composer locked")
            } else {
                banner.to_string()
            },
            detail: Some(banner.to_string()),
            composer_disabled,
        });
    }

    if lower.contains("failed") || lower.contains("error") || lower.contains("no session path") {
        return Some(CompactState {
            kind: CompactStateKind::Error,
            summary: if app.replay_mode {
                "reload failed · inspect details".to_string()
            } else {
                "runtime error · inspect details".to_string()
            },
            detail: Some(banner.to_string()),
            composer_disabled,
        });
    }

    Some(CompactState {
        kind: CompactStateKind::Degraded,
        summary: banner.to_string(),
        detail: Some(banner.to_string()),
        composer_disabled,
    })
}

fn activity_status_summary(app: &AppState, activity: &ActivityEntry) -> String {
    let turn_count = app.activities.len();
    let provider = if activity.provider_id.is_empty() {
        "-"
    } else {
        activity.provider_id.as_str()
    };
    let model = if activity.model_id.is_empty() {
        "-"
    } else {
        activity.model_id.as_str()
    };

    [
        format!("turn {turn_count}/{turn_count}"),
        request_id_label(&activity.request_id).into_owned(),
        format!("{provider}/{model}"),
    ]
    .join(" · ")
}

fn render_status_strip(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let state = compact_state(app);
    let base_style = Style::default()
        .fg(theme.chrome.footer_fg)
        .bg(theme.chrome.footer_bg);
    let status_line = Line::from(vec![
        Span::styled(
            state.kind.label(),
            Style::default()
                .fg(state.kind.color(theme))
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

    let mut lines = build_transcript_lines(app, theme);
    let visible_lines = inner_area.height as usize;
    if visible_lines > 0 && lines.len() > visible_lines {
        let start = lines.len().saturating_sub(visible_lines);
        lines = lines.split_off(start);
    }

    let content = Text::from(lines);

    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::default().fg(theme.text.primary))
            .wrap(Wrap { trim: false }),
        inner_area,
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
    if is_selected {
        assistant_meta.push("current".to_string());
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

    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", status_icon),
            Style::default().fg(status_color),
        ),
        Span::styled(
            format!("tool {}", tool_call.tool_id),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", tool_call.status),
            Style::default().fg(theme.text.secondary),
        ),
    ]));

    append_text_block(
        lines,
        &format!("args {}", tool_call.args_summary),
        theme.text.secondary,
        "  ",
    );

    match tool_call.status {
        ToolCallDisplayStatus::Succeeded => {
            if let Some(output) = &tool_call.truncated_output {
                lines.push(Line::from(vec![
                    Span::styled("  ↳ ", Style::default().fg(theme.status.success)),
                    Span::styled(output.clone(), Style::default().fg(theme.text.primary)),
                ]));
            }
        }
        ToolCallDisplayStatus::Failed => {
            if let Some(error) = &tool_call.output_summary {
                lines.push(Line::from(vec![
                    Span::styled("  ↳ ", Style::default().fg(theme.status.error)),
                    Span::styled(error.clone(), Style::default().fg(theme.status.error)),
                ]));
            }
        }
        _ => {}
    }
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

fn append_text_block(
    lines: &mut Vec<Line<'static>>,
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
    let runtime_state = compact_state(app);

    let border_style = panel_border_style(theme, is_focused);

    let title = format!("Inspector{}", if is_focused { " (focused)" } else { "" });

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.chrome.title));

    let content = if let Some(activity) = app.activities.get(app.selected_activity_index) {
        let mut lines = Vec::new();

        if let Some(detail) = runtime_state.detail.as_deref() {
            lines.push(Line::from(vec![
                Span::styled("Runtime: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    runtime_state.kind.label(),
                    Style::default()
                        .fg(runtime_state.kind.color(theme))
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                detail,
                Style::default().fg(theme.text.secondary),
            )));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![
            Span::styled(
                "Request ID: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                request_id_label(&activity.request_id),
                Style::default().fg(theme.text.primary),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Provider: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                &activity.provider_id,
                Style::default().fg(theme.text.primary),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Model: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&activity.model_id, Style::default().fg(theme.text.primary)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{}", activity.status),
                match activity.status {
                    crate::app::ActivityStatus::Error => Style::default().fg(theme.status.error),
                    crate::app::ActivityStatus::Done => Style::default().fg(theme.status.success),
                    _ => Style::default().fg(theme.text.primary),
                },
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Sequences: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{}-{}", activity.first_seq, activity.last_seq),
                Style::default().fg(theme.text.primary),
            ),
        ]));

        if let Some(req_data) = &activity.request_data {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Request:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            match serde_json::to_string_pretty(req_data) {
                Ok(json) => lines.push(Line::from(json)),
                Err(_) => lines.push(Line::from("[error serializing]")),
            }
        }

        if let Some(error) = &activity.error_message {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Error:",
                Style::default()
                    .fg(theme.status.error)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                error,
                Style::default().fg(theme.status.error),
            )));
        }

        Text::from(lines)
    } else if let Some(detail) = runtime_state.detail.as_deref() {
        Text::from(vec![
            Line::from(vec![
                Span::styled("Runtime: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    runtime_state.kind.label(),
                    Style::default()
                        .fg(runtime_state.kind.color(theme))
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
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render the Prompt input pane (bottom)
fn render_prompt_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Prompt;
    let runtime_state = compact_state(app);
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
            "Composer ({} lines · {} chars){}",
            composer_lines,
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

    let (text, style) = if composer_disabled {
        (
            match runtime_state.kind {
                CompactStateKind::Degraded => {
                    "Composer disabled — waiting for live recovery before sending.".to_string()
                }
                CompactStateKind::Disconnected => {
                    "Composer disabled — reopen the TUI to reconnect the live stream.".to_string()
                }
                _ => "Composer disabled — wait for the live session to recover.".to_string(),
            },
            Style::default().fg(theme.text.secondary),
        )
    } else if text.is_empty() {
        (
            "Type a prompt for the next turn…".to_string(),
            Style::default().fg(theme.text.secondary),
        )
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

/// Render the Help tab
fn render_help_tab(frame: &mut Frame, area: Rect, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Help")
        .title_style(Style::default().fg(theme.chrome.title));

    let help_text = r#"Keyboard Shortcuts:

Navigation:
  j/↓          Move down in list
  k/↑          Move up in list
  Tab          Cycle focus forward
  Shift+Tab    Cycle focus backward
  Space        Toggle follow mode

Secondary access:
  1            Return to live conversation
  i            Toggle live details drawer
  2            Open Events surface
  3            Open Diff surface
  4 / h        Open Help surface
  replay       Conversation / Events / Diff / Help stay in the tab bar

Prompt (when focused):
  Enter        Submit prompt
  Shift+Enter  Insert newline
  Esc          Clear prompt
  ↑/↓          Navigate history

Permission Modal:
  a            Allow permission
  d            Deny permission
  Esc          Dismiss modal

General:
  q            Quit
  r            Reload (replay mode only)
  ?            Show this help
"#;

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .style(Style::default().fg(theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let state = compact_state(app);
    let separator = " ".repeat(theme.live_shell.rhythm.status_separator as usize);
    let hint_text = if app.replay_mode {
        [
            "Tab nav", "1 convo", "2 events", "3 diff", "4 help", "r reload", "q quit",
        ]
        .join(&separator)
    } else if live_secondary_surface_open(app) {
        [
            "1 convo",
            "i details",
            "2 ev",
            "3 diff",
            "4 help",
            "Tab focus",
            "q quit",
        ]
        .join(&separator)
    } else {
        let details_hint = if app.details_drawer_open() {
            "i close"
        } else {
            "i details"
        };
        if state.composer_disabled {
            [details_hint, "2 ev", "3 diff", "4 help", "q quit"].join(&separator)
        } else {
            [
                "Enter send",
                "⇧↵ nl",
                details_hint,
                "2 ev",
                "3 diff",
                "4 help",
                "q quit",
            ]
            .join(&separator)
        }
    };

    let footer = Paragraph::new(hint_text).style(
        Style::default()
            .fg(theme.chrome.footer_fg)
            .bg(theme.chrome.canvas),
    );

    frame.render_widget(footer, area);
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

    #[test]
    fn theme_provides_default_colors() {
        let theme = Theme::default();
        assert!(matches!(
            theme.chrome.canvas,
            ratatui::style::Color::Rgb(_, _, _)
        ));
    }
}
