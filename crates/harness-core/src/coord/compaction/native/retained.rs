use super::super::super::context::Entry;
use harness_compaction::Origin;
use harness_providers::{CompletionMessage, MessageRole};

/// The messages lane V2 may keep verbatim: what the human typed. Reminders and other
/// harness notes, steering from other agents, branch summaries and compaction summaries are
/// never retained, even though they reach the provider as user messages.
pub(super) fn retained_messages(entries: &[Entry]) -> Vec<CompletionMessage> {
    entries
        .iter()
        .filter(|e| {
            e.message.role == MessageRole::User
                && e.origin == Origin::User
                && e.note_kind.is_none()
                && e.compaction.is_none()
                && e.branch_summary.is_none()
        })
        .map(|e| e.message.clone())
        .collect()
}
