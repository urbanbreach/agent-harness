use harness_core::event::EventV1;

use super::{AppState, UiIntent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDetailsViewModel {
    pub category: &'static str,
    pub message: String,
    pub recovery_hint: &'static str,
    pub request_id: Option<String>,
    pub prompt_text: Option<String>,
    pub replay_mode: bool,
}

impl AppState {
    pub fn last_error_details_view_model(&self) -> Option<ErrorDetailsViewModel> {
        let failed = self.events.iter().rev().find_map(|event| {
            let EventV1::RunFailed(data) = &event.payload else {
                return None;
            };
            Some((event.correlation_id.clone(), data.error.clone()))
        })?;
        let request_id = failed.0;
        let prompt_text = request_id
            .as_deref()
            .and_then(|request_id| self.prompt_text_for_request(request_id))
            .or_else(|| self.last_user_prompt_text());
        let category = provider_error_category(&failed.1);
        Some(ErrorDetailsViewModel {
            category,
            message: failed.1,
            recovery_hint: provider_error_recovery_hint(category),
            request_id,
            prompt_text,
            replay_mode: self.replay_mode,
        })
    }

    pub(in crate::app) fn open_error_details(&mut self) {
        if self.last_error_details_view_model().is_none() {
            self.status_banner = Some("no failed turn to inspect".to_string());
            return;
        }
        self.close_palette();
        self.overlay_state.error_details_visible = true;
    }

    pub(in crate::app) fn close_error_details(&mut self) {
        self.overlay_state.error_details_visible = false;
    }

    pub(in crate::app) fn handle_error_details_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> bool {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.close_error_details();
                true
            }
            crossterm::event::KeyCode::Enter if !self.replay_mode => {
                let Some(prompt_text) = self
                    .last_error_details_view_model()
                    .and_then(|details| details.prompt_text)
                else {
                    self.status_banner = Some("resubmit unavailable: no prior prompt".to_string());
                    return true;
                };
                self.close_error_details();
                self.emit_ui_intent(UiIntent::SubmitPrompt {
                    text: prompt_text,
                    selected_file_tags: Vec::new(),
                    selected_agent_tags: Vec::new(),
                    selected_resource_tags: Vec::new(),
                    launch_metadata: self.launch_metadata.clone(),
                });
                true
            }
            _ => true,
        }
    }

    fn prompt_text_for_request(&self, request_id: &str) -> Option<String> {
        self.events.iter().rev().find_map(|event| {
            let EventV1::UserMessageSubmitted(data) = &event.payload else {
                return None;
            };
            (data.request_id == request_id).then(|| data.text.clone())
        })
    }

    fn last_user_prompt_text(&self) -> Option<String> {
        self.events.iter().rev().find_map(|event| {
            let EventV1::UserMessageSubmitted(data) = &event.payload else {
                return None;
            };
            Some(data.text.clone())
        })
    }
}

fn provider_error_category(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("rate_limited")
        || lower.contains("rate limit")
        || lower.contains("status 429")
    {
        "rate_limited"
    } else if lower.contains("missing_credentials") || lower.contains("missing credential") {
        "missing_credentials"
    } else if lower.contains("invalid_credentials") || lower.contains("invalid credential") {
        "invalid_credentials"
    } else if lower.contains("context_window") || lower.contains("context window") {
        "context_window_exceeded"
    } else if lower.contains("unsupported_tool") || lower.contains("unsupported tool") {
        "unsupported_tool_call"
    } else if lower.contains("malformed") {
        "malformed_stream"
    } else if lower.contains("transport") || lower.contains("network") {
        "transport_failure"
    } else {
        "other"
    }
}

fn provider_error_recovery_hint(category: &str) -> &'static str {
    match category {
        "missing_credentials" => {
            "Configure the provider API key or apiKeyEnv value, then retry."
        }
        "invalid_credentials" => {
            "Check that the provider credential is valid for the selected provider and model."
        }
        "rate_limited" => {
            "Wait for the provider rate limit to reset or switch to a less constrained model/provider."
        }
        "context_window_exceeded" => {
            "Reduce prompt context, enable compaction, or choose a model with a larger context window."
        }
        "unsupported_tool_call" => {
            "Inspect the tool schema and provider support matrix, then retry with a supported tool shape."
        }
        "malformed_stream" => {
            "Retry the request; if it repeats, capture a support bundle because the provider stream was malformed."
        }
        "transport_failure" => "Check provider base URL/network reachability and retry the request.",
        _ => "Inspect the provider message and support bundle for the provider-specific failure detail.",
    }
}
