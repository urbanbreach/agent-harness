use std::borrow::Cow;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::app::{
    session_history_profile_label, session_history_provider_model_label,
    session_history_recency_label, session_history_resumability_label, session_history_run_name,
    session_history_status_label, ActivityEntry, ActivityStatus, AppState, Focus,
    OrchestrationTaskRow, OrchestrationTaskState, RuntimeStateKind, StartupLauncherAction, Tab,
    ToolCallDisplayStatus,
};
use crate::keybindings::Action;
use crate::layout::{
    composer_input_height, details_drawer_areas, inset_rect, secondary_surface_layout,
    split_secondary_surface, startup_shell_area, FrameLayoutPlan,
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
    if startup_shell_visible(app) {
        return;
    }

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
    render_live_details_overlay(frame, app, theme, plan.details_overlay);
    render_status_strip(frame, app, status_area, theme);
    render_prompt_pane(frame, app, composer_area, theme);
}

fn render_overlays(frame: &mut Frame, app: &AppState, theme: &Theme, plan: &FrameLayoutPlan) {
    for overlay in &app.overlay_stack() {
        match overlay {
            OverlayKind::DetailsDrawer => {}
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
    let block = elevated_card_block(
        Line::from(Span::styled(
            if app.session_history_visible {
                session_history_overlay_title(app)
            } else {
                "Command palette"
            },
            Style::default()
                .fg(theme.text.accent)
                .add_modifier(Modifier::BOLD),
        )),
        card_surface,
        theme.border.focus,
        theme.text.accent,
    );
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.session_history_visible {
        render_session_history_overlay(frame, app, theme, inner, card_surface);
    } else {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        render_command_palette_input(frame, app, theme, sections[0]);
        render_command_palette_list(frame, app, theme, sections[1]);
    }
}

fn render_session_history_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    card_surface: Color,
) {
    let show_banner = app.continue_disabled_banner.is_some();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if show_banner { 1 } else { 0 }),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    if let Some(banner) = app.continue_disabled_banner.as_deref() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate_plain_text(banner, usize::from(sections[0].width)),
                Style::default()
                    .fg(theme.status.warning)
                    .bg(card_surface)
                    .add_modifier(Modifier::BOLD),
            ))),
            sections[0],
        );
    }

    render_command_palette_input(frame, app, theme, sections[1]);
    render_session_history_list(frame, app, theme, sections[2]);
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
                Action::palette_command_label(command),
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
    label: &str,
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

    let mut spans = vec![Span::styled(label.to_string(), label_style)];
    let mut used_width = label.chars().count();

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

fn render_session_history_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.session_history_filtered.is_empty() {
        let empty = if app.session_history_entries.is_empty() {
            "No session history"
        } else {
            "No matching sessions"
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                empty,
                Style::default().fg(theme.text.secondary),
            ))),
            area,
        );
        return;
    }

    let row_height = 2usize;
    let visible_rows = (usize::from(area.height) / row_height).max(1);
    let selected = app
        .session_history_selected
        .min(app.session_history_filtered.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));

    for (visible_index, entry_index) in app
        .session_history_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let entry = &app.session_history_entries[*entry_index];
        let row_offset = (visible_index - scroll) * row_height;
        let row_y = area
            .y
            .saturating_add(u16::try_from(row_offset).unwrap_or(u16::MAX));
        if row_y >= area.y.saturating_add(area.height) {
            break;
        }

        let remaining_height = area
            .height
            .saturating_sub(u16::try_from(row_offset).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, remaining_height.min(2));
        let is_selected = visible_index == selected;
        if is_selected {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.surface.overlay)),
                row_area,
            );
        }

        let primary_area = Rect::new(row_area.x, row_area.y, row_area.width, 1);
        frame.render_widget(
            Paragraph::new(session_history_primary_line(entry, app, is_selected, theme)),
            primary_area,
        );

        if row_area.height > 1 {
            let secondary_area =
                Rect::new(row_area.x, row_area.y.saturating_add(1), row_area.width, 1);
            frame.render_widget(
                Paragraph::new(session_history_secondary_line(
                    entry,
                    app,
                    is_selected,
                    theme,
                    secondary_area.width,
                )),
                secondary_area,
            );
        }
    }
}

