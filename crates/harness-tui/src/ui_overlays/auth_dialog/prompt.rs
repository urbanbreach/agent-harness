use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::common::{
    horizontal_inset, render_panel, render_prompt_header, split_hint, PROMPT_HEADER_HEIGHT,
};
use super::prompt_panel::PromptPanel;

use crate::app::auth_dialog::auth_method_label;
use crate::app::AppState;
use crate::theme::Theme;

pub(super) fn render_custom_provider_prompt(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    PromptPanel {
        title: "Other",
        description: Some(
            "This only stores a credential. Configure the provider in harness.jsonc to use it.",
        ),
        placeholder: "Provider id",
        value: &app.connect_dialog.input_buffer,
        secret: false,
        error: app.connect_dialog.error_message.as_deref(),
        footer: "enter submit",
    }
    .render(frame, theme, root);
}

pub(super) fn render_api_key_prompt(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let dialog = &app.connect_dialog;
    let title = dialog
        .selected_provider
        .and_then(|provider_index| dialog.providers.get(provider_index))
        .and_then(|provider| {
            dialog
                .selected_method
                .and_then(|method_index| provider.methods.get(method_index))
        })
        .map(auth_method_label)
        .unwrap_or("API key");
    PromptPanel {
        title,
        description: None,
        placeholder: "API key",
        value: &dialog.input_buffer,
        secret: true,
        error: dialog.error_message.as_deref(),
        footer: "enter submit",
    }
    .render(frame, theme, root);
}

pub(super) fn render_enterprise_url_prompt(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    PromptPanel {
        title: "Enter details",
        description: Some("Enterprise URL (optional - press Enter to skip)"),
        placeholder: "Enterprise URL",
        value: &app.connect_dialog.input_buffer,
        secret: false,
        error: app.connect_dialog.error_message.as_deref(),
        footer: "enter submit",
    }
    .render(frame, theme, root);
}

pub(super) fn render_waiting_panel(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let dialog = &app.connect_dialog;
    let provider_label = dialog
        .selected_provider
        .and_then(|index| dialog.providers.get(index))
        .map(|provider| provider.label.as_str())
        .or_else(|| {
            dialog
                .custom_provider
                .as_ref()
                .map(|provider| provider.as_str())
        })
        .unwrap_or("provider");
    let method_label = dialog
        .selected_provider
        .and_then(|index| dialog.providers.get(index))
        .and_then(|provider| {
            dialog
                .selected_method
                .and_then(|method| provider.methods.get(method))
        })
        .map(auth_method_label)
        .unwrap_or("API key");
    let area = render_panel(frame, theme, root, 8);
    render_prompt_header(frame, theme, area, method_label);

    let body = horizontal_inset(
        Rect::new(
            area.x,
            area.y.saturating_add(PROMPT_HEADER_HEIGHT),
            area.width,
            5,
        ),
        2,
    );
    let lines = vec![
        Line::from(Span::styled(
            format!("Waiting for {provider_label} authorization..."),
            Style::default().fg(theme.text.tertiary),
        )),
        Line::from(""),
        Line::from(split_hint("c copy", theme)),
    ];
    frame.render_widget(Paragraph::new(lines), body);
}

pub(super) fn render_result_panel(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
    success: bool,
) {
    let dialog = &app.connect_dialog;
    let area = render_panel(frame, theme, root, 7);
    render_prompt_header(
        frame,
        theme,
        area,
        if success {
            "Connected"
        } else {
            "Connection failed"
        },
    );

    let message = dialog
        .toast
        .as_ref()
        .map(|toast| toast.message.as_str())
        .or(dialog.error_message.as_deref())
        .unwrap_or(if success {
            "Connected successfully"
        } else {
            "Authentication failed"
        });
    let model_info = dialog
        .selected_model
        .and_then(|index| dialog.models.get(index))
        .map(|model| format!("model: {model}"));
    let color = if success {
        theme.status.success
    } else {
        theme.status.error
    };
    let mut lines = vec![Line::from(Span::styled(
        message,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))];
    if let Some(model) = model_info {
        lines.push(Line::from(Span::styled(
            model,
            Style::default().fg(theme.text.tertiary),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(split_hint("c copy", theme)));
    let body = horizontal_inset(
        Rect::new(
            area.x,
            area.y.saturating_add(PROMPT_HEADER_HEIGHT),
            area.width,
            4,
        ),
        2,
    );
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), body);
}
