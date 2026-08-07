use std::num::NonZeroU16;

use ratatui::{
    buffer::CellDiffOption,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::common::{
    horizontal_inset, panel_area, render_panel, render_prompt_header, split_hint,
    PROMPT_HEADER_HEIGHT,
};
use super::prompt_panel::PromptPanel;

use crate::app::auth_dialog::{auth_method_label, AuthorizationDetail, ConnectDialogState};
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
    let area = render_panel(frame, theme, root, 9);
    render_prompt_header(frame, theme, area, method_label);

    let body = horizontal_inset(
        Rect::new(
            area.x,
            area.y.saturating_add(PROMPT_HEADER_HEIGHT),
            area.width,
            6,
        ),
        2,
    );
    let mut lines = Vec::new();
    if let Some(url) = dialog.authorization_url() {
        lines.push(Line::from(Span::styled(
            url.to_string(),
            Style::default()
                .fg(theme.markdown.link)
                .add_modifier(Modifier::UNDERLINED),
        )));
    }
    if let Some(code) = dialog.authorization_code() {
        lines.push(Line::from(vec![
            Span::styled("Enter code: ", Style::default().fg(theme.text.tertiary)),
            Span::styled(
                code.to_string(),
                Style::default()
                    .fg(theme.text.primary)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("Waiting for {provider_label} authorization..."),
        Style::default().fg(theme.text.tertiary),
    )));
    lines.push(Line::from(split_hint("c copy", theme)));
    if let Some(toast) = &dialog.toast {
        lines.push(Line::from(Span::styled(
            toast.message.as_str(),
            Style::default().fg(if toast.is_success {
                theme.status.success
            } else {
                theme.status.error
            }),
        )));
    }
    frame.render_widget(Paragraph::new(lines), body);
    if let Some(url) = dialog.authorization_url() {
        let label = crate::ui::ui_chrome::take_width_prefix(url, usize::from(body.width));
        let width = unicode_width::UnicodeWidthStr::width(label);
        if let Some(width) = u16::try_from(width).ok().and_then(NonZeroU16::new) {
            let hyperlink = crate::clipboard::format_osc8_hyperlink(url, label);
            if let Some(cell) = frame.buffer_mut().cell_mut((body.x, body.y)) {
                cell.set_symbol(&hyperlink)
                    .set_diff_option(CellDiffOption::ForcedWidth(width));
            }
        }
    }
}

pub(crate) fn waiting_authorization_detail_at(
    dialog: &ConnectDialogState,
    root: Rect,
    column: u16,
    row: u16,
) -> Option<AuthorizationDetail> {
    let area = panel_area(root, 9);
    let content = horizontal_inset(
        Rect::new(
            area.x,
            area.y.saturating_add(PROMPT_HEADER_HEIGHT),
            area.width,
            6,
        ),
        2,
    );
    if column < content.x || column >= content.x.saturating_add(content.width) {
        return None;
    }
    let mut detail_row = content.y;
    if dialog.authorization_url().is_some() {
        if row == detail_row {
            return Some(AuthorizationDetail::Url);
        }
        detail_row = detail_row.saturating_add(1);
    }
    (dialog.authorization_code().is_some() && row == detail_row)
        .then_some(AuthorizationDetail::Code)
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

    let message = if success {
        dialog
            .toast
            .as_ref()
            .map(|toast| toast.message.as_str())
            .or(dialog.error_message.as_deref())
            .unwrap_or("Connected successfully")
    } else {
        dialog
            .error_message
            .as_deref()
            .or_else(|| dialog.toast.as_ref().map(|toast| toast.message.as_str()))
            .unwrap_or("Authentication failed")
    };
    let model_info = dialog
        .selected_model
        .and_then(|index| dialog.models.get(index))
        .map(|model| format!("model: {model}"));
    let color = if success {
        theme.status.success
    } else {
        theme.status.error
    };
    let message_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let mut lines = message
        .lines()
        .map(|line| Line::from(Span::styled(line, message_style)))
        .collect::<Vec<_>>();
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