fn session_history_overlay_title(app: &AppState) -> &'static str {
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => "Replay session",
        StartupLauncherAction::ContinueSession => "Resume session",
        StartupLauncherAction::NewSession => "Session history",
    }
}

fn session_history_primary_line(
    entry: &crate::app::SessionHistoryEntry,
    app: &AppState,
    is_selected: bool,
    theme: &Theme,
) -> Line<'static> {
    let row_style = if is_selected {
        Style::default()
            .fg(theme.text.inverse)
            .bg(theme.surface.overlay)
    } else {
        Style::default()
    };
    let title_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text.primary)
            .add_modifier(Modifier::BOLD)
    };
    let meta_style = if is_selected {
        row_style
    } else {
        Style::default().fg(theme.text.secondary)
    };
    let action_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(session_history_action_color(app, entry, theme))
            .add_modifier(Modifier::BOLD)
    };
    let status_style = if is_selected {
        row_style
    } else {
        Style::default().fg(session_history_status_color(entry, theme))
    };

    Line::from(vec![
        Span::styled(session_history_action_prefix(app, entry), action_style),
        Span::styled(session_history_run_name(entry).to_string(), title_style),
        Span::styled(" · ", meta_style),
        Span::styled(session_history_recency_label(entry), meta_style),
        Span::styled(" · ", meta_style),
        Span::styled(
            session_history_status_label(entry).to_string(),
            status_style,
        ),
    ])
}

fn session_history_secondary_line(
    entry: &crate::app::SessionHistoryEntry,
    app: &AppState,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_style = if is_selected {
        Style::default()
            .fg(theme.text.inverse)
            .bg(theme.surface.overlay)
    } else {
        Style::default().fg(match app.startup_launcher_action {
            StartupLauncherAction::ReplaySession => theme.text.secondary,
            StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
                if entry.catalog.is_resumable {
                    theme.status.success
                } else {
                    theme.status.warning
                }
            }
        })
    };
    let content = format!(
        "  {} · {} · {}",
        session_history_profile_label(entry),
        session_history_provider_model_label(entry),
        session_history_resumability_label(entry),
    );

    Line::from(Span::styled(
        truncate_plain_text(&content, usize::from(width)),
        row_style,
    ))
}

fn session_history_action_prefix(
    app: &AppState,
    entry: &crate::app::SessionHistoryEntry,
) -> String {
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => "↺ replay ".to_string(),
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                "▶ resume ".to_string()
            } else {
                "! blocked ".to_string()
            }
        }
    }
}

fn session_history_action_color(
    app: &AppState,
    entry: &crate::app::SessionHistoryEntry,
    theme: &Theme,
) -> Color {
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => theme.status.info,
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                theme.status.success
            } else {
                theme.status.warning
            }
        }
    }
}

fn session_history_status_color(entry: &crate::app::SessionHistoryEntry, theme: &Theme) -> Color {
    match entry.catalog.status {
        Some(harness_core::proj::RunStatus::Running) => theme.status.info,
        Some(harness_core::proj::RunStatus::Finished) => theme.status.success,
        Some(harness_core::proj::RunStatus::Failed) => theme.status.error,
        None => theme.text.secondary,
    }
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
    if area.width == 0 || area.height == 0 {
        return;
    }

    let drawer_chunks = details_drawer_areas(area);

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.panel)),
        area,
    );
    render_details_orchestration_card(frame, app, drawer_chunks[0], theme);
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
    Style::default().fg(theme.text.secondary)
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

fn tool_status_badge(status: ToolCallDisplayStatus, theme: &Theme) -> Span<'static> {
    let (_, color, label, _) = tool_status_tokens(status, theme);
    status_badge(label, color, theme)
}

