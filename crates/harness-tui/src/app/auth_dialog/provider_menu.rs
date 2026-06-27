use harness_core::auth::ProviderId;

use super::{auth_method_label, ConnectDialogState, ConnectProviderOption};

const PROVIDER_PRIORITY: [&str; 7] = [
    "opencode",
    "opencode-go",
    "openai",
    "codex",
    "github-copilot",
    "anthropic",
    "google",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectProviderMenuItem {
    Provider(usize),
    Custom,
}

impl ConnectDialogState {
    pub fn refilter_providers(&self) -> Vec<&ConnectProviderOption> {
        self.filtered_provider_indices()
            .into_iter()
            .filter_map(|index| self.providers.get(index))
            .collect()
    }

    pub(crate) fn provider_menu_items(&self) -> Vec<ConnectProviderMenuItem> {
        let indices = self.filtered_provider_indices();
        let mut items = indices
            .iter()
            .copied()
            .filter(|index| {
                self.providers
                    .get(*index)
                    .is_some_and(is_popular_connect_provider)
            })
            .map(ConnectProviderMenuItem::Provider)
            .collect::<Vec<_>>();
        items.extend(
            indices
                .into_iter()
                .filter(|index| {
                    self.providers
                        .get(*index)
                        .is_some_and(|provider| !is_popular_connect_provider(provider))
                })
                .map(ConnectProviderMenuItem::Provider),
        );
        if custom_provider_matches(&self.filter_buffer) {
            items.push(ConnectProviderMenuItem::Custom);
        }
        items
    }

    pub(crate) fn filtered_provider_indices(&self) -> Vec<usize> {
        let filter = normalized_filter(&self.filter_buffer);
        self.providers
            .iter()
            .enumerate()
            .filter_map(|(index, provider)| {
                provider_matches(provider, filter.as_deref()).then_some(index)
            })
            .collect()
    }

    pub(crate) fn filtered_method_indices(&self) -> Vec<usize> {
        let Some(provider) = self
            .selected_provider
            .and_then(|index| self.providers.get(index))
        else {
            return Vec::new();
        };
        let filter = normalized_filter(&self.filter_buffer);
        provider
            .methods
            .iter()
            .enumerate()
            .filter_map(|(index, method)| {
                let label = auth_method_label(method).to_lowercase();
                filter
                    .as_deref()
                    .is_none_or(|needle| label.contains(needle))
                    .then_some(index)
            })
            .collect()
    }

    pub(crate) fn filtered_model_indices(&self) -> Vec<usize> {
        let filter = normalized_filter(&self.filter_buffer);
        self.models
            .iter()
            .enumerate()
            .filter_map(|(index, model)| {
                filter
                    .as_deref()
                    .is_none_or(|needle| model.to_lowercase().contains(needle))
                    .then_some(index)
            })
            .collect()
    }

    pub(crate) fn skip_model_matches_filter(&self) -> bool {
        normalized_filter(&self.filter_buffer).is_none_or(|needle| {
            "skip model selection".contains(&needle) || "skip".contains(&needle)
        })
    }
}

pub(crate) fn is_filter_char(c: char) -> bool {
    !c.is_control() && c != '\u{7f}'
}

pub(crate) fn is_popular_connect_provider(provider: &ConnectProviderOption) -> bool {
    PROVIDER_PRIORITY.contains(&provider.id.as_str())
}

pub(crate) fn normalize_custom_provider_id(value: &str) -> Option<ProviderId> {
    let normalized = value
        .trim()
        .strip_prefix("@ai-sdk/")
        .unwrap_or(value.trim());
    let mut chars = normalized.chars();
    let first = chars.next()?;
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return None;
    }
    ProviderId::parse(normalized)
}

fn normalized_filter(value: &str) -> Option<String> {
    let filter = value.trim().to_lowercase();
    (!filter.is_empty()).then_some(filter)
}

fn provider_matches(provider: &ConnectProviderOption, filter: Option<&str>) -> bool {
    filter.is_none_or(|needle| {
        provider.label.to_lowercase().contains(needle)
            || provider.id.as_str().to_lowercase().contains(needle)
            || provider.description.to_lowercase().contains(needle)
    })
}

fn custom_provider_matches(filter: &str) -> bool {
    normalized_filter(filter)
        .is_none_or(|needle| "other".contains(&needle) || "custom provider".contains(&needle))
}
