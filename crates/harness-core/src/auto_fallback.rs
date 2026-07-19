//! Provider model auto-fallback resolution on error.
//!
//! Uses the config-level `model_profile.fallback` chain already resolved into
//! [`ResolvedModelSelection`]. Same-model transport retries stay separate
//! (`provider_retry`); this module picks the *next model* after those exhaust.

use crate::config::{ResolvedModelSelection, ResolvedModelTarget};
use serde::{Deserialize, Serialize};

/// Outcome of selecting the next fallback after a model error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoFallbackOutcome {
    /// Switch to this model target next.
    Next {
        failed_model_ref: String,
        next: ResolvedModelTarget,
        remaining_after: usize,
    },
    /// Fallback chain exhausted (no further targets).
    Exhausted {
        failed_model_ref: String,
        tried: Vec<String>,
    },
}

impl AutoFallbackOutcome {
    pub const fn is_next(&self) -> bool {
        matches!(self, Self::Next { .. })
    }

    pub const fn is_exhausted(&self) -> bool {
        matches!(self, Self::Exhausted { .. })
    }
}

/// Resolve the next fallback model after `failed_model_ref` errors.
///
/// Walks `primary` then `fallback[]` in order. When `failed_model_ref` matches
/// the primary, returns `fallback[0]` if present. When it matches
/// `fallback[i]`, returns `fallback[i+1]` if present. Unknown failed refs are
/// treated as primary failure when the chain is non-empty.
pub fn resolve_next_fallback(
    selection: &ResolvedModelSelection,
    failed_model_ref: &str,
) -> AutoFallbackOutcome {
    let failed = failed_model_ref.trim();
    let mut chain: Vec<&ResolvedModelTarget> = Vec::with_capacity(1 + selection.fallback.len());
    chain.push(&selection.primary);
    chain.extend(selection.fallback.iter());

    let failed_index = chain
        .iter()
        .position(|target| target.model_ref == failed)
        .unwrap_or(0);

    let tried: Vec<String> = chain
        .iter()
        .take(failed_index.saturating_add(1))
        .map(|target| target.model_ref.clone())
        .collect();

    let next_index = failed_index.saturating_add(1);
    match chain.get(next_index) {
        Some(next) => AutoFallbackOutcome::Next {
            failed_model_ref: chain[failed_index].model_ref.clone(),
            next: (*next).clone(),
            remaining_after: chain.len().saturating_sub(next_index.saturating_add(1)),
        },
        None => AutoFallbackOutcome::Exhausted {
            failed_model_ref: if failed.is_empty() {
                selection.primary.model_ref.clone()
            } else {
                failed.to_string()
            },
            tried,
        },
    }
}

/// Remaining model refs after `current_model_ref` in a resolved selection chain.
///
/// When `current_model_ref` is unknown, returns the full fallback list (treat as
/// primary failure). Used to seed turn-level auto-fallback queues.
pub fn remaining_fallback_model_refs(
    selection: &ResolvedModelSelection,
    current_model_ref: &str,
) -> Vec<String> {
    match resolve_next_fallback(selection, current_model_ref) {
        AutoFallbackOutcome::Next {
            next,
            remaining_after,
            ..
        } => {
            let mut out = Vec::with_capacity(remaining_after.saturating_add(1));
            out.push(next.model_ref.clone());
            let mut cursor = next.model_ref;
            for _ in 0..remaining_after {
                match resolve_next_fallback(selection, &cursor) {
                    AutoFallbackOutcome::Next { next, .. } => {
                        cursor = next.model_ref.clone();
                        out.push(next.model_ref);
                    }
                    AutoFallbackOutcome::Exhausted { .. } => break,
                }
            }
            out
        }
        AutoFallbackOutcome::Exhausted { .. } => Vec::new(),
    }
}

/// Pop the next fallback model ref from a turn-local queue.
pub fn take_next_fallback_model_ref(chain: &mut Vec<String>) -> Option<String> {
    if chain.is_empty() {
        None
    } else {
        Some(chain.remove(0))
    }
}

/// Whether a terminal turn failure stage is eligible for model auto-fallback.
///
/// Same-model transport retries are handled first by `provider_retry`. After
/// those exhaust (or for non-retryable provider errors), model fallback may run.
pub fn is_provider_failure_fallback_eligible(failure_stage: &str) -> bool {
    failure_stage == "provider_error"
}

