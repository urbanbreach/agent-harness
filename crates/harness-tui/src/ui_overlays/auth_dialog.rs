use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::auth_dialog::{auth_method_label, ConnectDialogStep, ConnectProviderOption};
use crate::app::AppState;
use crate::theme::Theme;

pub(super) fn render_auth_dialog_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let dialog = &app.connect_dialog;
    if !dialog.visible {
        return;
    }

    let backdrop = Block::default().style(Style::default().bg(theme.surface.overlay));
    frame.render_widget(backdrop, root);

    let width = 64u16.min(root.width.saturating_sub(8));
    let height = match dialog.step {
        ConnectDialogStep::SelectProvider => 6 + dialog.providers.len() as u16,
        ConnectDialogStep::SelectMethod => 10,
        ConnectDialogStep::ApiKeyInput | ConnectDialogStep::EnterpriseUrl => 10,
        ConnectDialogStep::Waiting => 8,
        ConnectDialogStep::SelectModel => 6 + dialog.models.len() as u16 + 1,
        ConnectDialogStep::Success | ConnectDialogStep::Error => 8,
    };
    let height = height.min(root.height.saturating_sub(6));
    let x = root.x + (root.width.saturating_sub(width)) / 2;
    let y = root.y + (root.height.saturating_sub(height)) / 2;
    let area = Rect::new(x, y, width, height);

    let title = match dialog.step {
        ConnectDialogStep::SelectProvider => " Connect a provider ",
        ConnectDialogStep::SelectMethod => " Select auth method ",
        ConnectDialogStep::ApiKeyInput => " Enter API key ",
        ConnectDialogStep::EnterpriseUrl => " Enter details ",
        ConnectDialogStep::Waiting => " Connecting... ",
        ConnectDialogStep::SelectModel => " Select model ",
        ConnectDialogStep::Success => " Connected ",
        ConnectDialogStep::Error => " Connection failed ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(theme.border.strong))
        .style(Style::default().bg(theme.surface.panel_elevated));
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    match dialog.step {
        ConnectDialogStep::SelectProvider => {
            let filtered = dialog.refilter_providers();
            render_provider_list(frame, &filtered, dialog.selected, theme, inner);
        }
        ConnectDialogStep::SelectMethod => {
            let methods = dialog
                .selected_provider
                .and_then(|i| dialog.providers.get(i))
                .map(|p| p.methods.as_slice())
                .unwrap_or(&[]);
            render_method_list(frame, methods, dialog.selected, theme, inner);
        }
        ConnectDialogStep::ApiKeyInput => {
            let label = dialog
                .selected_provider
                .and_then(|i| dialog.providers.get(i))
                .map(|p| p.label.as_str())
                .unwrap_or("API key");
            render_input_field(frame, theme, inner, label, &dialog.input_buffer, true);
        }
        ConnectDialogStep::EnterpriseUrl => {
            render_input_field(
                frame,
                theme,
                inner,
                "Enterprise URL (optional - press Enter to skip)",
                &dialog.input_buffer,
                false,
            );
        }
        ConnectDialogStep::Waiting => {
            let provider_label = dialog
                .selected_provider
                .and_then(|i| dialog.providers.get(i))
                .map(|p| p.label.as_str())
                .unwrap_or("provider");
            let method_label = dialog
                .selected_provider
                .and_then(|i| dialog.providers.get(i))
                .and_then(|p| dialog.selected_method.map(|m| &p.methods[m]))
                .map(auth_method_label)
                .unwrap_or("authenticating");
            let lines = vec![
                Line::from(Span::styled(
                    format!("Waiting for {provider_label} authorization..."),
                    Style::default().fg(theme.text.primary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    method_label,
                    Style::default().fg(theme.text.secondary),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "esc cancel",
                    Style::default().fg(theme.text.tertiary),
                )),
            ];
            frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner);
        }
        ConnectDialogStep::SelectModel => {
            render_model_list(frame, &dialog.models, dialog.selected, theme, inner);
        }
        ConnectDialogStep::Success => {
            let toast_msg = dialog
                .toast
                .as_ref()
                .map(|t| t.message.as_str())
                .unwrap_or("Provider connected successfully.");
            let model_info = dialog
                .selected_model
                .and_then(|i| dialog.models.get(i))
                .map(|m| format!("  model: {m}"))
                .unwrap_or_default();
            let lines = vec![
                Line::from(Span::styled(
                    toast_msg,
                    Style::default()
                        .fg(theme.status.success)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(model_info),
                Line::from(""),
                Line::from(Span::styled(
                    "c copy  ·  any key close",
                    Style::default().fg(theme.text.tertiary),
                )),
            ];
            frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        }
        ConnectDialogStep::Error => {
            let error = dialog
                .toast
                .as_ref()
                .map(|t| t.message.as_str())
                .unwrap_or_else(|| {
                    dialog
                        .error_message
                        .as_deref()
                        .unwrap_or("Authentication failed.")
                });
            let lines = vec![
                Line::from(Span::styled(
                    error,
                    Style::default()
                        .fg(theme.status.error)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "c copy  ·  any key close",
                    Style::default().fg(theme.text.tertiary),
                )),
            ];
            frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        }
    }
}

fn render_provider_list(
    frame: &mut Frame,
    providers: &[&ConnectProviderOption],
    selected: usize,
    theme: &Theme,
    area: Rect,
) {
    let mut lines: Vec<Line> = Vec::new();
    for (index, provider) in providers.iter().enumerate() {
        let is_selected = index == selected;
        let marker = if is_selected { "●" } else { "○" };
        let mut spans = vec![Span::styled(
            marker,
            Style::default().fg(if is_selected {
                theme.text.accent
            } else {
                theme.text.tertiary
            }),
        )];
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            &provider.label,
            Style::default().fg(if is_selected {
                theme.text.primary
            } else {
                theme.text.secondary
            }),
        ));
        if !provider.description.is_empty() {
            spans.push(Span::styled(
                format!("  {}", provider.description),
                Style::default().fg(theme.text.tertiary),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓/jk select  ·  enter confirm  ·  esc cancel",
        Style::default().fg(theme.text.tertiary),
    )));

    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), area);
}

