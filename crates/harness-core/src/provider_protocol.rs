//! Provider protocol capability catalog (honest MVP).
//!
//! Documents which wire protocols the runtime can claim. Only
//! OpenAI-compatible is currently supported; other reference protocols are
//! catalogued as unsupported so inventory/doctor never imply false parity.

use serde::{Deserialize, Serialize};

/// Wire protocol families the product may eventually support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocolId {
    /// OpenAI-compatible chat/completions + responses-style transports.
    OpenAiCompatible,
    /// Anthropic Messages API (not implemented).
    AnthropicMessages,
    /// Google Generative Language / Gemini (not implemented).
    GoogleGenerative,
    /// AWS Bedrock runtime (not implemented).
    AwsBedrock,
    /// Azure OpenAI resource protocol quirks beyond openai_compatible (not implemented).
    AzureOpenAi,
}

impl ProviderProtocolId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai_compatible",
            Self::AnthropicMessages => "anthropic_messages",
            Self::GoogleGenerative => "google_generative",
            Self::AwsBedrock => "aws_bedrock",
            Self::AzureOpenAi => "azure_openai",
        }
    }
}

/// Support level for a catalogued protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolSupport {
    /// First-class runtime path exists.
    Supported,
    /// Documented only; no runtime transport.
    Unsupported,
}

impl ProtocolSupport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unsupported => "unsupported",
        }
    }

    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// One row in the protocol capability catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProtocolCapability {
    pub protocol: ProviderProtocolId,
    pub support: ProtocolSupport,
    pub notes: String,
}

/// Full catalog of protocol capabilities (static, deterministic).
pub fn provider_protocol_catalog() -> Vec<ProviderProtocolCapability> {
    vec![
        ProviderProtocolCapability {
            protocol: ProviderProtocolId::OpenAiCompatible,
            support: ProtocolSupport::Supported,
            notes: "Canonical runtime transport (`ProviderConfig::OpenAiCompatible`).".to_string(),
        },
        ProviderProtocolCapability {
            protocol: ProviderProtocolId::AnthropicMessages,
            support: ProtocolSupport::Unsupported,
            notes: "No first-party Anthropic Messages transport; do not claim parity.".to_string(),
        },
        ProviderProtocolCapability {
            protocol: ProviderProtocolId::GoogleGenerative,
            support: ProtocolSupport::Unsupported,
            notes: "No first-party Google Generative transport; do not claim parity.".to_string(),
        },
        ProviderProtocolCapability {
            protocol: ProviderProtocolId::AwsBedrock,
            support: ProtocolSupport::Unsupported,
            notes: "No first-party Bedrock transport; do not claim parity.".to_string(),
        },
        ProviderProtocolCapability {
            protocol: ProviderProtocolId::AzureOpenAi,
            support: ProtocolSupport::Unsupported,
            notes: "Azure-specific protocol quirks beyond openai_compatible are unsupported."
                .to_string(),
        },
    ]
}

/// Look up support for one protocol id.
pub fn protocol_support(protocol: ProviderProtocolId) -> ProtocolSupport {
    provider_protocol_catalog()
        .into_iter()
        .find(|row| row.protocol == protocol)
        .map(|row| row.support)
        .unwrap_or(ProtocolSupport::Unsupported)
}

/// True when any non-OpenAI-compatible protocol is supported (false in MVP).
pub fn non_openai_protocols_supported() -> bool {
    provider_protocol_catalog().into_iter().any(|row| {
        row.protocol != ProviderProtocolId::OpenAiCompatible && row.support.is_supported()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_protocol_row_carries_a_non_empty_explanatory_note() {
        // arrange
        let catalog = provider_protocol_catalog();

        // act — the catalog is static; evaluation is the walk itself

        // assert — honest catalog contract: non-empty notes, unique protocols, one Supported
        let mut seen = std::collections::HashSet::new();
        for entry in &catalog {
            assert!(
                !entry.notes.trim().is_empty(),
                "protocol {:?} has empty notes",
                entry.protocol
            );
            assert!(
                seen.insert(entry.protocol),
                "duplicate {:?}",
                entry.protocol
            );
        }
        let supported = catalog
            .iter()
            .filter(|entry| entry.support.is_supported())
            .count();
        assert_eq!(supported, 1);
    }

    #[test]
    fn catalog_marks_only_openai_compatible_supported() {
        // arrange
        // act
        // assert
        // When
        let catalog = provider_protocol_catalog();

        // Then
        assert!(!catalog.is_empty());
        let supported: Vec<_> = catalog
            .iter()
            .filter(|row| row.support.is_supported())
            .collect();
        assert_eq!(supported.len(), 1);
        assert_eq!(supported[0].protocol, ProviderProtocolId::OpenAiCompatible);
        assert!(!non_openai_protocols_supported());
        assert_eq!(
            protocol_support(ProviderProtocolId::AnthropicMessages),
            ProtocolSupport::Unsupported
        );
        assert_eq!(
            protocol_support(ProviderProtocolId::OpenAiCompatible),
            ProtocolSupport::Supported
        );
    }

    #[test]
    fn unsupported_rows_carry_honest_notes() {
        // arrange
        // act
        // assert
        for row in provider_protocol_catalog() {
            if row.protocol == ProviderProtocolId::OpenAiCompatible {
                continue;
            }
            assert_eq!(row.support, ProtocolSupport::Unsupported);
            assert!(!row.notes.is_empty());
        }
    }
}
