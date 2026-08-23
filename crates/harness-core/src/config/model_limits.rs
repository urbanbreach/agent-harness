use serde::{Deserialize, Serialize};

use super::{MaxInputSemantics, ModelLimitError, ModelLimitProvenance, ModelLimitProvenanceKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl ResolvedModelLimits {
    pub fn from_values(
        context_window: Option<u32>,
        max_input: Option<u32>,
        max_output: Option<u32>,
        provenance: ModelLimitProvenance,
    ) -> Self {
        Self {
            context_window: resolved_optional_limit(
                context_window,
                provenance.clone(),
                "context window",
            ),
            max_input: resolved_optional_limit(max_input, provenance.clone(), "max input"),
            max_output: resolved_optional_limit(max_output, provenance, "max output"),
            max_input_semantics: MaxInputSemantics::ProviderVisibleInputTokens,
        }
    }

    pub fn compatibility_mirror(
        context_window: Option<u32>,
        max_input: Option<u32>,
        max_output: Option<u32>,
    ) -> Self {
        Self::from_values(
            context_window,
            max_input,
            max_output,
            ModelLimitProvenance::compatibility("legacy model metadata mirror"),
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

    pub fn is_selectable_known(&self) -> bool {
        matches!(
            (self.context_window.tokens, self.max_output.tokens),
            (Some(context), Some(output)) if context > 0 && output > 0 && output <= context
        ) && matches!(self.max_input.tokens, None | Some(1..))
            && self
                .max_input
                .tokens
                .is_none_or(|input| input <= self.context_window.tokens.unwrap_or_default())
    }

    pub fn primary_provenance(&self) -> &ModelLimitProvenance {
        &self.context_window.provenance
    }

    pub fn validate(&self, identity: &str) -> Result<(), ModelLimitError> {
        let (context, input, output) = match (
            self.context_window.tokens,
            self.max_input.tokens,
            self.max_output.tokens,
        ) {
            (None, None, None) => return Ok(()),
            (Some(context), input, Some(output)) => (context, input, output),
            _ => {
                return Err(ModelLimitError::Partial {
                    identity: identity.to_string(),
                })
            }
        };
        if context == 0 {
            return Err(ModelLimitError::ZeroContext {
                identity: identity.to_string(),
            });
        }
        if output == 0 {
            return Err(ModelLimitError::ZeroOutput {
                identity: identity.to_string(),
            });
        }
        if let Some(input) = input {
            if input == 0 {
                return Err(ModelLimitError::ZeroInput {
                    identity: identity.to_string(),
                });
            }
            if input > context {
                return Err(ModelLimitError::InputAboveContext {
                    identity: identity.to_string(),
                    input,
                    context,
                });
            }
        }
        if output > context {
            return Err(ModelLimitError::OutputAboveContext {
                identity: identity.to_string(),
                output,
                context,
            });
        }
        Ok(())
    }
}

fn resolved_optional_limit(
    tokens: Option<u32>,
    provenance: ModelLimitProvenance,
    field: &str,
) -> ResolvedModelLimit {
    match tokens {
        Some(tokens) => ResolvedModelLimit {
            tokens: Some(tokens),
            provenance,
        },
        None => ResolvedModelLimit {
            tokens: None,
            provenance: ModelLimitProvenance::unknown(format!(
                "catalog has no authoritative {field}"
            )),
        },
    }
}

impl Default for ResolvedModelLimits {
    fn default() -> Self {
        let unknown = ModelLimitProvenance::unknown("model limits were not recorded");
        Self::from_values(None, None, None, unknown)
    }
}