fn tool_detail_label_style(label: &str, theme: &Theme, status: ToolCallDisplayStatus) -> Style {
    let color = match label {
        "state" => match status {
            ToolCallDisplayStatus::PendingPermission => theme.status.warning,
            ToolCallDisplayStatus::Queued => theme.text.secondary,
            ToolCallDisplayStatus::Running => theme.text.accent,
            ToolCallDisplayStatus::Succeeded => theme.status.success,
            ToolCallDisplayStatus::Failed => theme.status.error,
        },
        "result" => theme.status.success,
        "error" => theme.status.error,
        _ => theme.text.secondary,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn append_tool_card_row(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    label: &str,
    value: &str,
    label_style: Style,
    value_style: Style,
    theme: &Theme,
) {
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.push(Span::styled(format!("{label:<6}"), label_style));
    if !value.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(value.to_string(), value_style));
    }
    lines.push(Line::from(spans));
}

fn tool_state_summary(tool_call: &crate::app::ToolCallEntry) -> Option<&'static str> {
    match tool_call.status {
        ToolCallDisplayStatus::PendingPermission => Some("awaiting approval before execution"),
        ToolCallDisplayStatus::Queued => Some("waiting for execution"),
        ToolCallDisplayStatus::Running => Some("running…"),
        ToolCallDisplayStatus::Succeeded if tool_call.truncated_output.is_none() => {
            Some("completed without output")
        }
        ToolCallDisplayStatus::Failed if tool_call.truncated_output.is_none() => {
            Some("failed without error payload")
        }
        _ => None,
    }
}

fn tool_footer_summary(tool_call: &crate::app::ToolCallEntry) -> String {
    let mut parts = vec![format!("call {}", tool_call.tool_call_id)];
    if !tool_call.permissions.is_empty() {
        let count = tool_call.permissions.len();
        parts.push(format!(
            "{count} permission{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    parts.join(" · ")
}

fn tool_status_summary(app: &AppState) -> Option<(String, Color)> {
    let activity = app.activities.get(app.selected_activity_index)?;
    let tool_calls = &activity.tool_calls;
    if tool_calls.is_empty() {
        return None;
    }

    if tool_calls.len() == 1 {
        let tool_call = &tool_calls[0];
        let (_, color, label, _) = tool_status_tokens(tool_call.status, app.theme());
        return Some((format!("tool {} {label}", tool_call.tool_id), color));
    }

    let mut pending = 0usize;
    let mut queued = 0usize;
    let mut running = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for tool_call in tool_calls {
        match tool_call.status {
            ToolCallDisplayStatus::PendingPermission => pending += 1,
            ToolCallDisplayStatus::Queued => queued += 1,
            ToolCallDisplayStatus::Running => running += 1,
            ToolCallDisplayStatus::Succeeded => succeeded += 1,
            ToolCallDisplayStatus::Failed => failed += 1,
        }
    }

    let mut segments = vec!["tools".to_string()];
    if running > 0 {
        segments.push(format!("{running} running"));
    }
    if pending > 0 {
        segments.push(format!("{pending} approval"));
    }
    if queued > 0 {
        segments.push(format!("{queued} queued"));
    }
    if failed > 0 {
        segments.push(format!("{failed} failed"));
    }
    if succeeded > 0 {
        segments.push(format!("{succeeded} done"));
    }

    let color = if failed > 0 {
        app.theme().status.error
    } else if pending > 0 {
        app.theme().status.warning
    } else if running > 0 {
        app.theme().text.accent
    } else if queued > 0 {
        app.theme().text.secondary
    } else {
        app.theme().status.success
    };

    Some((segments.join(" · "), color))
}

fn compact_inline_payload(payload: &str, max_chars: usize) -> Option<String> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }

    let collapsed = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => compact_inline_json_value(&value),
        Err(_) => trimmed.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    if collapsed.chars().count() <= max_chars {
        return Some(collapsed);
    }

    let truncated = collapsed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    Some(format!("{truncated}…"))
}

fn compact_inline_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }

            let mut parts = Vec::new();
            for (key, value) in map.iter().take(4) {
                parts.push(format!("{key}={}", compact_inline_json_leaf(value)));
            }
            if map.len() > 4 {
                parts.push("…".to_string());
            }
            parts.join(", ")
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }

            let mut parts = items
                .iter()
                .take(4)
                .map(compact_inline_json_leaf)
                .collect::<Vec<_>>();
            if items.len() > 4 {
                parts.push("…".to_string());
            }
            format!("[{}]", parts.join(", "))
        }
        _ => compact_inline_json_leaf(value),
    }
}