fn render_method_list(
    frame: &mut Frame,
    methods: &[harness_core::auth::plugin::AuthMethodSpec],
    selected: usize,
    theme: &Theme,
    area: Rect,
) {
    let mut lines: Vec<Line> = Vec::new();
    for (index, method) in methods.iter().enumerate() {
        let is_selected = index == selected;
        let marker = if is_selected { "●" } else { "○" };
        let mut spans = vec![Span::styled(
            marker,
            Style::default().fg(if is_selected {
                theme.text.accent
            } else {
                theme.text.tertiary
            }),
        )];
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            auth_method_label(method),
            Style::default().fg(if is_selected {
                theme.text.primary
            } else {
                theme.text.secondary
            }),
        ));
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓/jk select  ·  enter confirm  ·  esc cancel",
        Style::default().fg(theme.text.tertiary),
    )));

    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), area);
}

fn render_model_list(
    frame: &mut Frame,
    models: &[String],
    selected: usize,
    theme: &Theme,
    area: Rect,
) {
    let mut lines: Vec<Line> = Vec::new();
    for (index, model) in models.iter().enumerate() {
        let is_selected = index == selected;
        let marker = if is_selected { "●" } else { "○" };
        let mut spans = vec![Span::styled(
            marker,
            Style::default().fg(if is_selected {
                theme.text.accent
            } else {
                theme.text.tertiary
            }),
        )];
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            model,
            Style::default().fg(if is_selected {
                theme.text.primary
            } else {
                theme.text.secondary
            }),
        ));
        lines.push(Line::from(spans));
    }

    let skip_index = models.len();
    let skip_selected = selected == skip_index;
    let skip_marker = if skip_selected { "●" } else { "○" };
    let mut skip_spans = vec![Span::styled(
        skip_marker,
        Style::default().fg(if skip_selected {
            theme.text.accent
        } else {
            theme.text.tertiary
        }),
    )];
    skip_spans.push(Span::raw(" "));
    skip_spans.push(Span::styled(
        "Skip model selection",
        Style::default().fg(if skip_selected {
            theme.text.primary
        } else {
            theme.text.secondary
        }),
    ));
    lines.push(Line::from(skip_spans));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑↓/jk select  ·  enter confirm  ·  esc skip",
        Style::default().fg(theme.text.tertiary),
    )));

    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), area);
}

fn render_input_field(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    label: &str,
    buffer: &str,
    _secret: bool,
) {
    let display_value = if _secret {
        "•".repeat(buffer.chars().count())
    } else {
        buffer.to_string()
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        label,
        Style::default().fg(theme.text.secondary),
    )));
    lines.push(Line::from(""));
    if display_value.is_empty() {
        lines.push(Line::from(Span::styled(
            "Type here...",
            Style::default().fg(theme.text.tertiary),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            display_value,
            Style::default().fg(theme.text.primary),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "enter confirm  ·  esc back",
        Style::default().fg(theme.text.tertiary),
    )));

    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), area);
}
