use crate::config::{ResolvedModelSelection, ResolvedModelTarget};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoFallbackOutcome {
    Next {
        failed_model_ref: String,
        next: Box<ResolvedModelTarget>,
        remaining_after: usize,
    },
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
fn position(selection: &ResolvedModelSelection, current: &str) -> usize {
    if current.trim() == selection.primary.model_ref {
        return 0;
    }
    selection
        .fallback
        .iter()
        .position(|t| t.model_ref == current.trim())
        .map_or(0, |n| n + 1)
}
pub fn resolve_next_fallback(
    selection: &ResolvedModelSelection,
    failed_model_ref: &str,
) -> AutoFallbackOutcome {
    let index = position(selection, failed_model_ref);
    let failed = if index == 0 {
        &selection.primary.model_ref
    } else {
        &selection.fallback[index - 1].model_ref
    };
    match selection.fallback.get(index) {
        Some(next) => AutoFallbackOutcome::Next {
            failed_model_ref: failed.clone(),
            next: Box::new(next.clone()),
            remaining_after: selection.fallback.len() - index - 1,
        },
        None => AutoFallbackOutcome::Exhausted {
            failed_model_ref: if failed_model_ref.trim().is_empty() {
                failed.clone()
            } else {
                failed_model_ref.trim().into()
            },
            tried: std::iter::once(&selection.primary)
                .chain(selection.fallback[..index].iter())
                .map(|t| t.model_ref.clone())
                .collect(),
        },
    }
}
pub fn remaining_fallback_model_refs(
    selection: &ResolvedModelSelection,
    current_model_ref: &str,
) -> Vec<String> {
    selection.fallback[position(selection, current_model_ref)..]
        .iter()
        .map(|t| t.model_ref.clone())
        .collect()
}
// ponytail: these Vec compatibility APIs shift short lists; the turn loop uses VecDeque.
pub fn take_next_fallback_model_ref(chain: &mut Vec<String>) -> Option<String> {
    (!chain.is_empty()).then(|| chain.remove(0))
}
pub fn take_next_fallback_model_target(
    chain: &mut Vec<ResolvedModelTarget>,
) -> Option<ResolvedModelTarget> {
    (!chain.is_empty()).then(|| chain.remove(0))
}
pub fn is_provider_failure_fallback_eligible(stage: &str) -> bool {
    stage == "provider_error"
}
pub fn format_auto_fallback_banner(failed: impl AsRef<str>, next: impl AsRef<str>) -> String {
    format!(
        "provider fallback: {} → {}",
        failed.as_ref().trim(),
        next.as_ref().trim()
    )
}
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
        } => format!(
            "provider fallback exhausted after {} (tried: {})",
            failed_model_ref.trim(),
            if tried.is_empty() {
                "none".into()
            } else {
                tried.join(" → ")
            }
        ),
    }
}
pub fn format_fallback_chain_label(selection: &ResolvedModelSelection) -> String {
    std::iter::once(&selection.primary)
        .chain(selection.fallback.iter())
        .map(|t| t.model_ref.as_str())
        .collect::<Vec<_>>()
        .join(" → ")
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoFallbackSummary {
    pub remaining: usize,
    pub chain_len: usize,
    pub exhausted: bool,
}
impl AutoFallbackSummary {
    pub const fn has_remaining(&self) -> bool {
        self.remaining > 0
    }
    pub fn one_line(&self) -> String {
        format!(
            "fallback chain: {} remaining of {} (exhausted={})",
            self.remaining, self.chain_len, self.exhausted
        )
    }
}
pub fn summarize_auto_fallback(
    selection: &ResolvedModelSelection,
    current: &str,
) -> AutoFallbackSummary {
    let remaining = selection.fallback.len() - position(selection, current);
    AutoFallbackSummary {
        remaining,
        chain_len: selection.fallback.len() + 1,
        exhausted: remaining == 0,
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackChainStep {
    pub failed_model_ref: String,
    pub outcome: AutoFallbackOutcome,
    pub remaining_after: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackChainWalk {
    pub steps: Vec<FallbackChainStep>,
    pub terminal_summary: AutoFallbackSummary,
    pub chain_label: String,
}
impl FallbackChainWalk {
    pub fn exhausted(&self) -> bool {
        self.steps.last().is_some_and(|s| s.outcome.is_exhausted())
    }
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
    pub fn remaining_counts(&self) -> Vec<usize> {
        self.steps.iter().map(|s| s.remaining_after).collect()
    }
}
pub fn orchestrate_fallback_chain(
    selection: &ResolvedModelSelection,
    start: &str,
) -> FallbackChainWalk {
    let index = position(selection, start);
    let mut current = if index == 0 {
        &selection.primary.model_ref
    } else {
        &selection.fallback[index - 1].model_ref
    };
    let mut steps = Vec::new();
    for (offset, next) in selection.fallback.iter().enumerate().skip(index) {
        let remaining_after = selection.fallback.len() - offset - 1;
        let outcome = AutoFallbackOutcome::Next {
            failed_model_ref: current.clone(),
            next: Box::new(next.clone()),
            remaining_after,
        };
        steps.push(FallbackChainStep {
            failed_model_ref: current.clone(),
            outcome,
            remaining_after,
        });
        current = &next.model_ref;
    }
    steps.push(FallbackChainStep {
        failed_model_ref: current.clone(),
        outcome: AutoFallbackOutcome::Exhausted {
            failed_model_ref: current.clone(),
            tried: std::iter::once(&selection.primary)
                .chain(selection.fallback.iter())
                .map(|t| t.model_ref.clone())
                .collect(),
        },
        remaining_after: 0,
    });
    FallbackChainWalk {
        steps,
        terminal_summary: AutoFallbackSummary {
            remaining: 0,
            chain_len: selection.fallback.len() + 1,
            exhausted: true,
        },
        chain_label: format_fallback_chain_label(selection),
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailureFallbackOrchestration {
    NotEligible {
        failure_stage: String,
    },
    Drained {
        switches: Vec<(String, String)>,
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
        matches!(self, Self::Drained { summary, .. } if summary.exhausted)
    }
}
pub fn orchestrate_provider_failure_fallback(
    selection: &ResolvedModelSelection,
    current: &str,
    stage: &str,
) -> ProviderFailureFallbackOrchestration {
    if !is_provider_failure_fallback_eligible(stage) {
        return ProviderFailureFallbackOrchestration::NotEligible {
            failure_stage: stage.into(),
        };
    }
    let walk = orchestrate_fallback_chain(selection, current);
    let (switches, remaining_after_each) = walk
        .steps
        .into_iter()
        .filter_map(|step| match step.outcome {
            AutoFallbackOutcome::Next {
                failed_model_ref,
                next,
                remaining_after,
            } => Some(((failed_model_ref, next.model_ref), remaining_after)),
            _ => None,
        })
        .unzip();
    ProviderFailureFallbackOrchestration::Drained {
        switches,
        remaining_after_each,
        summary: walk.terminal_summary,
        chain_label: walk.chain_label,
    }
}
