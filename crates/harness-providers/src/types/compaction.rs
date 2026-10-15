use std::collections::BTreeMap;

/// Per-model remote compaction override resolved by the coordinator from settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteModelOverride {
    /// `Some(false)` disables native compaction for the model; `Some(true)` opts it in.
    pub enabled: Option<bool>,
    /// Explicit wire protocol name (`responses`, `anthropic`, `chat`).
    pub api: Option<String>,
    /// Model id sent to the native endpoint instead of the active model.
    pub model: Option<String>,
}

/// Resolved remote compaction gates handed to a provider when it lists native lanes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeLaneSettings {
    /// Global `remote_enabled`; false offers no lane at all.
    pub remote_enabled: bool,
    /// Global streaming V2 gate; only a literal false disables it.
    pub v2_enabled: bool,
    pub v2_endpoint: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<RemoteModelOverride>,
}

/// Credentials a compaction side request sends to a configured endpoint.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CompactionCredential {
    pub api_key: Option<String>,
    pub headers: BTreeMap<String, String>,
}

impl std::fmt::Debug for CompactionCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompactionCredential")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field(
                "headers",
                &self
                    .headers
                    .keys()
                    .map(|name| (name.as_str(), "<redacted>"))
                    .collect::<BTreeMap<_, _>>(),
            )
            .finish()
    }
}