/// Canonical operator banner for a successful model switch.
///
/// Matches the status-dialog / event-ingest shape: `provider fallback: A → B`.
pub fn format_auto_fallback_banner(
    failed_model_ref: impl AsRef<str>,
    next_model_ref: impl AsRef<str>,
) -> String {
    format!(
        "provider fallback: {} → {}",
        failed_model_ref.as_ref().trim(),
        next_model_ref.as_ref().trim()
    )
}

/// Operator-facing one-line description of a fallback resolution outcome.
pub fn describe_auto_fallback_outcome(outcome: &AutoFallbackOutcome) -> String {
    match outcome {
        AutoFallbackOutcome::Next {
            failed_model_ref,
            next,
            remaining_after,
        } => {
            let mut line = format_auto_fallback_banner(failed_model_ref, &next.model_ref);
            if *remaining_after > 0 {
                line.push_str(&format!(" ({remaining_after} remaining)"));
            }
            line
        }
        AutoFallbackOutcome::Exhausted {
            failed_model_ref,
            tried,
        } => {
            format!(
                "provider fallback exhausted after {} (tried: {})",
                failed_model_ref.trim(),
                if tried.is_empty() {
                    "none".to_string()
                } else {
                    tried.join(" → ")
                }
            )
        }
    }
}

/// Compact remaining-chain label for operator diagnostics (primary first).
pub fn format_fallback_chain_label(selection: &ResolvedModelSelection) -> String {
    let mut refs = Vec::with_capacity(1 + selection.fallback.len());
    refs.push(selection.primary.model_ref.clone());
    refs.extend(selection.fallback.iter().map(|t| t.model_ref.clone()));
    refs.join(" → ")
}

/// Operator-facing counts for a resolved auto-fallback chain (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AutoFallbackSummary {
    pub remaining: usize,
    pub chain_len: usize,
    /// True when no further fallback models remain after `current_model_ref`.
    pub exhausted: bool,
}

impl AutoFallbackSummary {
    pub fn one_line(&self) -> String {
        format!(
            "fallback chain: {} remaining of {} (exhausted={})",
            self.remaining, self.chain_len, self.exhausted
        )
    }

    pub const fn has_remaining(&self) -> bool {
        self.remaining > 0
    }
}

/// Summarize remaining fallback capacity from a resolved selection + current model.
pub fn summarize_auto_fallback(
    selection: &ResolvedModelSelection,
    current_model_ref: &str,
) -> AutoFallbackSummary {
    let remaining = remaining_fallback_model_refs(selection, current_model_ref);
    let chain_len = 1usize.saturating_add(selection.fallback.len());
    AutoFallbackSummary {
        remaining: remaining.len(),
        chain_len,
        exhausted: remaining.is_empty(),
    }
}

/// One recorded step while walking a multi-model fallback chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackChainStep {
    /// Model that failed at this step.
    pub failed_model_ref: String,
    /// Resolution outcome after that failure.
    pub outcome: AutoFallbackOutcome,
    /// Remaining models after this step (0 when Exhausted or last Next).
    pub remaining_after: usize,
}

/// Full multi-fallback chain walk: primary→fb1→…→Exhausted.
///
/// Product orchestration path (not seed-only). Records every step with remaining
/// counts, terminal summary, and chain label. Mirrors sequential model switches
/// after `provider_error` eligibility in the turn runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackChainWalk {
    pub steps: Vec<FallbackChainStep>,
    pub terminal_summary: AutoFallbackSummary,
    pub chain_label: String,
}

