use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxInputSemantics {
    ProviderVisibleInputTokens,
}
impl MaxInputSemantics {
    pub fn as_str(self) -> &'static str {
        "provider_visible_input_tokens"
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
    pub source: Option<String>,
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
    pub fn compatibility(detail: impl Into<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::CompatibilityFallback,
            ..Self::explicit(detail)
        }
    }
    pub fn unknown(detail: impl Into<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::Unknown,
            ..Self::explicit(detail)
        }
    }
    pub fn generated(source: impl Into<String>, verified_at: Option<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::GeneratedCatalog,
            detail: "generated catalog".into(),
            source: Some(source.into()),
            verified_at,
        }
    }
    pub fn discovered(source: impl Into<String>, verified_at: Option<String>) -> Self {
        Self {
            kind: ModelLimitProvenanceKind::ProviderDiscovered,
            ..Self::generated(source, verified_at)
        }
    }
}
impl Default for ModelLimitProvenance {
    fn default() -> Self {
        Self::unknown("limits are unavailable")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelLimit {
    pub tokens: Option<u32>,
    pub provenance: ModelLimitProvenance,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelLimits {
    pub context_window: ResolvedModelLimit,
    pub max_input: ResolvedModelLimit,
    pub max_output: ResolvedModelLimit,
    pub max_input_semantics: MaxInputSemantics,
}
impl Default for ResolvedModelLimits {
    fn default() -> Self {
        Self::from_values(None, None, None, ModelLimitProvenance::default())
    }
}
impl ResolvedModelLimits {
    pub fn from_values(
        context: Option<u32>,
        input: Option<u32>,
        output: Option<u32>,
        provenance: ModelLimitProvenance,
    ) -> Self {
        let limit = |tokens| ResolvedModelLimit {
            tokens,
            provenance: if tokens.is_some() {
                provenance.clone()
            } else {
                ModelLimitProvenance::default()
            },
        };
        Self {
            context_window: limit(context),
            max_input: limit(input),
            max_output: limit(output),
            max_input_semantics: MaxInputSemantics::ProviderVisibleInputTokens,
        }
    }
    pub fn compatibility_mirror(
        context: Option<u32>,
        input: Option<u32>,
        output: Option<u32>,
    ) -> Self {
        Self::from_values(
            context,
            input,
            output,
            ModelLimitProvenance::compatibility("legacy model metadata"),
        )
    }
    pub fn context_window_tokens(&self) -> Option<u32> {
        self.context_window.tokens
    }
    pub fn max_input_tokens(&self) -> Option<u32> {
        self.max_input.tokens
    }
    pub fn max_output_tokens(&self) -> Option<u32> {
        self.max_output.tokens
    }
    pub fn is_exact(&self) -> bool {
        self.context_window.tokens.is_some()
            && self.max_input.tokens.is_some()
            && self.max_output.tokens.is_some()
    }
    pub fn has_authoritative_input(&self) -> bool {
        self.max_input.tokens.is_some()
            && self.max_input.provenance.kind != ModelLimitProvenanceKind::Unknown
    }
    pub fn primary_provenance(&self) -> &ModelLimitProvenance {
        &self.context_window.provenance
    }
    pub fn validate(&self, identity: &str) -> Result<(), ModelLimitError> {
        match (self.context_window.tokens, self.max_input.tokens, self.max_output.tokens) {
            (None, None, None) => Ok(()),
            (Some(context @ 1..), input, Some(output @ 1..))
                if output <= context && input.is_none_or(|n| n > 0 && n <= context) => Ok(()),
            _ => Err(ModelLimitError(format!("{identity}: context and output limits must be positive; input and output cannot exceed context"))),
        }
    }
    pub fn is_selectable_known(&self) -> bool {
        self.context_window.tokens.is_some() && self.validate("model").is_ok()
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct ModelLimitError(pub String);
