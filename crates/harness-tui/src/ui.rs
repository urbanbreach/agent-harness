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
use crate::layout::{
    centered_block_area, composer_input_height, details_drawer_areas, inset_rect,
    secondary_surface_layout, split_secondary_surface, FrameLayoutPlan,
};
use crate::overlay::OverlayKind;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelTarget {
    Transcript,
    Inspector,
}

pub fn render_app(frame: &mut Frame, app: &AppState) {
    let theme = app.theme();
    let area = frame.area();
    let plan = FrameLayoutPlan::for_app(app, area);

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.canvas)),
        area,
    );

    render_header(frame, app, plan.header, plan.header_text, theme);
    render_content(frame, app, plan.content, theme, &plan);
    render_footer(frame, app, plan.footer, plan.footer_text, theme);
    render_overlays(frame, app, theme, &plan);
}

fn render_header(frame: &mut Frame, app: &AppState, area: Rect, text_area: Rect, theme: &Theme) {
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
        let metadata = format!(
            "run {run_id} · {}/{} · {}",
            app.active_profile(),
            app.active_provider(),
            app.current_model_label()
        );
        app.launch_mode_label()
            .map(|label| format!("{label} · {metadata}"))
            .unwrap_or(metadata)
    };

    if app.replay_mode {
        let style = Style::default()
            .fg(theme.text.secondary)
            .bg(theme.surface.shell);
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(Paragraph::new(header_text).style(style), text_area);
    } else {
        frame.render_widget(
            Paragraph::new(header_text).style(Style::default().fg(theme.text.tertiary)),
            text_area,
        );
    }
}

fn render_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    plan: &FrameLayoutPlan,
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
        render_surface(frame, app, chunks[1], theme, plan);
    } else {
        render_surface(frame, app, area, theme, plan);
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
                Style::default().fg(theme.text.secondary)
            };
            Line::from(Span::styled(surface.label, style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(panel_block(theme, "Tabs", false, theme.surface.panel))
        .select(replay_tab_index(app.active_tab))
        .style(panel_style(theme.surface.panel, theme.text.tertiary))
        .highlight_style(panel_style(theme.surface.panel, theme.text.accent));

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
    plan: &FrameLayoutPlan,
) {
    let area = if app.replay_mode { area } else { plan.shell };

    match app.active_tab {
        Tab::Run => {
            if app.replay_mode {
                render_run_workspace(frame, app, area, theme)
            } else {
                render_live_session_surface(frame, app, theme, plan)
            }
        }
        Tab::Details => render_live_session_surface(frame, app, theme, plan),
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
fn render_run_workspace(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let shell = app.theme().live_shell_layout(area.width, area.height);
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
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    let Some(transcript_area) = plan.transcript else {
        return;
    };
    let Some(status_area) = plan.status else {
        return;
    };
    let Some(composer_area) = plan.composer else {
        return;
    };

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.shell)),
        plan.shell,
    );
    render_transcript_pane(frame, app, transcript_area, theme);
    render_status_strip(frame, app, status_area, theme);
    render_prompt_pane(frame, app, composer_area, theme);
}

fn render_overlays(frame: &mut Frame, app: &AppState, theme: &Theme, plan: &FrameLayoutPlan) {
    for overlay in &app.overlay_stack() {
        match overlay {
            OverlayKind::DetailsDrawer => {
                render_live_details_overlay(frame, app, theme, plan.details_overlay)
            }
            OverlayKind::CommandPalette => {
                render_command_palette_overlay(frame, app, theme, plan.palette_overlay)
            }
            OverlayKind::PermissionModal => {
                if let Some((permission_id, summary)) = app.active_permission() {
                    if let Some(modal) = plan.permission_modal {
                        render_permission_modal(frame, &permission_id, &summary, theme, modal);
                    }
                }
            }
        }
    }
}

fn render_command_palette_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    frame.render_widget(Clear, overlay);
    let card_surface = theme.surface.panel_elevated;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border.focus))
        .style(Style::default().bg(card_surface));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Commands",
            Style::default()
                .fg(theme.text.accent)
                .bg(card_surface)
                .add_modifier(Modifier::BOLD),
        ))),
        sections[0],
    );

    render_command_palette_input(frame, app, theme, sections[1]);
    render_command_palette_list(frame, app, theme, sections[2]);
}

