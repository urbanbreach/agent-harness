use std::collections::BTreeSet;

use crate::agent::{ProviderContext, ProviderConversationTurn};
use crate::conversation::ConversationMessage;
use crate::ids::EntryId;
use crate::session::{CanonicalProviderView, CanonicalSession, SessionEntryPayload};

pub(super) fn has_recoverable_provider_context(session: &CanonicalSession) -> bool {
    session.active_path().is_ok_and(|entries| {
        entries.iter().any(|entry| {
            matches!(
                entry.payload,
                SessionEntryPayload::AssistantMessage { .. }
                    | SessionEntryPayload::CompactionSummary { .. }
            )
        })
    })
}

pub(super) fn provider_context(
    view: CanonicalProviderView,
    events: &[crate::event::EventEnvelopeV1],
    agent_id: &str,
) -> Result<ProviderContext, String> {
    let kept_ids = kept_entry_ids(&view);
    let mut messages = crate::agent::canonical_recovery_messages(&view)
        .into_iter()
        .filter(|message| !matches!(message, ConversationMessage::Checkpoint(_)))
        .collect::<Vec<_>>();
    restore_legacy_request_ids(&mut messages, events, agent_id);
    let attachments = view
        .attachments
        .iter()
        .filter(|attachment| kept_ids.contains(&attachment.entry_id))
        .map(|attachment| attachment.attachment.clone())
        .collect::<Vec<_>>();
    let preserved_turns = (!messages.is_empty())
        .then_some(ProviderConversationTurn {
            messages,
            attachments,
            ..ProviderConversationTurn::default()
        })
        .into_iter()
        .collect();
    Ok(ProviderContext {
        compacted_summary: view
            .latest_compaction_summary
            .map(|compaction| compaction.summary),
        preserved_turns,
        checkpoint: None,
    })
}

fn restore_legacy_request_ids(
    messages: &mut [ConversationMessage],
    events: &[crate::event::EventEnvelopeV1],
    agent_id: &str,
) {
    let stream_key = format!("agent:{agent_id}");
    let mut request_ids = events
        .iter()
        .filter(|event| {
            event.actor.agent_id.as_deref() == Some(agent_id)
                || event.stream_key.as_deref() == Some(stream_key.as_str())
        })
        .filter_map(|event| match &event.payload {
            crate::event::EventV1::UserMessageSubmitted(user) => Some(user.request_id.clone()),
            _ => None,
        })
        .rev();
    for message in messages.iter_mut().rev() {
        let ConversationMessage::User(user) = message else {
            continue;
        };
        let Some(request_id) = request_ids.next() else {
            break;
        };
        user.request_id = request_id;
    }
}

fn kept_entry_ids(view: &CanonicalProviderView) -> BTreeSet<EntryId> {
    let Some(summary) = view.latest_compaction_summary.as_ref() else {
        return view.active_entry_ids.iter().cloned().collect();
    };
    let boundary = view
        .active_entry_ids
        .iter()
        .position(|entry_id| entry_id == &summary.first_kept_entry_id)
        .unwrap_or(0);
    view.active_entry_ids[boundary..]
        .iter()
        .filter(|entry_id| entry_id != &&summary.entry_id)
        .cloned()
        .collect()
}
