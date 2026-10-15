//! The native lane gates derived from settings. The configured summary endpoint is a
//! separate setting and reaches only the text-summary calls.
use crate::coord::turn::Worker;
use harness_providers::{CompletionRequest, NativeLaneSettings, RemoteModelOverride};

impl Worker {
    /// The remote gates the settings give `request`'s model: the global switches, the
    /// configured endpoints and the per-model override.
    pub(in crate::coord) fn native_lane_settings(
        &self,
        request: &CompletionRequest,
    ) -> NativeLaneSettings {
        let settings = &self.compaction;
        let model = request.provider_id.as_deref().and_then(|provider| {
            remote_override(&settings.remote_models, provider, &request.model_id)
        });
        NativeLaneSettings {
            remote_enabled: settings.remote_enabled,
            v2_enabled: settings.remote_streaming_v2_enabled
                && model
                    .as_ref()
                    .and_then(|m| m.v2_streaming_enabled)
                    .unwrap_or(true),
            v2_endpoint: model.as_ref().and_then(|m| m.v2_endpoint.clone()),
            endpoint: model.as_ref().and_then(|m| m.endpoint.clone()),
            model: model.map(|m| RemoteModelOverride {
                enabled: m.enabled,
                api: m.api,
                model: m.model,
            }),
        }
    }
}

/// Exact `provider/model` or `provider:model` wins over any wildcard; among terminal-`*`
/// prefixes the longest wins.
fn remote_override(
    models: &std::collections::BTreeMap<String, crate::config::RemoteModelSettings>,
    provider: &str,
    model: &str,
) -> Option<crate::config::RemoteModelSettings> {
    let slash = format!("{provider}/{model}");
    let colon = format!("{provider}:{model}");
    models
        .get(&slash)
        .or_else(|| models.get(&colon))
        .or_else(|| {
            models
                .iter()
                .filter_map(|(key, value)| {
                    let prefix = key.strip_suffix('*')?;
                    (slash.starts_with(prefix) || colon.starts_with(prefix))
                        .then_some((prefix.len(), value))
                })
                .max_by_key(|(length, _)| *length)
                .map(|(_, value)| value)
        })
        .cloned()
}
