//! Config-reachable provider leaf constructors and typed errors.
//!
//! This module defines the protocol-level boundary between config parsing
//! (owned by `harness-core::config`) and provider construction (owned by
//! `harness-providers`). Protocol workers call [`build_provider`] with
//! config-reachable parameters and receive either a ready [`Provider`] or a
//! truthful [`ProviderError`].
//!
//! Supported protocols are listed in [`ProviderProtocol`]. Unsupported
//! protocols are rejected with [`ProviderError::UnsupportedProtocol`] — no
//! silent fallback, no string-only error.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use crate::anthropic::{AnthropicProvider, AnthropicProviderConfig, AnthropicProviderError};
use crate::openai::{
    OpenAiApiMode, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
    OpenAiCompatibleProviderError,
};
use crate::Provider;

/// Config-reachable identifier for a provider protocol.
///
/// Each variant corresponds to a `type` tag in the runtime config
/// `ProviderConfig` enum. Variants here are the protocols the providers
/// crate can construct; the config root may advertise additional tags that
/// are not yet wired, and those are rejected as unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProtocol {
    /// `openai_compatible` — OpenAI-compatible streaming transport.
    OpenAiCompatible,
    /// `anthropic_messages` — Anthropic Messages API transport.
    AnthropicMessages,
}

impl fmt::Display for ProviderProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_type_tag())
    }
}

impl ProviderProtocol {
    /// Parse a config `type` tag into a protocol, returning `None` for
    /// unknown/unwired protocols.
    pub fn from_type_tag(tag: &str) -> Option<Self> {
        match tag {
            "openai_compatible" => Some(Self::OpenAiCompatible),
            "anthropic_messages" => Some(Self::AnthropicMessages),
            _ => None,
        }
    }

    /// The config `type` tag this protocol corresponds to.
    pub fn as_type_tag(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::AnthropicMessages => "anthropic_messages",
        }
    }

    /// All currently supported protocols, in canonical order.
    pub const SUPPORTED: [Self; 2] = [Self::OpenAiCompatible, Self::AnthropicMessages];
}

/// Comprehensive typed error for provider construction and protocol dispatch.
///
/// This is the leaf-level error protocol workers return. It is distinct from
/// [`crate::ProviderCredentialError`] (credential resolution at runtime) and
/// [`crate::ProviderErrorCategory`] (streaming error categorization) which
/// serve different layers.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// The config `type` tag does not map to any wired protocol.
    #[error("unsupported provider protocol `{tag}`; supported protocols: {}", supported_list(.supported))]
    UnsupportedProtocol {
        tag: String,
        supported: &'static [ProviderProtocol],
    },

    /// The protocol is recognized but the config parameters are invalid
    /// (missing base URL, invalid header, etc.).
    #[error("invalid config for provider `{protocol}`: {message}")]
    InvalidConfig {
        protocol: ProviderProtocol,
        message: String,
    },

    /// The HTTP client could not be constructed for this provider.
    #[error("failed to build HTTP client for provider `{protocol}`: {message}")]
    BuildHttpClient {
        protocol: ProviderProtocol,
        message: String,
    },

    /// A credential source is required but was not provided.
    #[error("credential source required for provider `{protocol}` but none configured")]
    MissingCredentialSource { protocol: ProviderProtocol },
}

/// Parameters for constructing an OpenAI-compatible provider from config.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleLeafParams {
    pub base_url: String,
    pub api_key: String,
    pub api_mode: OpenAiApiMode,
    pub timeout_ms: u64,
    pub headers: BTreeMap<String, String>,
}

/// Parameters for constructing an Anthropic Messages provider from config.
#[derive(Debug, Clone)]
pub struct AnthropicLeafParams {
    pub base_url: String,
    pub api_key: String,
    pub timeout_ms: u64,
    pub headers: BTreeMap<String, String>,
}

