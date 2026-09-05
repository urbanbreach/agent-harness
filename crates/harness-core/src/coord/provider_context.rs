use crate::agent::{ProviderContext, ProviderConversationTurn};
use crate::attachment_transport::AttachmentMetadata;
use crate::conversation::ConversationMessage;
use crate::text::truncate_with_ellipsis;

mod committed;
mod history;
mod restore;

pub(super) use committed::{
    event_belongs_to_agent, latest_agent_event_seq, project_committed_context,
    reconstruct_provider_context_from_events,
};
pub(super) use restore::{
    recover_canonical_provider_context_from_events,
    recover_canonical_provider_context_from_history,
    recover_canonical_provider_context_from_history_with_fallbacks,
    restore_provider_context_from_history, CanonicalProviderRecovery, RecoveredProviderContext,
};

pub(super) const PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS: usize = 240;

fn build_provider_context(
    messages: Vec<ConversationMessage>,
    attachments: Vec<AttachmentMetadata>,
    compacted_summary: Option<String>,
) -> ProviderContext {
    let preserved_turns = (!messages.is_empty())
        .then_some(ProviderConversationTurn {
            messages,
            attachments,
            ..ProviderConversationTurn::default()
        })
        .into_iter()
        .collect();
    ProviderContext {
        compacted_summary,
        preserved_turns,
        checkpoint: None,
    }
}

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
