use crate::agent::{ProviderContext, ProviderConversationTurn};
use crate::conversation::ConversationMessage;
use crate::session::{AssistantPart, CanonicalProviderView, SessionEntryPayload};

pub(super) fn conversation_messages_with_transient_overlay(
    view: &CanonicalProviderView,
    transient_turns: &[ProviderConversationTurn],
) -> Vec<ConversationMessage> {
    let transient_turns = distinct_transient_turns(transient_turns);
    let superseded_turn_ids = superseded_turn_ids(view, &transient_turns);
    let overlay_contains_pending = view.pending_prompt.as_ref().is_some_and(|prompt| {
        let pending_text = crate::attachment_transport::lower_provider_attachments(
            &prompt.text,
            &prompt.attachments,
        );
        transient_turns.iter().any(|turn| {
            turn.user_prompt == prompt.text
                || turn.messages.iter().any(|message| {
                    matches!(
                        message,
                        ConversationMessage::User(user) if user.text == pending_text
                    )
                })
        })
    });
    let mut messages =
        super::conversation_messages_excluding(view, &superseded_turn_ids, false, true);
    messages.extend(super::super::super::project_provider_context(
        &ProviderContext::from_turns(transient_turns),
    ));
    if view.pending_prompt.as_ref().is_some_and(|prompt| {
        !superseded_turn_ids.contains(prompt.turn_id.as_str())
            && !overlay_contains_pending
            && !pending_already_projected(view, prompt, &superseded_turn_ids)
    }) {
        messages.extend(super::pending_prompt_message(view));
    }
    messages
}

fn pending_already_projected(
    view: &CanonicalProviderView,
    pending: &crate::session::CanonicalPendingPrompt,
    excluded_turn_ids: &std::collections::BTreeSet<String>,
) -> bool {
    view.entries.iter().any(|entry| {
        entry.turn_id.as_ref() == Some(&pending.turn_id)
            && !excluded_turn_ids.contains(pending.turn_id.as_str())
    }) || view.entries.iter().rposition(|entry| {
        matches!(
            &entry.payload,
            SessionEntryPayload::UserMessage { text, .. } if text == &pending.text
        )
    }).is_some_and(|user_index| {
        !view.entries[user_index.saturating_add(1)..].iter().any(|entry| {
            matches!(
                &entry.payload,
                SessionEntryPayload::AssistantMessage { parts, .. }
                    if parts.iter().any(|part| matches!(part, AssistantPart::Text { text } if !text.is_empty()))
            )
        })
    })
}

fn superseded_turn_ids(
    view: &CanonicalProviderView,
    transient_turns: &[ProviderConversationTurn],
) -> std::collections::BTreeSet<String> {
    let mut superseded = std::collections::BTreeSet::new();
    for transient in transient_turns {
        let logical_turn_id = logical_turn_id(transient);
        let request_id = transient.request_id.as_ref().map(ToString::to_string);
        let typed_match = view.entries.iter().find_map(|entry| {
            let turn_id = entry.turn_id.as_ref()?;
            let direct_match = logical_turn_id.as_deref() == Some(turn_id.as_str());
            let provenance_match = matches!(
                (&entry.payload, request_id.as_deref()),
                (
                    SessionEntryPayload::AssistantMessage {
                        provenance: Some(provenance),
                        ..
                    },
                    Some(request_id)
                ) if provenance.request_id.as_str() == request_id
            );
            (direct_match || provenance_match).then(|| turn_id.to_string())
        });
        let matched = typed_match.or_else(|| {
            let transient_user_text = transient.messages.iter().find_map(|message| match message {
                ConversationMessage::User(user) => Some(user.text.as_str()),
                ConversationMessage::Checkpoint(_)
                | ConversationMessage::Assistant(_)
                | ConversationMessage::ToolResult(_) => None,
            });
            view.entries
                .iter()
                .rev()
                .find_map(|entry| match &entry.payload {
                    SessionEntryPayload::UserMessage { text, .. }
                        if transient_user_text == Some(text.as_str())
                            || text == &transient.user_prompt =>
                    {
                        entry.turn_id.as_ref().map(ToString::to_string)
                    }
                    _ => None,
                })
        });
        superseded.extend(matched);
    }
    superseded
}

fn distinct_transient_turns(turns: &[ProviderConversationTurn]) -> Vec<ProviderConversationTurn> {
    let mut seen = std::collections::BTreeSet::new();
    let mut distinct = turns
        .iter()
        .rev()
        .filter(|turn| {
            logical_turn_id(turn)
                .map(|turn_id| seen.insert(turn_id))
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    distinct.reverse();
    distinct
}

fn logical_turn_id(turn: &ProviderConversationTurn) -> Option<String> {
    turn.messages.iter().find_map(|message| match message {
        ConversationMessage::User(user) => Some(user.request_id.to_string()),
        ConversationMessage::Checkpoint(_)
        | ConversationMessage::Assistant(_)
        | ConversationMessage::ToolResult(_) => None,
    })
}