fn render_command_palette_input(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.overlay)),
        area,
    );

    let mut input = app.palette_input.clone();
    let cursor_byte = input
        .char_indices()
        .nth(app.palette_cursor)
        .map(|(index, _)| index)
        .unwrap_or(input.len());
    input.insert(cursor_byte, '█');

    let line = Line::from(vec![
        Span::styled(
            "> ",
            Style::default()
                .fg(theme.text.accent)
                .bg(theme.surface.overlay)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            input,
            Style::default()
                .fg(theme.text.primary)
                .bg(theme.surface.overlay),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_command_palette_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.palette_filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No commands",
                Style::default().fg(theme.text.secondary),
            ))),
            area,
        );
        return;
    }

    let visible_rows = usize::from(area.height);
    let selected = app
        .palette_selected
        .min(app.palette_filtered.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));

    for (row, command) in app
        .palette_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let row_y = area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        let is_selected = row == selected;
        if is_selected {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.surface.overlay)),
                row_area,
            );
        }

        frame.render_widget(
            Paragraph::new(command_palette_row(
                command,
                palette_command_description(command),
                is_selected,
                theme,
                row_area.width,
            )),
            row_area,
        );
    }
}

fn command_palette_row(
    command: &str,
    description: &str,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let row_style = if is_selected {
        Style::default()
            .fg(theme.text.inverse)
            .bg(theme.surface.overlay)
    } else {
        Style::default()
    };
    let label_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(theme.text.primary)
    };
    let description_style = if is_selected {
        row_style
    } else {
        row_style.fg(theme.text.secondary)
    };

    let mut spans = vec![Span::styled(command.to_string(), label_style)];
    let mut used_width = command.chars().count();

    let gap_width = 2;
    let available_description = row_width.saturating_sub(used_width.saturating_add(gap_width));
    let description = truncate_plain_text(description, available_description);
    if !description.is_empty() {
        spans.push(Span::styled("  ", row_style));
        used_width = used_width.saturating_add(gap_width);
        used_width = used_width.saturating_add(description.chars().count());
        spans.push(Span::styled(description, description_style));
    }

    if is_selected && used_width < row_width {
        spans.push(Span::styled(" ".repeat(row_width - used_width), row_style));
    }

    Line::from(spans)
}

fn palette_command_description(command: &str) -> &'static str {
    Action::palette_commands()
        .iter()
        .find_map(|(candidate, description)| (*candidate == command).then_some(*description))
        .unwrap_or("")
}

fn truncate_plain_text(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let text_width = text.chars().count();
    if text_width <= max_width {
        return text.to_string();
    }

    if max_width == 1 {
        return "…".to_string();
    }

    let truncated = text
        .chars()
        .take(max_width.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
}

fn render_details_drawer(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let drawer_chunks = details_drawer_areas(area);

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.panel)),
        area,
    );
    render_details_activity_card(frame, app, drawer_chunks[0], theme);
    render_details_inspector_card(frame, app, drawer_chunks[1], theme);
}

fn render_live_details_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    frame.render_widget(Clear, overlay);
    render_details_drawer(frame, app, overlay, theme);
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

fn panel_style(surface: Color, foreground: Color) -> Style {
    Style::default().fg(foreground).bg(surface)
}

fn panel_block<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
    surface: Color,
) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(panel_border_style(theme, is_focused))
        .style(Style::default().bg(surface))
        .title(title)
        .title_style(panel_style(surface, theme.text.secondary))
}

fn elevated_card_block<'a>(
    title: impl Into<Line<'a>>,
    surface: Color,
    border: Color,
    title_color: Color,
) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(surface))
        .title(title)
        .title_style(panel_style(surface, title_color))
}

fn panel_border_style(theme: &Theme, is_focused: bool) -> Style {
    let border = if is_focused {
        theme.border.focus
    } else {
        theme.border.subtle
    };
    Style::default().fg(border)
}

