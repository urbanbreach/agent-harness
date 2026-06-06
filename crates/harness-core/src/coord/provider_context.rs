use crate::event::{HookExecutionMetadata, HookExecutionStatus};
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};

mod model_summary;
mod operational_memory;
mod planning;
mod restore;
mod summary;
mod tokens;

#[cfg(test)]
pub(super) use model_summary::{build_model_compaction_prompt, validate_model_compaction_summary};
pub(super) use model_summary::{
    compaction_summary_model_ref, model_backed_compaction_summary_for, ModelBackedCompactionSummary,
};
use operational_memory::build_provider_compaction_facts;
#[cfg(test)]
pub(super) use planning::ProviderContextCompactionPlan;
pub(super) use planning::{
    serialize_provider_context_checkpoint, CompactionSummaryDecision, ProviderCompactionTrigger,
    ProviderContextCompactionRequest,
};
pub(super) use restore::restore_provider_context_from_history;
#[cfg(test)]
pub(super) use summary::{
    build_provider_context_summary, provider_context_summary_required_headings,
};
use tokens::summarize_compaction_text;
pub(super) use tokens::{approximate_provider_context_tokens, approximate_text_tokens};

const PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS: u32 = 1_024;
const PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MAX_TOKENS: u32 = 8_000;
const PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MIN_TOKENS: u32 = 2_000;
const PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS: usize = 6_000;
pub(super) const PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS: usize = 240;
const PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS: usize = 1_200;
pub(super) const PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION: u32 = 2;
const PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_HEADINGS: &[&str] = &[
    "## Original Request",
    "## Early Progress",
    "## Context for Suffix",
];
const PROVIDER_CONTEXT_HARNESS_SUMMARY_HEADINGS: &[&str] = &[
    "## Goal",
    "## Constraints",
    "## Progress",
    "## Key Decisions",
    "## Next Steps",
    "## Critical Context",
];
const PROVIDER_CONTEXT_LEGACY_SUMMARY_HEADINGS: &[&str] = &[
    "## Goal",
    "## Constraints & Preferences",
    "## Progress",
    "### Done",
    "### In Progress",
    "### Blocked",
    "## Key Decisions",
    "## Next Steps",
    "## Critical Context",
    "## Source Facts",
    "## Relevant Files / Artifacts",
];

pub(super) fn truncated_failure_reason(reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        None
    } else {
        Some(truncate_with_ellipsis(
            reason,
            PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS,
        ))
    }
}

pub(super) fn compaction_summary_override_from_hooks(
    hook_executions: &[HookExecutionMetadata],
) -> Option<String> {
    hook_executions.iter().rev().find_map(|execution| {
        if execution.status != HookExecutionStatus::Succeeded {
            return None;
        }
        let summary = execution.output_summary.as_deref()?.trim();
        summary
            .strip_prefix("compaction_summary:")
            .and_then(non_empty_trimmed)
            .map(ToOwned::to_owned)
    })
}

pub(super) fn is_provider_context_overflow_reason(reason: &str) -> bool {
    let normalized = reason.to_ascii_lowercase();
    [
        "context length",
        "context window",
        "too many tokens",
        "prompt token count",
        "maximum context",
        "input token",
        "reduce the length",
        "token count of",
        "exceeds the limit",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_failure_reason_omits_blank_input_after_trimming() {
        assert_eq!(truncated_failure_reason(""), None);
        assert_eq!(truncated_failure_reason(" \n\t "), None);
    }

    #[test]
    fn truncated_failure_reason_trims_non_empty_input() {
        assert_eq!(
            truncated_failure_reason("  provider failed closed  ").as_deref(),
            Some("provider failed closed")
        );
    }

    #[test]
    fn truncated_failure_reason_caps_long_input_with_ellipsis() {
        let long_reason = "x".repeat(PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS + 1);
        let reason = truncated_failure_reason(&long_reason).expect("truncated reason");

        assert_eq!(
            reason.chars().count(),
            PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS + 1
        );
        assert!(reason.ends_with('…'));
    }
}
