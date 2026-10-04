use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::{app::AppState, keybindings::Action, theme::Theme};

#[path = "ui_control_dock_disclosure/hints.rs"]
mod hints;
#[path = "ui_control_dock_disclosure/summaries.rs"]
mod summaries;
use hints::{hint_candidates, preferred_binding, shortcut_row};

pub(super) fn render_control_dock_disclosure(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    let surface = theme.surface.canvas;
    let base = Style::default().bg(surface);
    frame.render_widget(Block::default().style(base), area);
    if app.tasks_pane.visible && app.tasks_pane.focused {
        super::ui_tasks_pane::render_footer(frame, app, area, theme);
        return;
    }
    if app.todo_pane_focused() {
        let key = base.fg(theme.text.primary).add_modifier(Modifier::BOLD);
        let label = base.fg(theme.text.secondary);
        let done = if app.todo_pane.hide_done {
            ":show done  │  "
        } else {
            ":hide done  │  "
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("h", key),
                Span::styled(done, label),
                Span::styled(preferred_binding(app, Action::Help, "Ctrl+x"), key),
                Span::styled(":shortcuts", label),
            ])),
            area,
        );
        return;
    }

    // Startup has no disclosure rectangle; replay uses its read-only composer.
    let active = !app.composer_disabled();
    let clear = app.clear_prompt_confirmation_pending();
    let turn = app.active_turn_in_progress();
    let context = app.current_request_budget_snapshot().is_some()
        || app.uses_unknown_budget_fallback()
        || app.active_context_usage().is_some()
        || app.compaction_usage_metrics().completed_count > 0;
    if active && !clear && app.starting_session_seed_visible() {
        frame.render_widget(
            Paragraph::new(hints::starting(app, theme)).style(base),
            area,
        );
        return;
    }

    let background = app.active_background_task_count();
    let foreground_shortcuts = !app.interrupt_hint_visible() || turn;
    let compact = area.width <= theme.live_shell.breakpoints.minimum.width;
    let max_width = usize::from(area.width);
    // Reuse the row across the background, foreground and fallback paths.
    let mut shortcuts = (active && !clear && (background > 0 && !turn || foreground_shortcuts))
        .then(|| shortcut_row(app, theme, compact, false));
    let shortcuts_fit = shortcuts
        .as_ref()
        .is_some_and(|row| width(row) <= max_width);
    if active && !clear && background > 0 && !turn {
        if shortcuts_fit {
            frame.render_widget(
                Paragraph::new(shortcuts.take().unwrap_or_default()).style(base),
                area,
            );
            return;
        }
        if !context {
            frame.render_widget(
                Paragraph::new(hints::background(app, theme, background)).style(base),
                area,
            );
            return;
        }
    }
    if active && !clear && foreground_shortcuts && (!context || turn) && shortcuts_fit {
        frame.render_widget(
            Paragraph::new(shortcuts.take().unwrap_or_default()).style(base),
            Rect {
                width: area.width.saturating_sub(2).max(1),
                ..area
            },
        );
        return;
    }

    let mut hints = if clear {
        vec![Line::from(vec![
            Span::styled(
                "Esc",
                base.fg(theme.terminal_colors.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                ":press again to clear",
                base.fg(theme.terminal_colors.secondary),
            ),
        ])]
    } else if active && foreground_shortcuts && shortcuts_fit {
        vec![shortcuts.take().unwrap_or_default()]
    } else if active && foreground_shortcuts && !app.composer.prompt_buffer.is_empty() {
        vec![shortcut_row(app, theme, compact, true)]
    } else {
        hint_candidates(app, theme)
    };
    let mut summaries = summaries::candidates(app, theme, active);
    if active && area.width < theme.live_shell.breakpoints.minimum.width {
        hints.retain(|line| {
            let text = line
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
                .to_ascii_lowercase();
            !text.contains("history") && !text.contains("quit")
        });
    }

    // Keep the first summary-major pair at the lowest weighted score.
    let selected = summaries
        .iter()
        .enumerate()
        .flat_map(|(s, summary)| {
            let summary_width = width(summary);
            hints.iter().enumerate().filter_map(move |(h, hint)| {
                let gap = usize::from(!summary.spans.is_empty() && !hint.spans.is_empty()) * 2;
                (summary_width
                    .saturating_add(width(hint))
                    .saturating_add(gap)
                    <= max_width)
                    .then_some((s, h))
            })
        })
        .min_by_key(|(s, h)| s + h * 2);
    if let Some((s, h)) = selected {
        let mut summary = summaries.swap_remove(s);
        let hint = hints.swap_remove(h);
        if active {
            if !summary.spans.is_empty() && !hint.spans.is_empty() {
                summary.spans.push(Span::styled("  ", base));
            }
            summary.spans.extend(hint.spans);
            frame.render_widget(Paragraph::new(summary).style(base), area);
        } else {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(u16::try_from(width(&hint)).unwrap_or(u16::MAX)),
                ])
                .split(area);
            if !summary.spans.is_empty() && columns[0].width > 0 {
                frame.render_widget(Paragraph::new(summary).style(base), columns[0]);
            }
            if !hint.spans.is_empty() && columns[1].width > 0 {
                frame.render_widget(
                    Paragraph::new(hint).style(base).alignment(Alignment::Right),
                    columns[1],
                );
            }
        }
    } else if let Some(line) = active
        .then(|| hints.into_iter().find(|line| width(line) <= max_width))
        .flatten()
        .or_else(|| summaries.into_iter().next())
    {
        frame.render_widget(Paragraph::new(line).style(base), area);
    }
}