fn compact_inline_json_leaf(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.split_whitespace().collect::<Vec<_>>().join(" "),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(items) => format!(
            "[{} item{}]",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        serde_json::Value::Object(fields) => format!(
            "{{{} field{}}}",
            fields.len(),
            if fields.len() == 1 { "" } else { "s" }
        ),
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
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(
            format!(" {} ", state.kind.label()),
            Style::default()
                .fg(theme.text.inverse)
                .bg(runtime_state_color(state.kind, theme))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", base_style),
        Span::styled(state.summary, base_style),
    ];

    if !app.replay_mode {
        append_orchestration_status(&mut spans, app, area.width, base_style, theme);
    }

    if let Some((tool_summary, tool_color)) = tool_status_summary(app) {
        let separator = "  ·  ";
        let available = usize::from(area.width)
            .saturating_sub(status_strip_width(&spans))
            .saturating_sub(separator.chars().count());
        if available > 10 {
            spans.push(Span::styled(separator, base_style));
            spans.push(Span::styled(
                truncate_plain_text(&tool_summary, available),
                Style::default()
                    .fg(tool_color)
                    .bg(theme.surface.shell)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }

    let status_line = Line::from(spans);

    frame.render_widget(Paragraph::new(status_line).style(base_style), area);
}

fn append_orchestration_status(
    spans: &mut Vec<Span<'static>>,
    app: &AppState,
    width: u16,
    base_style: Style,
    theme: &Theme,
) {
    let summary = app.orchestration_summary();
    let latest_warning = app.orchestration_latest_warning();
    let count_segments = [
        format!("  ·  agents {}", summary.active_agents),
        format!(" · queued {}", summary.queued),
        format!(" · running {}", summary.running),
        format!(" · stale {}", summary.stale),
    ];

    let mut appended_all_counts = true;
    for segment in count_segments {
        if !append_status_segment_if_fits(spans, width, segment, base_style) {
            appended_all_counts = false;
            break;
        }
    }

    if !appended_all_counts {
        return;
    }

    let Some(latest_warning) = latest_warning else {
        return;
    };

    let available = usize::from(width).saturating_sub(status_strip_width(spans));
    let warning_prefix_width = " · warn ".chars().count();
    if available <= warning_prefix_width {
        return;
    }

    let warning_style = Style::default()
        .fg(theme.status.warning)
        .bg(theme.surface.shell)
        .add_modifier(Modifier::BOLD);
    let warning_segment = truncate_plain_text(&format!(" · warn {latest_warning}"), available);
    spans.push(Span::styled(warning_segment, warning_style));
}

fn append_status_segment_if_fits(
    spans: &mut Vec<Span<'static>>,
    width: u16,
    segment: String,
    style: Style,
) -> bool {
    let available = usize::from(width).saturating_sub(status_strip_width(spans));
    if segment.chars().count() > available {
        return false;
    }

    spans.push(Span::styled(segment, style));
    true
}

fn status_strip_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
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

fn render_details_orchestration_card(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);
    let surface = if is_focused {
        theme.surface.overlay
    } else {
        theme.surface.panel_elevated
    };
    let [title_area, body_area] = details_section_areas(area);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    render_details_section_title(
        frame,
        title_area,
        theme,
        surface,
        "Orchestration",
        None,
        is_focused,
    );

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    let rows = app.orchestration_visible_rows();
    let visible_rows =
        orchestration_card_lines(app, &rows, theme, body_area.height, body_area.width);

    frame.render_widget(
        Paragraph::new(Text::from(visible_rows)).style(panel_style(surface, theme.text.primary)),
        body_area,
    );
}

fn orchestration_card_lines(
    app: &AppState,
    rows: &[OrchestrationTaskRow],
    theme: &Theme,
    height: u16,
    width: u16,
) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(orchestration_summary_line(app, theme, width));

    if height == 1 {
        return lines;
    }

    lines.push(orchestration_warning_line(app, theme, width));
    let task_slots = usize::from(height.saturating_sub(2));
    if task_slots == 0 || rows.is_empty() {
        return lines;
    }

    if rows.len() <= task_slots {
        lines.extend(
            rows.iter()
                .map(|row| orchestration_task_line(app, row, theme, width)),
        );
        return lines;
    }

    if task_slots == 1 {
        lines.push(orchestration_overflow_line(rows.len(), theme));
        return lines;
    }

    let visible_task_count = task_slots.saturating_sub(1);
    lines.extend(
        rows.iter()
            .take(visible_task_count)
            .map(|row| orchestration_task_line(app, row, theme, width)),
    );
    lines.push(orchestration_overflow_line(
        rows.len().saturating_sub(visible_task_count),
        theme,
    ));
    lines
}

#[cfg(test)]
pub(crate) fn orchestration_card_text_for_test(
    app: &AppState,
    height: u16,
    width: u16,
) -> Vec<String> {
    orchestration_card_lines(
        app,
        &app.orchestration_visible_rows(),
        app.theme(),
        height,
        width,
    )
    .into_iter()
    .map(|line| {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    })
    .collect()
}

fn orchestration_summary_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let summary = app.orchestration_summary();
    let text = format!(
        "agents {} · queued {} · running {} · stale {}",
        summary.active_agents, summary.queued, summary.running, summary.stale
    );
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        muted_meta_style(theme),
    ))
}

