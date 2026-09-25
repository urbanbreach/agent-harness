use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub api_mode: OpenAiApiMode,
    pub timeout_ms: u64,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    Responses,
    ChatCompletions,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiAuthProfile {
    Codex,
    GithubCopilot,
}

#[derive(Debug, Error)]
pub enum OpenAiCompatibleProviderError {
    #[error("failed to build HTTP client: {0}")]
    BuildHttpClient(#[source] reqwest::Error),
    #[error("invalid header name `{header}`: {source}")]
    InvalidHeaderName {
        header: String,
        #[source]
        source: reqwest::header::InvalidHeaderName,
    },
    #[error("invalid header value for `{header}`: {source}")]
    InvalidHeaderValue {
        header: String,
        #[source]
        source: reqwest::header::InvalidHeaderValue,
    },
}

/// Models supported by Codex ChatGPT sign-in, verified 2026-09-25.
/// https://learn.chatgpt.com/docs/models#deprecated-codex-models
pub fn codex_oauth_model_allowed(model_id: &str) -> bool {
    matches!(
        model_id,
        "gpt-6-astra"
            | "gpt-6-sol"
            | "gpt-6-luna"
            | "gpt-5.6-sol"
            | "gpt-5.6-terra"
            | "gpt-5.6-luna"
            | "gpt-5.5"
    )
}