fn width(line: &Line<'_>) -> usize {
    // Preserve legacy candidate priority for Unicode labels. Painting uses terminal cells.
    line.spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum()
}

pub(super) fn render_replay_read_only_composer_content(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: &Theme,
) {
    let surface = theme.surface.canvas;
    let runtime = app.runtime_state_view();
    let body = if runtime.kind == crate::app::RuntimeStateKind::Failure {
        match runtime
            .detail
            .as_deref()
            .filter(|detail| !detail.trim().is_empty())
        {
            Some(detail) => format!("Replay is read-only · {} · {detail}", runtime.summary),
            None => format!("Replay is read-only · {}", runtime.summary),
        }
    } else {
        "Replay is read-only.".to_string()
    };

    let content_area = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    if content_area.height == 0 {
        return;
    }

    let hint_visible = content_area.height > 1;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(content_area);

    let rail = "▎ ";
    let body = super::truncate_plain_text(
        &body,
        usize::from(rows[0].width).saturating_sub(rail.chars().count()),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(rail, Style::default().fg(theme.status.disabled).bg(surface)),
            Span::styled(body, Style::default().fg(theme.status.disabled).bg(surface)),
        ])),
        rows[0],
    );

    if hint_visible && rows[1].height > 0 {
        let hint_prefix = "  ";
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(hint_prefix, Style::default().bg(surface)),
                Span::styled(
                    super::truncate_plain_text(
                        &replay_read_only_shortcut_hints(app),
                        usize::from(rows[1].width).saturating_sub(hint_prefix.chars().count()),
                    ),
                    Style::default().fg(theme.text.secondary).bg(surface),
                ),
            ])),
            rows[1],
        );
    }
}

pub(super) fn replay_read_only_shortcut_hints(app: &AppState) -> String {
    [
        app.keymap
            .get_binding_label(Action::Help, "shortcuts")
            .to_ascii_lowercase(),
        app.keymap
            .get_binding_label(Action::FocusNext, "focus")
            .to_ascii_lowercase(),
        app.keymap
            .get_binding_label(Action::Quit, "quit")
            .to_ascii_lowercase(),
    ]
    .join("  ·  ")
}

pub(super) fn completed_session_status_summary(
    app: &AppState,
    state: &crate::app::RuntimeState,
) -> Option<String> {
    if !app.completed_session_shell_active() || app.replay_mode {
        return None;
    }

    Some(match state.kind {
        crate::app::RuntimeStateKind::Failure => {
            "run failed · inspect transcript · session shell preserved".to_string()
        }
        _ => "run finished · session shell preserved".to_string(),
    })
}