fn orchestration_warning_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let warning = app.orchestration_latest_warning().unwrap_or("none");
    let text = format!("warn: {warning}");
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        Style::default().fg(theme.status.warning),
    ))
}

fn orchestration_task_line(
    app: &AppState,
    row: &OrchestrationTaskRow,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let (state_label, state_color) = orchestration_state_tokens(row.state, theme);
    let owner = app.orchestration_owner_labels(row);
    let queue_key = row.queue_key.as_deref().unwrap_or("queue:none");
    let detail = format!(
        "{} · {}/{} · {}",
        row.task_id, owner.label, owner.profile, queue_key
    );

    let badge_width = state_label.chars().count().saturating_add(4);
    let detail = truncate_plain_text(&detail, usize::from(width).saturating_sub(badge_width));

    Line::from(vec![
        status_badge(state_label, state_color, theme),
        Span::raw(" "),
        Span::styled(detail, muted_meta_style(theme)),
    ])
}

fn orchestration_overflow_line(hidden_count: usize, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("+{hidden_count} more"),
        Style::default()
            .fg(theme.text.tertiary)
            .add_modifier(Modifier::BOLD),
    ))
}

fn orchestration_state_tokens(
    state: OrchestrationTaskState,
    theme: &Theme,
) -> (&'static str, Color) {
    match state {
        OrchestrationTaskState::Queued => ("queued", theme.text.secondary),
        OrchestrationTaskState::Running => ("running", theme.status.info),
        OrchestrationTaskState::Stale => ("stale", theme.status.warning),
        OrchestrationTaskState::Completed => ("completed", theme.status.success),
        OrchestrationTaskState::Cancelled => ("cancelled", theme.status.error),
        OrchestrationTaskState::LateResult => ("late-result", theme.status.warning),
    }
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
    startup_shell_visible(app)
}

fn startup_shell_visible(app: &AppState) -> bool {
    !app.replay_mode
        && app.activities.is_empty()
        && app.transcript_pending_permissions().is_empty()
        && app.prompt_buffer.is_empty()
}

fn startup_mode_label(app: &AppState) -> Option<&str> {
    if !app.startup_mode {
        return None;
    }

    let mode = app.launch_mode_label()?.trim();
    if mode.eq_ignore_ascii_case("demo") {
        Some("Demo")
    } else if mode.eq_ignore_ascii_case("mock") {
        Some("Mock")
    } else {
        None
    }
}

fn startup_shell_metadata(app: &AppState) -> String {
    let mut segments = vec![
        format!("Preset {}", app.active_profile()),
        format!("{}/{}", app.active_provider(), app.current_model_label()),
    ];
    if let Some(mode) = startup_mode_label(app) {
        segments.push(mode.to_string());
    }
    segments.join(" · ")
}

