//! What the reading model needs from an archive.
use harness_tokenizer::{classify, Family};

/// Reader capabilities that change how an archive is built and shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReaderPolicy {
    /// The reader must not be shown assistant reasoning (its lineage extracts it).
    pub excludes_archived_reasoning: bool,
}

impl ReaderPolicy {
    /// Policy from the model's lineage, not from provider or gateway names.
    pub fn for_model(provider: &str, model: &str) -> Self {
        Self {
            excludes_archived_reasoning: matches!(
                classify(provider, model).family,
                Family::Anthropic { .. }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_exclusion_follows_lineage() {
        for (provider, model, excludes) in [
            ("anthropic", "claude-opus-5", true),
            ("openrouter", "anthropic/claude-sonnet-4-6", true),
            ("openai", "gpt-5.5", false),
            ("anthropic", "unlisted", false),
        ] {
            assert_eq!(
                ReaderPolicy::for_model(provider, model).excludes_archived_reasoning,
                excludes,
                "{provider} {model}"
            );
        }
    }
}
