//! TUI rendering module for Agent Harness.
//!
//! Implements a multi-tab interface with:
//! - Run workspace: 3-pane layout (Activity, Transcript, Inspector) + Prompt input
//! - Events tab: Event list with details
//! - Diff tab: Diff viewer for applied edits
//! - Help tab: Keyboard shortcuts

use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::app::{ActivityEntry, ActivityStatus, AppState, Focus, Tab, UiIntent};
use crate::theme::Theme;

/// Main render entry point
pub fn render_app(frame: &mut Frame, app: &AppState) {
    let theme = Theme::default();
    let area = frame.area();

    // Main vertical layout: header, content, footer
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(0),    // Content (tabs + main area)
            Constraint::Length(1), // Footer
        ])
        .split(area);

    render_header(frame, app, main_chunks[0], &theme);
    render_content(frame, app, main_chunks[1], &theme);
    render_footer(frame, app, main_chunks[2], &theme);

    // Render permission modal on top if active
    if let Some((permission_id, summary)) = app.active_permission() {
        render_permission_modal(frame, &permission_id, &summary, &theme);
    }
}

/// Render the header bar with run info
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
        format!(
            "REPLAY: {} (run={}, r to reload)",
            app.session_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            run_id
        )
    } else {
        format!(
            "Agent Harness | run={} | profile={} | provider={} | model={}",
            run_id, profile, provider, model
        )
    };

    let header =
        Paragraph::new(header_text).style(Style::default().fg(theme.header_fg).bg(theme.header_bg));
    frame.render_widget(header, area);
}

/// Render the content area (tabs + main content)
fn render_content(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_tabs(frame, app, chunks[0], theme);

    let content_area = chunks[1];
    match app.active_tab {
        Tab::Run => render_run_workspace(frame, app, content_area, theme),
        Tab::Events => render_events_tab(frame, app, content_area, theme),
        Tab::Diff => render_diff_tab(frame, app, content_area, theme),
        Tab::Help => render_help_tab(frame, content_area, theme),
    }
}

/// Render the tab bar
fn render_tabs(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let titles: Vec<Line> = ["Run", "Events", "Diff", "Help"]
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let style = if i == app.active_tab as usize {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            Line::from(Span::styled(*title, style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Tabs"))
        .select(app.active_tab as usize)
        .style(Style::default().fg(theme.border))
        .highlight_style(Style::default().fg(theme.accent));

    frame.render_widget(tabs, area);
}

/// Render the Run workspace with 3-pane layout + prompt
fn render_run_workspace(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    // Split into main area and prompt area
    // Prompt height: 3 rows default
    let prompt_height = 3u16;
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(prompt_height + 2)]) // +2 for borders
        .split(area);

    // Main panes: horizontal split into Activity | Transcript | Inspector
    // Layout percentages: 25% | 50% | 25%
    let pane_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Activity
            Constraint::Percentage(50), // Transcript
            Constraint::Percentage(25), // Inspector
        ])
        .split(main_chunks[0]);

    render_activity_pane(frame, app, pane_chunks[0], theme);
    render_transcript_pane(frame, app, pane_chunks[1], theme);
    render_inspector_pane(frame, app, pane_chunks[2], theme);
    render_prompt_pane(frame, app, main_chunks[1], theme);
}