fn render_live_empty_state(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

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

    let shell_area = startup_shell_area(area, theme);
    let surface = theme.surface.panel;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border.strong))
        .style(Style::default().bg(surface));
    let content_area = block.inner(shell_area);

    frame.render_widget(block, shell_area);

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
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
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(startup_shell_metadata(app))
            .style(Style::default().fg(theme.text.tertiary).bg(surface))
            .alignment(Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new("").style(Style::default().fg(theme.text.tertiary).bg(surface)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(theme.live_shell.empty_state.value_prop)
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(help_row)
            .style(Style::default().fg(theme.text.secondary).bg(surface))
            .alignment(Alignment::Center),
        rows[4],
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
    let (status_icon, status_color, _status_label, _) = tool_status_tokens(tool_call.status, theme);

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
        Span::raw(" "),
        tool_status_badge(tool_call.status, theme),
    ]));

    if let Some(state_summary) = tool_state_summary(tool_call) {
        append_tool_card_row(
            lines,
            &format!("  {} ", glyphs.card_mid),
            "state",
            state_summary,
            tool_detail_label_style("state", theme, tool_call.status),
            subdued_payload_style(theme),
            theme,
        );
    }

    if let Some(args_summary) = compact_inline_payload(&tool_call.args_summary, 96) {
        append_tool_card_row(
            lines,
            &format!("  {} ", glyphs.card_mid),
            "args",
            &args_summary,
            tool_detail_label_style("args", theme, tool_call.status),
            subdued_payload_style(theme),
            theme,
        );
    }

    if let Some(output) = tool_call
        .truncated_output
        .as_deref()
        .and_then(|output| compact_inline_payload(output, 96))
    {
        let label = if tool_call.status == ToolCallDisplayStatus::Failed {
            "error"
        } else {
            "result"
        };
        let output_style = if tool_call.status == ToolCallDisplayStatus::Failed {
            Style::default().fg(theme.status.error)
        } else {
            subdued_payload_style(theme)
        };
        append_tool_card_row(
            lines,
            &format!("  {} ", glyphs.card_mid),
            label,
            &output,
            tool_detail_label_style(label, theme, tool_call.status),
            output_style,
            theme,
        );
    }

    let footer = tool_footer_summary(tool_call);
    let status_line = vec![
        Span::styled(
            format!("  {} ", glyphs.card_bottom),
            transcript_prefix_style(theme),
        ),
        Span::styled(
            format!("{} ", status_icon),
            Style::default().fg(status_color),
        ),
        Span::styled(footer, Style::default().fg(theme.text.tertiary)),
    ];
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
    let surface = if is_focused {
        theme.surface.overlay
    } else {
        theme.surface.panel_elevated
    };
    let [title_area, body_area] = details_section_areas(area);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    render_details_section_title(
        frame, title_area, theme, surface, "Details", None, is_focused,
    );

    frame.render_widget(
        Paragraph::new(build_inspector_content(app, theme))
            .style(panel_style(surface, theme.text.primary))
            .scroll((app.details_scroll, 0))
            .wrap(Wrap { trim: true }),
        body_area,
    );
}

fn details_section_areas(area: Rect) -> [Rect; 2] {
    let inner = inset_rect(area, 1, 0);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    [chunks[0], chunks[1]]
}