fn request_id_label(request_id: &str) -> Cow<'_, str> {
    if request_id.is_empty() {
        Cow::Borrowed("pending turn")
    } else {
        Cow::Borrowed(request_id)
    }
}

fn transcript_label_style(theme: &Theme, is_selected: bool) -> Style {
    let color = if is_selected {
        theme.text.accent
    } else {
        theme.text.primary
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn muted_meta_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.secondary)
}

fn subdued_payload_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.tertiary)
}

fn transcript_prefix_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.tertiary)
}

fn status_badge(label: impl Into<String>, color: Color, theme: &Theme) -> Span<'static> {
    Span::styled(
        format!(" {} ", label.into()),
        Style::default()
            .fg(theme.text.inverse)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

fn append_prefixed_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    prefix: &str,
    prefix_style: Style,
    content_style: Style,
) {
    for line in text.lines() {
        let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
        if !line.is_empty() {
            spans.push(Span::styled(line.to_string(), content_style));
        }
        lines.push(Line::from(spans));
    }

    if text.is_empty() {
        lines.push(Line::from(Span::styled(prefix.to_string(), prefix_style)));
    }
}

fn tool_status_tokens(
    status: ToolCallDisplayStatus,
    theme: &Theme,
) -> (&'static str, Color, &'static str, bool) {
    match status {
        ToolCallDisplayStatus::PendingPermission => (
            theme.live_shell.glyphs.pending_permission,
            theme.status.warning,
            "pending permission",
            false,
        ),
        ToolCallDisplayStatus::Queued => (
            theme.live_shell.glyphs.queued,
            theme.text.secondary,
            "queued",
            false,
        ),
        ToolCallDisplayStatus::Running => (
            theme.live_shell.glyphs.running,
            theme.text.accent,
            "running",
            false,
        ),
        ToolCallDisplayStatus::Succeeded => (
            theme.live_shell.glyphs.succeeded,
            theme.status.success,
            "succeeded",
            true,
        ),
        ToolCallDisplayStatus::Failed => (
            theme.live_shell.glyphs.failed,
            theme.status.error,
            "failed",
            true,
        ),
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
        RuntimeStateKind::Ready => theme.status.info,
        RuntimeStateKind::Success => theme.status.success,
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming => theme.status.info,
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
        .fg(theme.text.secondary)
        .bg(theme.surface.shell);
    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} ", state.kind.label()),
            Style::default()
                .fg(theme.text.inverse)
                .bg(runtime_state_color(state.kind, theme))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", base_style),
        Span::styled(state.summary, base_style),
    ]);

    frame.render_widget(Paragraph::new(status_line).style(base_style), area);
}

/// Render the Activity pane (left)
fn render_activity_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);

    let title = format!(
        "Activity (j/k active{}{})",
        if app.follow_mode { ", follow" } else { "" },
        if is_focused { ", focused" } else { "" }
    );

    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, title, is_focused, surface);

    if app.activities.is_empty() {
        let empty = Paragraph::new("No activities yet")
            .block(block)
            .style(panel_style(surface, theme.text.secondary));
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
                    .fg(theme.text.inverse)
                    .bg(theme.border.focus)
                    .add_modifier(Modifier::BOLD)
            } else {
                panel_style(surface, theme.text.primary)
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
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: false });

    frame.render_widget(activity_list, area);
}