/// Config-reachable parameters for provider construction.
///
/// Each variant carries the minimal typed parameters the corresponding
/// provider constructor needs. The caller (typically the CLI bootstrap or
/// a protocol worker) is responsible for resolving env-var API keys before
/// calling [`build_provider`].
#[derive(Debug, Clone)]
pub enum ProviderLeafParams {
    OpenAiCompatible(OpenAiCompatibleLeafParams),
    AnthropicMessages(AnthropicLeafParams),
}

impl ProviderLeafParams {
    /// The protocol this parameter set targets.
    pub fn protocol(&self) -> ProviderProtocol {
        match self {
            Self::OpenAiCompatible(_) => ProviderProtocol::OpenAiCompatible,
            Self::AnthropicMessages(_) => ProviderProtocol::AnthropicMessages,
        }
    }
}

/// Build a provider from config-reachable parameters.
///
/// This is the leaf factory protocol workers call instead of editing shared
/// config roots. Supported protocols return a ready [`Provider`]; unsupported
/// protocols return [`ProviderError::UnsupportedProtocol`].
///
/// # Examples
///
/// ```
/// use std::collections::BTreeMap;
/// use harness_providers::leaf::{
///     build_provider, OpenAiCompatibleLeafParams, ProviderLeafParams,
/// };
/// use harness_providers::openai::OpenAiApiMode;
///
/// let params = ProviderLeafParams::OpenAiCompatible(OpenAiCompatibleLeafParams {
///     base_url: "http://127.0.0.1:8317/v1".to_string(),
///     api_key: "test-key".to_string(),
///     api_mode: OpenAiApiMode::Auto,
///     timeout_ms: 60000,
///     headers: BTreeMap::new(),
/// });
/// let provider = build_provider(params).expect("openai_compatible should construct");
/// ```
pub fn build_provider(params: ProviderLeafParams) -> Result<Arc<dyn Provider>, ProviderError> {
    match params {
        ProviderLeafParams::OpenAiCompatible(p) => {
            if p.base_url.trim().is_empty() {
                return Err(ProviderError::InvalidConfig {
                    protocol: ProviderProtocol::OpenAiCompatible,
                    message: "base_url must not be empty".to_string(),
                });
            }
            let config = OpenAiCompatibleProviderConfig {
                base_url: p.base_url,
                api_key: p.api_key,
                api_mode: p.api_mode,
                timeout_ms: p.timeout_ms,
                headers: p.headers,
            };
            let provider = OpenAiCompatibleProvider::new(config).map_err(map_openai_error)?;
            Ok(Arc::new(provider) as Arc<dyn Provider>)
        }
        ProviderLeafParams::AnthropicMessages(p) => {
            if p.base_url.trim().is_empty() {
                return Err(ProviderError::InvalidConfig {
                    protocol: ProviderProtocol::AnthropicMessages,
                    message: "base_url must not be empty".to_string(),
                });
            }
            let config = AnthropicProviderConfig {
                base_url: p.base_url,
                api_key: p.api_key,
                timeout_ms: p.timeout_ms,
                headers: p.headers,
            };
            let provider = AnthropicProvider::new(config).map_err(map_anthropic_error)?;
            Ok(Arc::new(provider) as Arc<dyn Provider>)
        }
    }
}

/// Attempt to resolve a protocol from a config type tag and return a typed
/// error if the protocol is not supported.
///
/// This is the single entry point protocol workers use to validate that a
/// config tag is wired before attempting construction.
pub fn resolve_protocol(tag: &str) -> Result<ProviderProtocol, ProviderError> {
    ProviderProtocol::from_type_tag(tag).ok_or_else(|| ProviderError::UnsupportedProtocol {
        tag: tag.to_string(),
        supported: &ProviderProtocol::SUPPORTED,
    })
}

fn map_openai_error(err: OpenAiCompatibleProviderError) -> ProviderError {
    match err {
        OpenAiCompatibleProviderError::BuildHttpClient(source) => ProviderError::BuildHttpClient {
            protocol: ProviderProtocol::OpenAiCompatible,
            message: source.to_string(),
        },
        OpenAiCompatibleProviderError::InvalidHeaderName { header, .. } => {
            ProviderError::InvalidConfig {
                protocol: ProviderProtocol::OpenAiCompatible,
                message: format!("invalid header name `{header}`"),
            }
        }
        OpenAiCompatibleProviderError::InvalidHeaderValue { header, .. } => {
            ProviderError::InvalidConfig {
                protocol: ProviderProtocol::OpenAiCompatible,
                message: format!("invalid header value for `{header}`"),
            }
        }
    }
}

