use ratatui::{
    style::Style,
    widgets::{Block, Clear},
    Frame,
};

use crate::app::auth_dialog::ConnectDialogStep;
use crate::app::AppState;
use crate::theme::Theme;

#[path = "auth_dialog/common.rs"]
mod common;
#[path = "auth_dialog/prompt.rs"]
mod prompt;
#[path = "auth_dialog/prompt_panel.rs"]
mod prompt_panel;
#[path = "auth_dialog/provider_rows.rs"]
mod provider_rows;
#[path = "auth_dialog/select.rs"]
mod select;

pub(crate) use prompt::waiting_authorization_detail_at;
pub(super) use prompt_panel::PromptPanel;

pub(super) fn render_auth_dialog_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: ratatui::layout::Rect,
) {
    let dialog = &app.connect_dialog;
    if !dialog.visible {
        return;
    }

    frame.render_widget(Clear, root);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.overlay)),
        root,
    );

    match dialog.step {
        ConnectDialogStep::SelectProvider => {
            select::render_provider_select(frame, app, theme, root);
        }
        ConnectDialogStep::SelectMethod => {
            select::render_method_select(frame, app, theme, root);
        }
        ConnectDialogStep::SelectModel => {
            select::render_model_select(frame, app, theme, root);
        }
        ConnectDialogStep::CustomProviderId => {
            prompt::render_custom_provider_prompt(frame, app, theme, root);
        }
        ConnectDialogStep::ApiKeyInput => {
            prompt::render_api_key_prompt(frame, app, theme, root);
        }
        ConnectDialogStep::EnterpriseUrl => {
            prompt::render_enterprise_url_prompt(frame, app, theme, root);
        }
        ConnectDialogStep::Waiting => {
            prompt::render_waiting_panel(frame, app, theme, root);
        }
        ConnectDialogStep::Success => {
            prompt::render_result_panel(frame, app, theme, root, true);
        }
        ConnectDialogStep::Error => {
            prompt::render_result_panel(frame, app, theme, root, false);
        }
    }
}