fn render_details_activity_card(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);
    let title = format!(
        "Activity summary{}{}",
        if app.follow_mode { " · follow" } else { "" },
        if is_focused { " · focused" } else { "" }
    );
    let surface = theme.surface.panel_elevated;
    let border = if is_focused {
        theme.border.focus
    } else {
        theme.border.strong
    };
    let block = elevated_card_block(
        title,
        surface,
        border,
        if is_focused {
            theme.text.primary
        } else {
            theme.text.secondary
        },
    );

    if app.activities.is_empty() {
        frame.render_widget(
            Paragraph::new("No activities yet")
                .block(block)
                .style(panel_style(surface, theme.text.secondary)),
            area,
        );
        return;
    }

    let rows: Vec<Line> = app
        .activities
        .iter()
        .enumerate()
        .map(|(idx, activity)| {
            let is_selected = idx == app.selected_activity_index;
            let (_, badge_color, status_label) = match activity.status {
                ActivityStatus::Streaming => (
                    theme.live_shell.glyphs.streaming,
                    theme.status.info,
                    "streaming…",
                ),
                ActivityStatus::Done => {
                    (theme.live_shell.glyphs.done, theme.status.success, "done")
                }
                ActivityStatus::Error => {
                    (theme.live_shell.glyphs.error, theme.status.error, "error")
                }
            };
            let marker = if is_selected {
                format!("{} ", theme.live_shell.transcript_glyphs.user_marker)
            } else {
                "  ".to_string()
            };
            let model_display = if activity.model_id.is_empty() {
                "-"
            } else {
                activity.model_id.as_str()
            };

            Line::from(vec![
                Span::styled(
                    marker,
                    Style::default().fg(if is_selected {
                        theme.text.accent
                    } else {
                        theme.text.tertiary
                    }),
                ),
                Span::styled(
                    request_id_label(&activity.request_id).into_owned(),
                    transcript_label_style(theme, is_selected),
                ),
                Span::styled(format!(" · {model_display} · "), muted_meta_style(theme)),
                status_badge(status_label, badge_color, theme),
            ])
        })
        .collect();

    frame.render_widget(
        Paragraph::new(Text::from(rows))
            .block(block)
            .style(panel_style(surface, theme.text.primary))
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Render the Transcript pane (center)
fn render_transcript_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if !app.replay_mode {
        let inner_area = inset_rect(area, theme.live_shell.rhythm.transcript_gutter_x, 0);

        if live_empty_state_visible(app) {
            render_live_empty_state(frame, app, inner_area, theme);
            return;
        }

        let lines = build_transcript_lines(app, theme);
        let transcript_scroll = transcript_scroll_offset(app, &lines, inner_area);
        let content = Text::from(lines);

        frame.render_widget(
            Paragraph::new(content)
                .style(panel_style(theme.surface.shell, theme.text.primary))
                .scroll((transcript_scroll, 0))
                .wrap(Wrap { trim: false }),
            inner_area,
        );
        return;
    }

    let is_focused = transcript_surface_focused(app);

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

    let surface = theme.surface.panel;
    let block = panel_block(theme, title, is_focused, surface);

    let inner_area = inset_rect(
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
            .style(panel_style(surface, theme.text.primary))
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
    let hit_areas = FrameLayoutPlan::for_app(app, area).wheel_hit_areas;
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

    let _ = live_empty_state_mode_label(app, theme);

    let help_row = [
        app.keymap.get_binding_label(Action::SubmitPrompt, "send"),
        app.keymap
            .get_binding_label(Action::InsertNewline, "newline"),
        format!(
            "{}/{} history",
            app.keymap.get_binding_str(Action::HistoryUp),
            app.keymap.get_binding_str(Action::HistoryDown)
        ),
    ]
    .join(" · ");

    let content_area = centered_block_area(
        area,
        area.width
            .min(theme.live_shell.empty_state.max_width)
            .max(1),
        4,
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    frame.render_widget(
        Paragraph::new(theme.live_shell.empty_state.title)
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(theme.surface.shell)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new("").style(
            Style::default()
                .fg(theme.text.tertiary)
                .bg(theme.surface.shell),
        ),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(theme.live_shell.empty_state.value_prop)
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(theme.surface.shell)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(help_row)
            .style(
                Style::default()
                    .fg(theme.text.secondary)
                    .bg(theme.surface.shell),
            )
            .alignment(Alignment::Left),
        rows[3],
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
    let header_style = transcript_label_style(theme, is_selected);
    let meta_style = muted_meta_style(theme);

    if let Some(user_msg) = &activity.user_message {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", theme.live_shell.transcript_glyphs.user_marker),
                Style::default().fg(theme.text.accent),
            ),
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
            theme.status.info,
            "streaming…",
        ),
        ActivityStatus::Done => (theme.live_shell.glyphs.done, theme.status.success, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };
    let mut assistant_meta = Vec::new();
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
    let mut assistant_line = vec![
        Span::styled(
            format!("{} ", assistant_icon),
            Style::default().fg(assistant_color),
        ),
        status_badge(assistant_status, assistant_color, theme),
        Span::raw(" "),
        Span::styled("assistant", header_style),
    ];
    if !assistant_meta.is_empty() {
        assistant_line.push(Span::styled(
            format!(" · {}", assistant_meta.join(" · ")),
            meta_style,
        ));
    }
    lines.push(Line::from(assistant_line));

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
    let glyphs = &theme.live_shell.transcript_glyphs;
    let (status_icon, status_color, status_label, is_final) =
        tool_status_tokens(tool_call.status, theme);

    lines.push(Line::from(vec![
        Span::styled(
            format!("  {} ", glyphs.card_top),
            transcript_prefix_style(theme),
        ),
        Span::styled("tool ", muted_meta_style(theme)),
        Span::styled(
            tool_call.tool_id.clone(),
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if let Some(summary) = tool_call.transcript_summary() {
        append_prefixed_text_block(
            lines,
            &summary,
            &format!("  {}  ", glyphs.card_mid),
            transcript_prefix_style(theme),
            subdued_payload_style(theme),
        );
    }

    let mut status_line = vec![
        Span::styled(
            format!("  {} ", glyphs.card_bottom),
            transcript_prefix_style(theme),
        ),
        Span::styled(
            format!("{} ", status_icon),
            Style::default().fg(status_color),
        ),
    ];
    if is_final {
        status_line.push(status_badge(status_label, status_color, theme));
    } else {
        status_line.push(Span::styled(status_label, subdued_payload_style(theme)));
    }
    lines.push(Line::from(status_line));
}

fn append_pending_permission_lines(lines: &mut Vec<Line<'static>>, summary: &str, theme: &Theme) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", theme.live_shell.glyphs.pending_permission),
            Style::default().fg(theme.status.warning),
        ),
        status_badge("requested", theme.status.warning, theme),
        Span::raw(" "),
        Span::styled(
            "permission",
            Style::default()
                .fg(theme.status.warning)
                .add_modifier(Modifier::BOLD),
        ),
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
fn build_inspector_content(app: &AppState, theme: &Theme) -> Text<'static> {
    let runtime_state = app.runtime_state();

    if let Some(activity) = app.activities.get(app.selected_activity_index) {
        let mut lines = Vec::new();

        if let Some(detail) = runtime_state.detail.clone() {
            lines.push(Line::from(vec![
                Span::styled("Runtime ", Style::default().add_modifier(Modifier::BOLD)),
                status_badge(
                    runtime_state.kind.label(),
                    runtime_state_color(runtime_state.kind, theme),
                    theme,
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
                Span::styled("Runtime ", Style::default().add_modifier(Modifier::BOLD)),
                status_badge(
                    runtime_state.kind.label(),
                    runtime_state_color(runtime_state.kind, theme),
                    theme,
                ),
            ]),
            Line::from(Span::styled(
                detail,
                Style::default().fg(theme.text.secondary),
            )),
        ])
    } else {
        Text::from("No activity selected")
    }
}

fn render_inspector_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details && activity_surface_visible(app);

    let title = format!("Inspector{}", if is_focused { " (focused)" } else { "" });
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, title, is_focused, surface);
    let content = build_inspector_content(app, theme);

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .scroll((app.details_scroll, 0))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn render_details_inspector_card(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details && activity_surface_visible(app);
    let surface = theme.surface.panel_elevated;
    let border = if is_focused {
        theme.border.focus
    } else {
        theme.border.strong
    };
    let block = elevated_card_block(
        format!(
            "Inspector detail{}",
            if is_focused { " · focused" } else { "" }
        ),
        surface,
        border,
        if is_focused {
            theme.text.primary
        } else {
            theme.text.secondary
        },
    );

    frame.render_widget(
        Paragraph::new(build_inspector_content(app, theme))
            .block(block)
            .style(panel_style(surface, theme.text.primary))
            .scroll((app.details_scroll, 0))
            .wrap(Wrap { trim: true }),
        area,
    );
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

fn append_permission_details(
    lines: &mut Vec<Line<'static>>,
    permissions: &[crate::app::PermissionEntry],
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

fn append_tool_call_details(
    lines: &mut Vec<Line<'static>>,
    tool_calls: &[crate::app::ToolCallEntry],
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

    let char_count = app.prompt_buffer.chars().count();
    let composer_lines = composer_input_height(&app.prompt_buffer, area.width);
    let title = if composer_disabled {
        format!("Composer · disabled · {}", runtime_state.kind.label())
    } else {
        format!(
            "Composer · {} {} · {} chars",
            composer_lines,
            line_label(composer_lines),
            char_count
        )
    };

    let surface = theme.surface.panel_elevated;
    let border_color = if composer_disabled {
        theme.status.disabled
    } else if is_focused {
        theme.border.focus
    } else {
        theme.border.subtle
    };
    let title_color = if composer_disabled {
        theme.status.disabled
    } else if is_focused {
        theme.text.primary
    } else {
        theme.text.secondary
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(surface))
        .title(Line::from(Span::styled(
            title,
            Style::default().fg(title_color),
        )));

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
        let hint_color = if composer_disabled {
            theme.status.disabled
        } else {
            theme.text.secondary
        };
        (
            runtime_state.composer_hint,
            panel_style(surface, hint_color),
        )
    } else if composer_disabled {
        (text, panel_style(surface, theme.status.disabled))
    } else {
        (text, panel_style(surface, theme.text.primary))
    };

    let paragraph = Paragraph::new(text)
        .block(block)
        .style(style)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Render the Events tab (legacy 2-pane layout)
fn render_events_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let layout = render_secondary_surface_shell(frame, area, theme);
    let [event_list_area, event_details_area] =
        split_secondary_surface(layout.body, 40, theme.live_shell.rhythm.surface_gap);

    render_event_list(frame, app, event_list_area, theme);
    render_event_details(frame, app, event_details_area, theme);
}

/// Render the event list (left pane of Events tab)
fn render_event_list(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List;

    let follow_indicator = if app.follow_mode { ", follow" } else { "" };
    let title = format!("Events (j/k active{})", follow_indicator);
    let surface = theme.surface.panel;
    let block = panel_block(theme, title, is_focused, surface);

    if app.events.is_empty() {
        let empty = Paragraph::new("No events")
            .block(block)
            .style(panel_style(surface, theme.text.secondary));
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
                    .fg(theme.text.inverse)
                    .bg(theme.border.focus)
                    .add_modifier(Modifier::BOLD)
            } else {
                panel_style(surface, theme.text.primary)
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
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: false });

    frame.render_widget(list, area);
}

/// Render event details (right pane of Events tab)
fn render_event_details(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details;

    let title = "Event details";
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, title, is_focused, surface);

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
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render the Diff tab
fn render_diff_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let layout = render_secondary_surface_shell(frame, area, theme);
    let [event_list_area, diff_area] =
        split_secondary_surface(layout.body, 40, theme.live_shell.rhythm.surface_gap);

    render_event_list(frame, app, event_list_area, theme);

    // Right pane: diff viewer
    let is_focused = app.focus == Focus::Details;
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, "Diff", is_focused, surface);

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
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, diff_area);
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
    let layout = render_secondary_surface_shell(frame, area, theme);
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, "Help", false, surface);

    let paragraph = Paragraph::new(help_text(app))
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, layout.body);
}

