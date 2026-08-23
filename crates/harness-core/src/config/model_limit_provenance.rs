use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxInputSemantics {
    /// Maximum provider-visible prompt tokens, excluding generated output.
    ProviderVisibleInputTokens,
}

impl MaxInputSemantics {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderVisibleInputTokens => "provider_visible_input_tokens",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLimitProvenanceKind {
    ExplicitConfig,
    GeneratedCatalog,
    ProviderDiscovered,
    CompatibilityFallback,
    Unknown,
}

impl ModelLimitProvenanceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitConfig => "explicit_config",
            Self::GeneratedCatalog => "generated_catalog",
            Self::ProviderDiscovered => "provider_discovered",
            Self::CompatibilityFallback => "compatibility_fallback",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimitProvenance {
    pub kind: ModelLimitProvenanceKind,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
}

impl ModelLimitProvenance {
    pub fn explicit(detail: impl Into<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::ExplicitConfig,
            detail: detail.into(),
            source: None,
            verified_at: None,
        }
    }

    pub fn generated(source: impl Into<String>, verified_at: Option<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::GeneratedCatalog,
            detail: "generated provider catalog".to_string(),
            source: Some(source.into()),
            verified_at,
        }
    }

    pub fn discovered(source: impl Into<String>, verified_at: Option<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::ProviderDiscovered,
            detail: "provider-discovered catalog metadata".to_string(),
            source: Some(source.into()),
            verified_at,
        }
    }

    pub fn compatibility(detail: impl Into<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::CompatibilityFallback,
            detail: detail.into(),
            source: None,
            verified_at: None,
        }
    }

    pub(super) fn unknown(detail: impl Into<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::Unknown,
            detail: detail.into(),
            source: None,
            verified_at: None,
        }
    }
}

impl Default for ModelLimitProvenance {
    fn default() -> Self {
        Self::explicit("model configuration")
    }
}