impl FallbackChainWalk {
    pub fn exhausted(&self) -> bool {
        self.steps
            .last()
            .is_some_and(|step| step.outcome.is_exhausted())
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Remaining counts after each step (same length as `steps`).
    pub fn remaining_counts(&self) -> Vec<usize> {
        self.steps.iter().map(|s| s.remaining_after).collect()
    }
}

/// Walk the full fallback chain from `start_model_ref` until Exhausted.
///
/// Each step calls [`resolve_next_fallback`]. On `Next`, continues from the
/// switched model; on `Exhausted`, stops. Terminal summary is taken at the last
/// failed model (exhausted=true when the chain has no further targets).
pub fn orchestrate_fallback_chain(
    selection: &ResolvedModelSelection,
    start_model_ref: &str,
) -> FallbackChainWalk {
    let chain_label = format_fallback_chain_label(selection);
    let chain_len = 1usize.saturating_add(selection.fallback.len());
    let mut steps = Vec::with_capacity(chain_len);
    let mut cursor = start_model_ref.trim().to_string();
    if cursor.is_empty() {
        cursor = selection.primary.model_ref.clone();
    }

    // Cap iterations to chain length + 1 so a malformed selection cannot loop.
    for _ in 0..=chain_len {
        let outcome = resolve_next_fallback(selection, &cursor);
        match &outcome {
            AutoFallbackOutcome::Next {
                failed_model_ref,
                next,
                remaining_after,
            } => {
                steps.push(FallbackChainStep {
                    failed_model_ref: failed_model_ref.clone(),
                    remaining_after: *remaining_after,
                    outcome: outcome.clone(),
                });
                cursor = next.model_ref.clone();
            }
            AutoFallbackOutcome::Exhausted {
                failed_model_ref, ..
            } => {
                steps.push(FallbackChainStep {
                    failed_model_ref: failed_model_ref.clone(),
                    remaining_after: 0,
                    outcome,
                });
                break;
            }
        }
    }

    let terminal_model = steps
        .last()
        .map(|s| s.failed_model_ref.as_str())
        .unwrap_or(start_model_ref);
    let terminal_summary = summarize_auto_fallback(selection, terminal_model);

    FallbackChainWalk {
        steps,
        terminal_summary,
        chain_label,
    }
}

/// Runtime-shaped provider-failure orchestration.
///
/// When `failure_stage` is fallback-eligible, drains the remaining-model queue
/// (same semantics as `agent_turn_runtime` + [`take_next_fallback_model_ref`])
/// and records each switch until the queue is empty (exhausted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailureFallbackOrchestration {
    /// Stage is not eligible for model auto-fallback.
    NotEligible { failure_stage: String },
    /// Queue drained with zero or more model switches; always ends exhausted.
    Drained {
        /// Ordered (failed_model, next_model) switches performed.
        switches: Vec<(String, String)>,
        /// Remaining queue length after each switch (always 0 after final).
        remaining_after_each: Vec<usize>,
        summary: AutoFallbackSummary,
        chain_label: String,
    },
}

impl ProviderFailureFallbackOrchestration {
    pub const fn is_not_eligible(&self) -> bool {
        matches!(self, Self::NotEligible { .. })
    }

    pub const fn is_drained(&self) -> bool {
        matches!(self, Self::Drained { .. })
    }

    pub fn exhausted(&self) -> bool {
        match self {
            Self::NotEligible { .. } => false,
            Self::Drained { summary, .. } => summary.exhausted,
        }
    }
}