/// Render the Activity pane (left)
fn render_activity_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && app.active_tab == Tab::Run;

    let border_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let title = format!(
        "Activity (j/k active{}{})",
        if app.follow_mode { ", follow" } else { "" },
        if is_focused { ", focused" } else { "" }
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.title));

    if app.activities.is_empty() {
        let empty = Paragraph::new("No activities yet")
            .block(block)
            .style(Style::default().fg(theme.fg));
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
                ActivityStatus::Streaming => "◐",
                ActivityStatus::Done => "●",
                ActivityStatus::Error => "✗",
            };
            let status_text = match activity.status {
                ActivityStatus::Streaming => "streaming…",
                ActivityStatus::Done => "done",
                ActivityStatus::Error => "error",
            };

            let style = if is_selected {
                Style::default()
                    .fg(theme.selected_fg)
                    .bg(theme.selected_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            let model_display = if activity.model_id.is_empty() {
                "-"
            } else {
                &activity.model_id
            };

            let content = format!(
                "{}{} {} {} {}",
                prefix, activity.request_id, model_display, status_text, status_icon
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
    let is_focused = app.focus == Focus::Details && app.active_tab == Tab::Run;

    let border_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let title = format!(
        "Transcript{}{}",
        if is_focused { " (focused)" } else { "" },
        if app.follow_mode { " (following)" } else { "" }
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.title));

    let content = if let Some(activity) = app.activities.get(app.selected_activity_index) {
        let mut lines = Vec::new();

        if let Some(user_msg) = &activity.user_message {
            lines.push(Line::from(vec![
                Span::styled(
                    "User: ",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(&user_msg.text, Style::default().fg(theme.fg)),
            ]));
            lines.push(Line::from(""));
        }

        if activity.status == ActivityStatus::Error {
            if let Some(error) = &activity.error_message {
                lines.push(Line::from(vec![
                    Span::styled("[provider error] ", Style::default().fg(theme.error)),
                    Span::styled(error, Style::default().fg(theme.error)),
                ]));
            }
        } else if !activity.transcript_text.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "Assistant: ",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(activity.transcript_text.as_str()));
        } else if activity.status == ActivityStatus::Streaming {
            lines.push(Line::from("Waiting for response..."));
        } else {
            lines.push(Line::from("No content"));
        }

        Text::from(lines)
    } else {
        Text::from("Select an activity to view transcript")
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(theme.fg))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render the Inspector pane (right)
fn render_inspector_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details && app.active_tab == Tab::Run;

    let border_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let title = format!("Inspector{}", if is_focused { " (focused)" } else { "" });

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.title));

    let content = if let Some(activity) = app.activities.get(app.selected_activity_index) {
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(
                "Request ID: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(&activity.request_id, Style::default().fg(theme.fg)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Provider: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&activity.provider_id, Style::default().fg(theme.fg)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Model: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&activity.model_id, Style::default().fg(theme.fg)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{}", activity.status),
                match activity.status {
                    crate::app::ActivityStatus::Error => Style::default().fg(theme.error),
                    crate::app::ActivityStatus::Done => Style::default().fg(theme.success),
                    _ => Style::default().fg(theme.fg),
                },
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Sequences: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{}-{}", activity.first_seq, activity.last_seq),
                Style::default().fg(theme.fg),
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
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                error,
                Style::default().fg(theme.error),
            )));
        }

        Text::from(lines)
    } else {
        Text::from("No activity selected")
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(theme.fg))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render the Prompt input pane (bottom)
fn render_prompt_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Prompt;

    let border_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let char_count = app.prompt_buffer.chars().count();
    let title = format!(
        "Prompt ({} chars){}",
        char_count,
        if is_focused { " [focused]" } else { "" }
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.title));

    // Create text with cursor indicator
    let mut text = app.prompt_buffer.clone();
    if is_focused {
        // Add cursor indicator (█) at cursor position
        let cursor_byte_pos = app
            .prompt_buffer
            .char_indices()
            .nth(app.prompt_cursor)
            .map(|(i, _)| i)
            .unwrap_or(app.prompt_buffer.len());
        text.insert(cursor_byte_pos, '█');
    }

    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(theme.fg))
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

    let border_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let follow_indicator = if app.follow_mode { ", follow" } else { "" };
    let title = format!("Events (j/k active{})", follow_indicator);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.title));

    if app.events.is_empty() {
        let empty = Paragraph::new("No events")
            .block(block)
            .style(Style::default().fg(theme.fg));
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
                    .fg(theme.selected_fg)
                    .bg(theme.selected_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
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

    let border_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let title = if is_focused {
        "Event details (Tab to focus)"
    } else {
        "Event details (Tab to focus)"
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title)
        .title_style(Style::default().fg(theme.title));

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
        .style(Style::default().fg(theme.fg))
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
    let border_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.border)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title("Diff (Tab to focus)")
        .title_style(Style::default().fg(theme.title));

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
        .style(Style::default().fg(theme.fg))
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
        .title_style(Style::default().fg(theme.title));

    let help_text = r#"Keyboard Shortcuts:

Navigation:
  j/↓          Move down in list
  k/↑          Move up in list
  Tab          Cycle focus forward (List → Details → Prompt)
  Shift+Tab    Cycle focus backward
  Space        Toggle follow mode

Tabs:
  1            Switch to Run tab
  2            Switch to Events tab
  3            Switch to Diff tab
  4            Switch to Help tab
  h            Show Help

Prompt (when focused):
  Enter        Submit prompt
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
        .style(Style::default().fg(theme.fg))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

/// Render the footer with status and key hints
fn render_footer(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let mut parts = Vec::new();

    if app.events_trimmed_count > 0 {
        parts.push(format!("[trimmed {} old events]", app.events_trimmed_count));
    }

    if app.transcript_trimmed_count > 0 {
        parts.push(format!(
            "[trimmed {} transcript chars]",
            app.transcript_trimmed_count
        ));
    }

    let status_text = if let Some(banner) = &app.status_banner {
        banner.clone()
    } else if !parts.is_empty() {
        parts.join(" ")
    } else {
        format!(
            "Tab focus | Ctrl+P palette | q quit | {} events",
            app.events.len()
        )
    };

    let footer =
        Paragraph::new(status_text).style(Style::default().fg(theme.footer_fg).bg(theme.footer_bg));

    frame.render_widget(footer, area);
}

/// Render the permission modal
fn render_permission_modal(frame: &mut Frame, permission_id: &str, summary: &str, theme: &Theme) {
    let area = frame.area();
    let popup_width = 50.min(area.width.saturating_sub(4));
    let popup_height = 7.min(area.height.saturating_sub(4));

    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_rect = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the modal
    frame.render_widget(Clear, popup_rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.modal_border))
        .title("Permission Requested")
        .title_style(Style::default().fg(theme.title));

    let content = format!(
        "{}\n\n[a]llow  [d]eny  [esc]dismiss",
        summary
            .chars()
            .take(popup_width as usize - 4)
            .collect::<String>()
    );

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(theme.fg).bg(theme.modal_bg))
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
        assert!(matches!(theme.bg, ratatui::style::Color::Rgb(_, _, _)));
    }
}
