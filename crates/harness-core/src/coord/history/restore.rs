//! Pure helpers for reconstructing settled provider history.
use super::*;
use harness_compaction::{ImageDetail, ImageMeta, ImageOrigin, NoteKind, Origin, Stop};

/// What the journal recorded about how one provider request ended.
pub(super) struct Finish<'a> {
    pub usage: Option<&'a harness_providers::CompletionUsage>,
    pub stop: Stop,
}

pub(super) fn finishes(events: &[EventEnvelopeV1]) -> HashMap<&str, Finish<'_>> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(finished) => {
                let limited = finished.metadata.as_ref().is_some_and(|m| {
                    m.provider_error_category
                        == Some(harness_providers::ProviderErrorCategory::OutputLimitExceeded)
                });
                let stop = if limited {
                    Stop::Truncated
                } else {
                    match finished.finish_reason.as_str() {
                        "error" | "stream_guard" => Stop::Error,
                        "cancelled" => Stop::Aborted,
                        _ => Stop::Complete,
                    }
                };
                // Usage of a failed or aborted response never anchors an estimate.
                let usage = finished
                    .usage
                    .as_ref()
                    .filter(|_| matches!(stop, Stop::Complete | Stop::Truncated));
                Some((finished.request_id.as_str(), Finish { usage, stop }))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn apply_pruned_events(
    entries: &mut [Entry],
    events: &[EventEnvelopeV1],
    agent: &str,
    tool_ids: &HashMap<String, (u64, String)>,
) {
    for pruned in events.iter().filter_map(|event| match &event.payload {
        EventV1::ContextPruned(pruned) if pruned.agent_id == agent => Some(pruned),
        _ => None,
    }) {
        apply_edits(entries, &pruned.edits, tool_ids);
    }
}

pub(super) fn assistant_reasoning(parts: &[crate::session::AssistantPart]) -> Vec<String> {
    parts
        .iter()
        .filter_map(|part| match part {
            crate::session::AssistantPart::Reasoning { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Incomplete-response recovery replaced a truncated answer before compacting; the retry
/// regenerates it, so the restored turn drops it.
pub(super) fn drop_recovered_truncation(
    turns: &mut [Vec<Entry>],
    turn: Option<usize>,
    limited: &HashSet<u64>,
) {
    if let Some(turn) = turn.and_then(|index| turns.get_mut(index))
        && turn
            .last()
            .is_some_and(|entry| limited.contains(&entry.seq))
    {
        turn.pop();
    }
}

pub(super) fn user_entry(
    text: &str,
    event: &EventEnvelopeV1,
    turn: String,
    origin: Origin,
    note_kind: Option<NoteKind>,
) -> Entry {
    let mut entry = Entry::new(
        CompletionMessage::text(MessageRole::User, text),
        event.seq,
        Some(turn),
    );
    entry.at_ms = event_time(event);
    entry.origin = origin;
    entry.note_kind = note_kind;
    entry
}

pub(super) fn event_time(event: &EventEnvelopeV1) -> Option<u64> {
    crate::transcript_projection::event_time_ms(event)
}

/// The stored argument text is returned as recorded when it is a JSON object; anything else
/// replays as an empty object.
pub(super) fn replayed_arguments(summary: &str) -> String {
    serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .filter(serde_json::Value::is_object)
        .map_or_else(|| "{}".into(), |_| summary.to_owned())
}

/// Image facts of every entry, derived from its attachments after edits settled.
pub(in crate::coord) fn derive_images(entries: &mut [Entry]) {
    for entry in entries {
        let origin = if entry.compaction.is_some() {
            ImageOrigin::ArchiveFrame
        } else if entry.message.role == MessageRole::Tool {
            ImageOrigin::Tool
        } else {
            ImageOrigin::User
        };
        entry.images = entry
            .attachments
            .iter()
            .filter(|a| a.mime.starts_with("image/"))
            .map(|a| ImageMeta {
                mime: a.mime.clone(),
                width: a.dimensions.map(|d| d.width),
                height: a.dimensions.map(|d| d.height),
                detail: a.detail.as_deref().and_then(image_detail),
                origin,
            })
            .collect();
    }
}

fn image_detail(name: &str) -> Option<ImageDetail> {
    match name {
        "auto" => Some(ImageDetail::Auto),
        "low" => Some(ImageDetail::Low),
        "high" => Some(ImageDetail::High),
        "original" => Some(ImageDetail::Original),
        _ => None,
    }
}
