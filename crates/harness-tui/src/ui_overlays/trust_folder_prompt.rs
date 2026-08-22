//! Trust folder prompt overlay rendering.
//!
//! Renders a centered dialog prompting the operator to trust or deny
//! repository-local executable execution for the current workspace.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::AppState;
use crate::theme::Theme;

pub fn render_trust_folder_prompt_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    if !app.trust_folder_prompt_visible {
        return;
    }

    render_overlay_dim_backdrop(frame, root);

    let width = 56.min(root.width.saturating_sub(4));
    let height = 9.min(root.height.saturating_sub(4));
    let x = root.x + (root.width.saturating_sub(width)) / 2;
    let y = root.y + (root.height.saturating_sub(height)) / 2;
    let area = Rect::new(x, y, width, height);

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    let surface = theme.surface.overlay;
    let accent = theme.text.accent;
    let muted = theme.text.secondary;
    let text = theme.text.primary;
    let border = Style::default()
        .fg(theme.terminal_colors.muted)
        .bg(surface);

    let title = "Folder Trust";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::default().fg(accent).bg(surface)))
        .border_style(border);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let body = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            "Repository-local executables require folder trust.",
            Style::default().fg(muted).bg(surface),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Allow: permit repo-local binary execution",
            Style::default().fg(text).bg(surface),
        )]),
        Line::from(vec![Span::styled(
            "Deny:  block repo-local binary execution",
            Style::default().fg(muted).bg(surface),
        )]),
    ])
    .alignment(Alignment::Left);
    frame.render_widget(body, chunks[1]);

    let footer = Paragraph::new(vec![Line::from(vec![Span::styled(
        "[Esc] Cancel",
        Style::default().fg(muted).bg(surface),
    )])])
    .alignment(Alignment::Center);
    frame.render_widget(footer, chunks[2]);
}

fn render_overlay_dim_backdrop(frame: &mut Frame, root: Rect) {
    let backdrop = Block::default().style(Style::default().dim());
    frame.render_widget(backdrop, root);
}