fn render_secondary_surface_shell(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
) -> crate::layout::SecondarySurfaceLayout {
    let layout = secondary_surface_layout(area, theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.shell)),
        layout.shell,
    );
    layout
}

fn render_footer(frame: &mut Frame, app: &AppState, area: Rect, text_area: Rect, theme: &Theme) {
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

    let style = Style::default().fg(theme.text.tertiary);
    if app.replay_mode {
        let replay_style = style.bg(theme.surface.shell);
        frame.render_widget(Block::default().style(replay_style), area);
        frame.render_widget(Paragraph::new(hint_text).style(replay_style), text_area);
    } else {
        frame.render_widget(Paragraph::new(hint_text).style(style), text_area);
    }
}

/// Render the permission modal
fn render_permission_modal(
    frame: &mut Frame,
    _permission_id: &str,
    summary: &str,
    theme: &Theme,
    popup_rect: Rect,
) {
    frame.render_widget(Clear, popup_rect);
    let surface = theme.surface.overlay;
    let block = elevated_card_block(
        Line::from(vec![
            Span::styled(
                format!("{} ", theme.live_shell.glyphs.pending_permission),
                Style::default().fg(theme.status.warning),
            ),
            Span::styled(
                "Permission Requested",
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        surface,
        theme.border.focus,
        theme.text.accent,
    );
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(summary)
            .style(panel_style(surface, theme.text.primary))
            .wrap(Wrap { trim: true }),
        sections[0],
    );

    frame.render_widget(
        Paragraph::new("[a]llow  [d]eny  [esc]dismiss")
            .style(
                Style::default()
                    .fg(theme.text.secondary)
                    .bg(theme.surface.panel_elevated)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        sections[1],
    );
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
        build_transcript_lines(app, app.theme())
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
            theme.surface.canvas,
            ratatui::style::Color::Rgb(_, _, _)
        ));
    }

    #[test]
    fn wheel_hit_testing_uses_app_theme() {
        let area = Rect::new(0, 0, 140, 40);

        let mut default_app = AppState::new_live(None, false, None);
        default_app.active_tab = Tab::Details;
        let default_hit_areas = FrameLayoutPlan::for_app(&default_app, area).wheel_hit_areas;

        let mut themed_app = AppState::new_live(None, false, None);
        themed_app.active_tab = Tab::Details;
        let mut custom_theme = Theme::default();
        custom_theme.live_shell.primary.centered_content_width = 72;
        custom_theme.live_shell.primary.content_margin_x = 6;
        custom_theme.live_shell.primary.activity_drawer_width = 18;
        custom_theme.live_shell.primary.inspector_drawer_width = 36;
        themed_app.set_theme(custom_theme);

        let themed_hit_areas = FrameLayoutPlan::for_app(&themed_app, area).wheel_hit_areas;
        assert_ne!(default_hit_areas.overlay, themed_hit_areas.overlay);
        assert_ne!(default_hit_areas.inspector, themed_hit_areas.inspector);

        assert_eq!(
            hovered_wheel_target(&themed_app, area, 40, 20),
            Some(WheelTarget::Inspector)
        );
        assert_ne!(
            hovered_wheel_target(&default_app, area, 40, 20),
            Some(WheelTarget::Inspector)
        );
    }

    #[test]
    fn live_header_uses_actual_launch_metadata() {
        let mut app = AppState::new_live(None, false, None);
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Demo"));
        assert!(debug.contains("run unknown"));
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
    fn live_empty_state_uses_shared_startup_copy_without_mode_badges() {
        let mut demo = AppState::new_live(None, false, None);
        demo.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
        );

        let demo_debug = render_debug(&demo, 100, 24);
        assert!(demo_debug.contains("Harness"));
        assert!(demo_debug.contains("Start a conversation to begin"));
        assert!(!demo_debug.contains("Demo mode · mock provider"));

        let mut mock = AppState::new_live(None, false, None);
        mock.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
        );

        let mock_debug = render_debug(&mock, 100, 24);
        assert!(mock_debug.contains("Harness"));
        assert!(mock_debug.contains("Start a conversation to begin"));
        assert!(!mock_debug.contains("Mock mode · mock provider"));
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
        let hit_areas = FrameLayoutPlan::for_app(&app, area).wheel_hit_areas;
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
        let hit_areas = FrameLayoutPlan::for_app(&app, area).wheel_hit_areas;
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
        let hit_areas = FrameLayoutPlan::for_app(&app, area).wheel_hit_areas;
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
        assert!(transcript.contains("tool fs.read"));
        assert!(transcript.contains("12 lines read"));
        assert!(transcript.contains("succeeded"));
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
        assert!(transcript.contains("tool shell.run"));
        assert!(transcript.contains("exit code: 1 stderr: permission denied"));
        assert!(transcript.contains("failed"));
        assert!(!transcript.contains(r#"{"cmd":"false","cwd":"/tmp/demo"}"#));
        assert!(!transcript.contains("args {"));
    }
}