fn map_anthropic_error(err: AnthropicProviderError) -> ProviderError {
    match err {
        AnthropicProviderError::BuildHttpClient(source) => ProviderError::BuildHttpClient {
            protocol: ProviderProtocol::AnthropicMessages,
            message: source.to_string(),
        },
    }
}

fn supported_list(supported: &'static [ProviderProtocol]) -> String {
    supported
        .iter()
        .map(|p| p.as_type_tag())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openai_params() -> OpenAiCompatibleLeafParams {
        OpenAiCompatibleLeafParams {
            base_url: "http://127.0.0.1:8317/v1".to_string(),
            api_key: "test-key".to_string(),
            api_mode: OpenAiApiMode::Auto,
            timeout_ms: 60000,
            headers: BTreeMap::new(),
        }
    }

    fn anthropic_params() -> AnthropicLeafParams {
        AnthropicLeafParams {
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "test-key".to_string(),
            timeout_ms: 60000,
            headers: BTreeMap::new(),
        }
    }

    #[test]
    fn build_provider_constructs_openai_compatible() {
        let params = ProviderLeafParams::OpenAiCompatible(openai_params());
        let provider = build_provider(params);
        assert!(provider.is_ok(), "openai_compatible should construct");
    }

    #[test]
    fn build_provider_constructs_anthropic_messages() {
        let params = ProviderLeafParams::AnthropicMessages(anthropic_params());
        let provider = build_provider(params);
        assert!(provider.is_ok(), "anthropic_messages should construct");
    }

    #[test]
    fn build_provider_rejects_empty_base_url_for_openai() {
        let mut params = openai_params();
        params.base_url = "  ".to_string();
        let result = build_provider(ProviderLeafParams::OpenAiCompatible(params));
        assert!(matches!(
            result,
            Err(ProviderError::InvalidConfig {
                protocol: ProviderProtocol::OpenAiCompatible,
                ..
            })
        ));
    }

    #[test]
    fn build_provider_rejects_empty_base_url_for_anthropic() {
        let mut params = anthropic_params();
        params.base_url = String::new();
        let result = build_provider(ProviderLeafParams::AnthropicMessages(params));
        assert!(matches!(
            result,
            Err(ProviderError::InvalidConfig {
                protocol: ProviderProtocol::AnthropicMessages,
                ..
            })
        ));
    }

    #[test]
    fn resolve_protocol_accepts_supported_tags() {
        assert_eq!(
            resolve_protocol("openai_compatible").unwrap(),
            ProviderProtocol::OpenAiCompatible
        );
        assert_eq!(
            resolve_protocol("anthropic_messages").unwrap(),
            ProviderProtocol::AnthropicMessages
        );
    }

    #[test]
    fn resolve_protocol_rejects_unsupported_tag() {
        let result = resolve_protocol("google_gemini");
        assert!(matches!(
            result,
            Err(ProviderError::UnsupportedProtocol { tag, .. }) if tag == "google_gemini"
        ));
    }

    #[test]
    fn resolve_protocol_rejects_empty_tag() {
        let result = resolve_protocol("");
        assert!(matches!(
            result,
            Err(ProviderError::UnsupportedProtocol { .. })
        ));
    }

    #[test]
    fn provider_protocol_type_tag_roundtrip() {
        for protocol in ProviderProtocol::SUPPORTED {
            let tag = protocol.as_type_tag();
            assert_eq!(ProviderProtocol::from_type_tag(tag), Some(protocol));
        }
    }

    #[test]
    fn unsupported_protocol_error_lists_supported_protocols() {
        let err = resolve_protocol("bedrock").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("openai_compatible"),
            "error should list supported protocols"
        );
        assert!(
            message.contains("anthropic_messages"),
            "error should list supported protocols"
        );
    }
}