fn render_details_section_title(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    surface: Color,
    title: &str,
    meta: Option<&str>,
    is_focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let indicator = if is_focused { "●" } else { "○" };
    let indicator_color = if is_focused {
        theme.text.accent
    } else {
        theme.text.tertiary
    };

    let mut spans = vec![
        Span::styled(
            format!("{indicator} "),
            Style::default().fg(indicator_color).bg(surface),
        ),
        Span::styled(
            title.to_string(),
            Style::default()
                .fg(if is_focused {
                    theme.text.primary
                } else {
                    theme.text.secondary
                })
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(meta) = meta {
        spans.push(Span::styled(
            format!(" · {meta}"),
            Style::default().fg(theme.text.tertiary).bg(surface),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
            Span::styled("tool ", Style::default().fg(theme.text.secondary)),
            Span::styled(
                tool_call.tool_id.clone(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            tool_status_badge(tool_call.status, theme),
        ]));
        append_labeled_value(
            lines,
            "  Call ID: ",
            tool_call.tool_call_id.clone(),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            "  State: ",
            tool_call.status.to_string(),
            status_color,
        );
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
            "  Args:",
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
                ("  Error:", theme.status.error)
            } else {
                ("  Result:", theme.text.primary)
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
    if app.replay_mode {
        let surface = theme.surface.panel_elevated;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.status.disabled))
            .style(Style::default().bg(surface))
            .title(Line::from(Span::styled(
                "Composer · replay read-only",
                Style::default().fg(theme.status.disabled),
            )));
        let paragraph = Paragraph::new(
            "Replay is read-only — prompt editing and submit are disabled. Inspect the transcript or press r to reload.",
        )
        .block(block)
        .style(panel_style(surface, theme.status.disabled))
        .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
        return;
    }

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

    let hint_text = if !app.replay_mode
        && app
            .launch_mode_label()
            .is_some_and(|label| label.eq_ignore_ascii_case("continued"))
    {
        format!("continued live run{separator}{hint_text}")
    } else {
        hint_text
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
        custom_theme.live_shell.primary.content_margin_x = 10;
        custom_theme.live_shell.primary.activity_drawer_width = 18;
        custom_theme.live_shell.primary.details_sidebar_width = 36;
        themed_app.set_theme(custom_theme);

        let themed_hit_areas = FrameLayoutPlan::for_app(&themed_app, area).wheel_hit_areas;
        assert_ne!(default_hit_areas.overlay, themed_hit_areas.overlay);
        assert_ne!(default_hit_areas.inspector, themed_hit_areas.inspector);

        let themed_inspector = themed_hit_areas.inspector.expect("themed inspector area");
        let probe_column = themed_inspector.x.saturating_add(2);
        let probe_row = themed_inspector.y.saturating_add(1);

        assert_eq!(
            hovered_wheel_target(&themed_app, area, probe_column, probe_row),
            Some(WheelTarget::Inspector)
        );
        assert_ne!(
            hovered_wheel_target(&default_app, area, probe_column, probe_row),
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
        assert!(!debug.contains("Demo"));
        assert!(!debug.contains("run unknown"));
        assert!(debug.contains("Preset deep · proxy/gpt-5.4"));
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
        assert!(!mock_debug.contains("Preset worker · mock/model-1 · Mock"));
    }

    #[test]
    fn startup_shell_shows_profile_provider_and_model_chrome() {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Harness"));
        assert!(debug.contains("Preset deep · proxy/gpt-5.4 · Demo"));
        assert!(debug.contains("Start a conversation to begin"));
    }

    #[test]
    fn replay_prompt_pane_is_visibly_read_only() {
        let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), Vec::new());

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Composer · replay read-only"));
        assert!(debug.contains("Replay is read-only"));
        assert!(!debug.contains("Type a prompt for the next turn"));
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
            .find(|debug| debug.contains("Args:"))
            .expect("tool detail section should be reachable via scroll");
        assert!(tool_debug.contains("Tool calls:"));
        assert!(tool_debug.contains("Args:"));
        assert!(tool_debug.contains("State: succeeded"));
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
            .find(|debug| debug.contains("Result:"))
            .expect("tool output section should be reachable via scroll");
        assert!(output_debug.contains("Result:"));
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
        assert!(transcript.contains("args   limit=20, path=src/lib.rs, start_line=42"));
        assert!(transcript.contains("result 12 lines read"));
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
        assert!(transcript.contains("args   cmd=false, cwd=/tmp/demo"));
        assert!(transcript.contains("error  exit code: 1 stderr: permission denied"));
        assert!(transcript.contains("exit code: 1 stderr: permission denied"));
        assert!(transcript.contains("failed"));
        assert!(!transcript.contains(r#"{"cmd":"false","cwd":"/tmp/demo"}"#));
        assert!(!transcript.contains("args {"));
    }

    #[test]
    fn status_strip_surfaces_selected_tool_summary() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_status",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_status".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Check tool status".to_string(),
                request_digest: "digest-tool-status".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_status",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_status".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false"}"#.to_string(),
                args_digest: "digest-tool-status-args".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_status",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_status".to_string(),
            }),
        ));

        let debug = render_debug(&app, 160, 30);
        assert!(debug.contains("tool shell.run running"));
    }
}