/// Orchestrate provider-failure model fallback for a resolved selection.
///
/// Product path used by tests and diagnostics; turn runtime uses the same
/// eligibility + queue-drain primitives.
pub fn orchestrate_provider_failure_fallback(
    selection: &ResolvedModelSelection,
    current_model_ref: &str,
    failure_stage: &str,
) -> ProviderFailureFallbackOrchestration {
    if !is_provider_failure_fallback_eligible(failure_stage) {
        return ProviderFailureFallbackOrchestration::NotEligible {
            failure_stage: failure_stage.to_string(),
        };
    }

    let chain_label = format_fallback_chain_label(selection);
    let mut queue = remaining_fallback_model_refs(selection, current_model_ref);
    let mut switches = Vec::with_capacity(queue.len());
    let mut remaining_after_each = Vec::with_capacity(queue.len());
    let mut current = current_model_ref.trim().to_string();
    if current.is_empty() {
        current = selection.primary.model_ref.clone();
    }

    while let Some(next) = take_next_fallback_model_ref(&mut queue) {
        switches.push((current.clone(), next.clone()));
        remaining_after_each.push(queue.len());
        current = next;
    }

    let summary = summarize_auto_fallback(selection, &current);
    ProviderFailureFallbackOrchestration::Drained {
        switches,
        remaining_after_each,
        summary,
        chain_label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_resolution::ModelResolution;

    fn target(model_ref: &str) -> ResolvedModelTarget {
        ResolvedModelTarget {
            model_ref: model_ref.to_string(),
            provider: "mock".into(),
            model: model_ref
                .rsplit_once(':')
                .map(|(_, m)| m)
                .unwrap_or(model_ref)
                .into(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            resolution: ModelResolution::default(),
        }
    }

    fn selection(primary: &str, fallbacks: &[&str]) -> ResolvedModelSelection {
        ResolvedModelSelection {
            selector: "profile".into(),
            profile: Some("profile".into()),
            primary: target(primary),
            fallback: fallbacks.iter().map(|m| target(m)).collect(),
        }
    }

    #[test]
    fn primary_failure_selects_first_fallback() {
        // arrange
        // act
        // assert
        // Given
        let sel = selection("p:main", &["p:fb1", "p:fb2"]);

        // When
        let outcome = resolve_next_fallback(&sel, "p:main");

        // Then
        match outcome {
            AutoFallbackOutcome::Next {
                next,
                remaining_after,
                ..
            } => {
                assert_eq!(next.model_ref, "p:fb1");
                assert_eq!(remaining_after, 1);
            }
            other => panic!("expected Next, got {other:?}"),
        }
    }

    #[test]
    fn mid_chain_failure_selects_next_fallback() {
        // arrange
        // act
        // assert
        let sel = selection("p:main", &["p:fb1", "p:fb2"]);
        let outcome = resolve_next_fallback(&sel, "p:fb1");
        match outcome {
            AutoFallbackOutcome::Next {
                next,
                remaining_after,
                ..
            } => {
                assert_eq!(next.model_ref, "p:fb2");
                assert_eq!(remaining_after, 0);
            }
            other => panic!("expected Next, got {other:?}"),
        }
    }

    #[test]
    fn last_fallback_failure_is_exhausted() {
        // arrange
        // act
        // assert
        let sel = selection("p:main", &["p:fb1"]);
        let outcome = resolve_next_fallback(&sel, "p:fb1");
        match outcome {
            AutoFallbackOutcome::Exhausted { tried, .. } => {
                assert_eq!(tried, vec!["p:main".to_string(), "p:fb1".to_string()]);
            }
            other => panic!("expected Exhausted, got {other:?}"),
        }
    }

    #[test]
    fn empty_fallback_chain_exhausts_on_primary_error() {
        // arrange
        // act
        // assert
        let sel = selection("p:only", &[]);
        let outcome = resolve_next_fallback(&sel, "p:only");
        assert!(outcome.is_exhausted());
    }

    #[test]
    fn remaining_fallback_model_refs_lists_tail_after_primary() {
        // arrange
        // act
        // assert
        // Given
        let sel = selection("p:main", &["p:fb1", "p:fb2"]);

        // When / Then
        assert_eq!(
            remaining_fallback_model_refs(&sel, "p:main"),
            vec!["p:fb1".to_string(), "p:fb2".to_string()]
        );
        assert_eq!(
            remaining_fallback_model_refs(&sel, "p:fb1"),
            vec!["p:fb2".to_string()]
        );
        assert!(remaining_fallback_model_refs(&sel, "p:fb2").is_empty());
    }

    #[test]
    fn take_next_fallback_model_ref_drains_queue() {
        // arrange
        // act
        // assert
        let mut chain = vec!["p:fb1".to_string(), "p:fb2".to_string()];
        assert_eq!(
            take_next_fallback_model_ref(&mut chain).as_deref(),
            Some("p:fb1")
        );
        assert_eq!(
            take_next_fallback_model_ref(&mut chain).as_deref(),
            Some("p:fb2")
        );
        assert_eq!(take_next_fallback_model_ref(&mut chain), None);
    }

    #[test]
    fn provider_error_stage_is_fallback_eligible() {
        // arrange
        // act
        // assert
        assert!(is_provider_failure_fallback_eligible("provider_error"));
        assert!(!is_provider_failure_fallback_eligible("cancelled"));
        assert!(!is_provider_failure_fallback_eligible(
            "overflow_retry_failed"
        ));
    }

    #[test]
    fn format_auto_fallback_banner_matches_operator_shape() {
        // arrange
        // act
        // assert
        // Given / When
        let banner = format_auto_fallback_banner("model-a", "model-b");

        // Then
        assert_eq!(banner, "provider fallback: model-a → model-b");
    }

    #[test]
    fn describe_auto_fallback_outcome_covers_next_and_exhausted() {
        // arrange
        // act
        // assert
        // Given
        let sel = selection("p:main", &["p:fb1", "p:fb2"]);
        let next = resolve_next_fallback(&sel, "p:main");
        let exhausted = resolve_next_fallback(&sel, "p:fb2");

        // When / Then next
        let next_line = describe_auto_fallback_outcome(&next);
        assert!(next_line.starts_with("provider fallback: p:main → p:fb1"));
        assert!(next_line.contains("1 remaining"));

        // When / Then exhausted
        let exhausted_line = describe_auto_fallback_outcome(&exhausted);
        assert!(exhausted_line.contains("exhausted"));
        assert!(exhausted_line.contains("p:main → p:fb1 → p:fb2"));
    }

    #[test]
    fn format_fallback_chain_label_lists_primary_then_fallbacks() {
        // arrange
        // act
        // assert
        // Given
        let sel = selection("p:main", &["p:fb1", "p:fb2"]);

        // When
        let label = format_fallback_chain_label(&sel);

        // Then
        assert_eq!(label, "p:main → p:fb1 → p:fb2");
    }

    #[test]
    fn auto_fallback_summary_one_line_and_remaining_counts() {
        // arrange
        // act
        // assert
        // Given
        let sel = selection("p:main", &["p:fb1", "p:fb2"]);

        // When
        let mid = summarize_auto_fallback(&sel, "p:main");
        let last = summarize_auto_fallback(&sel, "p:fb2");

        // Then
        assert_eq!(
            mid,
            AutoFallbackSummary {
                remaining: 2,
                chain_len: 3,
                exhausted: false,
            }
        );
        assert!(mid.has_remaining());
        assert!(mid.one_line().contains("2 remaining of 3"));
        assert!(mid.one_line().contains("exhausted=false"));
        assert_eq!(
            last,
            AutoFallbackSummary {
                remaining: 0,
                chain_len: 3,
                exhausted: true,
            }
        );
        assert!(!last.has_remaining());
        assert!(last.one_line().contains("exhausted=true"));
    }

    #[test]
    fn orchestrate_fallback_chain_walks_primary_through_exhaustion_with_remaining() {
        // arrange
        // act
        // assert
        // Given: primary → fb1 → fb2 → fb3 → fb4
        let sel = selection("p:main", &["p:fb1", "p:fb2", "p:fb3", "p:fb4"]);

        // When
        let walk = orchestrate_fallback_chain(&sel, "p:main");

        // Then: 4 Next steps + 1 Exhausted
        assert_eq!(walk.step_count(), 5);
        assert!(walk.exhausted());
        assert_eq!(walk.remaining_counts(), vec![3, 2, 1, 0, 0]);
        assert_eq!(walk.terminal_summary.remaining, 0);
        assert_eq!(walk.terminal_summary.chain_len, 5);
        assert!(walk.terminal_summary.exhausted);
        assert_eq!(walk.chain_label, "p:main → p:fb1 → p:fb2 → p:fb3 → p:fb4");
        assert!(walk.steps[0].outcome.is_next());
        assert!(walk.steps[3].outcome.is_next());
        assert!(walk.steps[4].outcome.is_exhausted());
        match &walk.steps[4].outcome {
            AutoFallbackOutcome::Exhausted { tried, .. } => {
                assert_eq!(
                    tried,
                    &vec![
                        "p:main".to_string(),
                        "p:fb1".to_string(),
                        "p:fb2".to_string(),
                        "p:fb3".to_string(),
                        "p:fb4".to_string(),
                    ]
                );
            }
            other => panic!("expected terminal Exhausted, got {other:?}"),
        }
    }

    #[test]
    fn orchestrate_provider_failure_fallback_drains_queue_until_exhausted() {
        // arrange
        // act
        // assert
        // Given
        let sel = selection("p:main", &["p:fb1", "p:fb2", "p:fb3"]);

        // When: eligible provider_error drains remaining queue
        let drained = orchestrate_provider_failure_fallback(&sel, "p:main", "provider_error");

        // Then
        match drained {
            ProviderFailureFallbackOrchestration::Drained {
                switches,
                remaining_after_each,
                summary,
                chain_label,
            } => {
                assert_eq!(
                    switches,
                    vec![
                        ("p:main".to_string(), "p:fb1".to_string()),
                        ("p:fb1".to_string(), "p:fb2".to_string()),
                        ("p:fb2".to_string(), "p:fb3".to_string()),
                    ]
                );
                assert_eq!(remaining_after_each, vec![2, 1, 0]);
                assert_eq!(summary.remaining, 0);
                assert_eq!(summary.chain_len, 4);
                assert!(summary.exhausted);
                assert!(chain_label.contains("p:main"));
                assert!(chain_label.contains("p:fb3"));
            }
            other => panic!("expected Drained, got {other:?}"),
        }

        // When: non-eligible stage
        let blocked = orchestrate_provider_failure_fallback(&sel, "p:main", "cancelled");
        assert!(blocked.is_not_eligible());
        assert!(!blocked.exhausted());
    }
}
