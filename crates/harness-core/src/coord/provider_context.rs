use crate::text::truncate_with_ellipsis;

mod restore;

pub(super) use restore::restore_provider_context_from_history;

pub(super) const PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS: usize = 240;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

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
        let reason = truncated_failure_reason(&long_reason).unwrap_or_abort();

        assert_eq!(
            reason.chars().count(),
            PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS + 1
        );
        assert!(reason.ends_with('…'));
    }
}
