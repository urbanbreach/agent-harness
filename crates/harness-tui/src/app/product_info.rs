use super::*;

#[derive(Default)]
pub(crate) struct ProductInfo {
    pub(crate) visible: bool,
    pub(crate) title: &'static str,
    pub(crate) rows: Vec<(String, String)>,
    pub(crate) selected: usize,
    pub(crate) query: String,
    focus_return: Option<Focus>,
}

impl ProductInfo {
    pub(crate) fn matches(&self) -> Vec<&(String, String)> {
        let query = self.query.to_lowercase();
        self.rows
            .iter()
            .filter(|(label, text)| format!("{label} {text}").to_lowercase().contains(&query))
            .collect()
    }
}

impl AppState {
    pub(in crate::app) fn open_product_info(&mut self, usage: bool) {
        let mut rows = Vec::new();
        if usage {
            rows = self.recorded_usage_rows();
        } else if self.replay_mode {
            rows.push((
                "Recorded session".into(),
                "Live extension configuration and connection state are unavailable during replay."
                    .into(),
            ));
        } else {
            if let Some(config) = harness_core::config::registered_integrations_config() {
                for (name, server) in config.mcp.servers {
                    let state = mcp_connection_description(&name, server.enabled());
                    rows.push((format!("MCP · {name}"), state));
                }
            }
            if let Some(summary) = self.extension_manifest_summary.as_ref() {
                rows.push(("Extension manifest".into(), summary.one_line()));
            }
            if let Some(summary) = self.extension_discover_summary {
                rows.push(("Discovery".into(), summary.one_line()));
            }
            if rows.is_empty() {
                rows.push((
                    "No configured extensions".into(),
                    "Use the MCP configuration controls to manage configured servers.".into(),
                ));
            }
            rows.push(("Management".into(), "Press m for the existing MCP controls. Extension installation and marketplace browsing are unavailable in this TUI.".into()));
        }
        self.product_info = ProductInfo {
            visible: true,
            title: if usage { "Usage" } else { "Extensions" },
            rows,
            selected: 0,
            query: String::new(),
            focus_return: Some(self.focus),
        };
        self.modal_interaction.invalidate();
    }

    fn recorded_usage_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        let mut input = 0u64;
        let mut output = 0u64;
        let mut unknown = 0;
        for activity in &self.activities {
            if let Some(usage) = activity.usage {
                input += u64::from(usage.prompt_tokens);
                output += u64::from(usage.completion_tokens);
                let cache = activity
                    .cache_usage
                    .map(|cache| {
                        format!(
                            "\nCache read: {}\nCache write: {}",
                            cache.read_tokens, cache.write_tokens
                        )
                    })
                    .unwrap_or_default();
                rows.push((
                    format!("{} · {} tokens", activity.model_id, usage.total_tokens),
                    format!(
                        "Input: {}\nOutput: {}\nTotal: {}{cache}",
                        usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                    ),
                ));
            } else {
                unknown += 1;
            }
        }
        rows.insert(
            0,
            (
                "Recorded usage".into(),
                format!(
                    "Input: {input}\nOutput: {output}\nRequests without recorded usage: {unknown}"
                ),
            ),
        );
        if let Some(budget) = self.current_request_budget_snapshot() {
            rows.push((
                "Context budget".into(),
                format!(
                    "Estimated occupied input: {}\nCapacity: {}\nBudget status: {:?}",
                    budget.occupied_input_tokens,
                    budget
                        .compaction_threshold_tokens
                        .map_or_else(|| "unknown".into(), |value| value.to_string()),
                    budget.status
                ),
            ));
        } else {
            rows.push((
                "Context budget".into(),
                "Capacity unavailable in this session".into(),
            ));
        }
        rows.push(("Provider account".into(), "Account credits and billing are unavailable in Harness. Token figures above come from recorded provider usage.".into()));
        rows
    }

    pub(in crate::app) fn handle_product_info_key(&mut self, key: KeyEvent) {
        let count = self.product_info.matches().len();
        match key.code {
            KeyCode::Esc => {
                self.product_info.visible = false;
                if let Some(focus) = self.product_info.focus_return.take() {
                    self.focus = focus;
                }
            }
            KeyCode::Up => {
                self.product_info.selected = self.product_info.selected.saturating_sub(1)
            }
            KeyCode::Down => {
                self.product_info.selected =
                    (self.product_info.selected + 1).min(count.saturating_sub(1))
            }
            KeyCode::PageUp => {
                self.product_info.selected = self.product_info.selected.saturating_sub(8)
            }
            KeyCode::PageDown => {
                self.product_info.selected =
                    (self.product_info.selected + 8).min(count.saturating_sub(1))
            }
            KeyCode::Char('m') if self.product_info.title == "Extensions" && !self.replay_mode => {
                self.product_info.visible = false;
                self.open_toggles_menu();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some((label, text)) =
                    self.product_info.matches().get(self.product_info.selected)
                {
                    match clipboard::copy(&ui::safe_product_text(&format!("{label}\n{text}"))) {
                        Ok(()) => self.show_toast("Copied", ToastVariant::Info),
                        Err(err) => self.show_toast(
                            format!("clipboard copy failed: {err}"),
                            ToastVariant::Error,
                        ),
                    }
                }
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.product_info.query.push(c);
                self.product_info.selected = 0;
            }
            KeyCode::Backspace => {
                let _ = self.product_info.query.pop();
                self.product_info.selected = 0;
            }
            _ => {}
        }
    }
}

fn mcp_connection_description(name: &str, enabled: bool) -> String {
    if !enabled {
        return "Disabled".into();
    }
    match harness_core::config::registered_mcp_server_connection_state(name) {
        Some(harness_core::config::McpServerConnectionState::Connected) => "Connected".into(),
        Some(harness_core::config::McpServerConnectionState::Failed(error)) => {
            format!("Failed: {error}")
        }
        None => "Connection state unavailable".into(),
    }
}
