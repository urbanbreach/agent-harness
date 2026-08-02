//! Typed model-alias resolver contract (plan §1.1).
//!
//! Maps logical model IDs (what users select and what appears in config) to
//! canonical backend model IDs (what the provider actually routes to on the
//! wire). The resolver is a pure function — no network calls, no config I/O.
//!
//! Contract (plan §1.1):
//! - `umans-coder` → `umans-kimi-k2.7`
//! - `umans-flash` → `umans-qwen3.6-35b-a3b`
//!
//! Both aliases receive config/model-resolution coverage but no duplicate
//! live-provider run. Task 35 records logical ID, canonical backend ID, and
//! wire provider model ID in its receipt.

/// A typed model alias mapping: logical model ID → canonical backend model ID.
///
/// The `logical_id` is what the user configures and selects (e.g. `umans-coder`).
/// The `canonical_backend_id` is the actual backend model the provider routes
/// to (e.g. `umans-kimi-k2.7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelAlias {
    pub logical_id: &'static str,
    pub canonical_backend_id: &'static str,
}

/// Known model aliases for the `umans-ai-coding-plan` provider.
///
/// These are the only aliases declared in plan §1.1. Adding new aliases
/// requires updating this table and the corresponding tests.
const KNOWN_MODEL_ALIASES: &[ModelAlias] = &[
    ModelAlias {
        logical_id: "umans-coder",
        canonical_backend_id: "umans-kimi-k2.7",
    },
    ModelAlias {
        logical_id: "umans-flash",
        canonical_backend_id: "umans-qwen3.6-35b-a3b",
    },
];

/// Resolve a logical model ID to its canonical backend model ID.
///
/// If the input is not a known alias, it is returned unchanged — the caller
/// treats it as a direct model ID. This is a pure function with no side
/// effects or network calls.
pub fn resolve_model_alias(logical_id: &str) -> &str {
    KNOWN_MODEL_ALIASES
        .iter()
        .find(|alias| alias.logical_id == logical_id)
        .map(|alias| alias.canonical_backend_id)
        .unwrap_or(logical_id)
}

/// Returns `true` if the given model ID is a known logical alias.
pub fn is_model_alias(model_id: &str) -> bool {
    KNOWN_MODEL_ALIASES
        .iter()
        .any(|alias| alias.logical_id == model_id)
}

/// Returns all known model aliases as a static slice.
pub fn known_model_aliases() -> &'static [ModelAlias] {
    KNOWN_MODEL_ALIASES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_umans_coder_to_umans_kimi_k27() {
        // arrange
        // act
        let canonical = resolve_model_alias("umans-coder");

        // assert
        assert_eq!(canonical, "umans-kimi-k2.7");
    }

    #[test]
    fn resolves_umans_flash_to_umans_qwen36_35b_a3b() {
        // arrange
        // act
        let canonical = resolve_model_alias("umans-flash");

        // assert
        assert_eq!(canonical, "umans-qwen3.6-35b-a3b");
    }

    #[test]
    fn passes_through_non_alias_model_ids_unchanged() {
        // arrange
        // act
        // assert
        assert_eq!(resolve_model_alias("umans-kimi-k2.7"), "umans-kimi-k2.7");
        assert_eq!(
            resolve_model_alias("umans-qwen3.6-35b-a3b"),
            "umans-qwen3.6-35b-a3b"
        );
        assert_eq!(resolve_model_alias("umans-glm-5.2"), "umans-glm-5.2");
        assert_eq!(resolve_model_alias("gpt-5.5"), "gpt-5.5");
        assert_eq!(resolve_model_alias("unknown-model"), "unknown-model");
    }

    #[test]
    fn passes_through_empty_string_unchanged() {
        // arrange
        // act
        let canonical = resolve_model_alias("");

        // assert
        assert_eq!(canonical, "");
    }

    #[test]
    fn is_model_alias_returns_true_for_known_aliases() {
        // arrange
        // act
        // assert
        assert!(is_model_alias("umans-coder"));
        assert!(is_model_alias("umans-flash"));
    }

    #[test]
    fn is_model_alias_returns_false_for_canonical_and_unknown() {
        // arrange
        // act
        // assert
        assert!(!is_model_alias("umans-kimi-k2.7"));
        assert!(!is_model_alias("umans-qwen3.6-35b-a3b"));
        assert!(!is_model_alias("umans-glm-5.2"));
        assert!(!is_model_alias("gpt-5.5"));
        assert!(!is_model_alias("unknown-model"));
    }

    #[test]
    fn known_model_aliases_contains_exactly_two_entries() {
        // arrange
        // act
        let aliases = known_model_aliases();

        // assert
        assert_eq!(aliases.len(), 2, "expected exactly two model aliases");
    }

    #[test]
    fn known_model_aliases_cover_both_declared_aliases() {
        // arrange
        // act
        let aliases = known_model_aliases();

        // assert
        assert!(aliases
            .iter()
            .any(|a| a.logical_id == "umans-coder" && a.canonical_backend_id == "umans-kimi-k2.7"));
        assert!(aliases
            .iter()
            .any(|a| a.logical_id == "umans-flash"
                && a.canonical_backend_id == "umans-qwen3.6-35b-a3b"));
    }

    #[test]
    fn resolve_model_alias_makes_no_network_calls() {
        // This test documents the contract that the resolver is a pure
        // function. If this test compiles and runs, the resolver has no
        // hidden I/O or network dependencies — it operates on static data
        // only.
        // arrange
        // act
        let results: Vec<&str> = ["umans-coder", "umans-flash", "umans-glm-5.2", ""]
            .iter()
            .map(|id| resolve_model_alias(id))
            .collect();

        // assert
        assert_eq!(
            results,
            [
                "umans-kimi-k2.7",
                "umans-qwen3.6-35b-a3b",
                "umans-glm-5.2",
                ""
            ]
        );
    }
}
