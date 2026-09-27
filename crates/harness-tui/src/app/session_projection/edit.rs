//! Edit presentation has one reducer for live input and retained history.
use super::{ActivityEntry, EditDisplayStatus, EditEntry, EventEnvelopeV1, EventV1, ToolCallEntry};
use std::collections::VecDeque;

pub(super) fn apply_edit(tool: &mut ToolCallEntry, event: &EventV1) {
    let (edit_id, path, status) = match event {
        EventV1::EditProposed(edit) => (&edit.edit_id, &edit.path, EditDisplayStatus::Proposed),
        EventV1::EditApplied(edit) => (&edit.edit_id, &edit.path, EditDisplayStatus::Applied),
        EventV1::EditRejected(edit) => (&edit.edit_id, &edit.path, EditDisplayStatus::Rejected),
        _ => return,
    };
    let (summary, patch_digest) = tool
        .edit
        .take()
        .map(|edit| (edit.summary, edit.patch_digest))
        .unwrap_or_default();
    let mut next = EditEntry {
        edit_id: edit_id.clone(),
        path: path.clone(),
        status,
        summary,
        patch_digest,
        new_file_digest: None,
        diff_rel_path: None,
        diff_digest: None,
        rejection_reason: None,
    };
    match event {
        EventV1::EditProposed(edit) => {
            next.summary = Some(edit.summary.clone());
            next.patch_digest = Some(edit.patch_digest.clone());
        }
        EventV1::EditApplied(edit) => {
            next.new_file_digest = Some(edit.new_file_digest.clone());
            next.diff_rel_path.clone_from(&edit.diff_rel_path);
            next.diff_digest.clone_from(&edit.diff_digest);
        }
        EventV1::EditRejected(edit) => next.rejection_reason = Some(edit.reason.clone()),
        _ => {}
    }
    tool.edit = Some(next);
}

// Complete histories own their edits. Capped inline slices keep their prior
// presentation because the missing proposal cannot be reconstructed.
pub(super) fn hydrate_edits(
    events: &[EventEnvelopeV1],
    activities: &mut VecDeque<ActivityEntry>,
    trimmed_prior: Option<&VecDeque<ActivityEntry>>,
) {
    for event in events.iter().filter(|event| {
        matches!(
            event.payload,
            EventV1::EditProposed(_) | EventV1::EditApplied(_) | EventV1::EditRejected(_)
        )
    }) {
        let Some(tool_call_id) = event.correlation_id.as_deref() else {
            continue;
        };
        let Some(tool) = activities
            .iter_mut()
            .flat_map(|activity| activity.tool_calls.iter_mut())
            .find(|tool| tool.tool_call_id == tool_call_id)
        else {
            continue;
        };
        apply_edit(tool, &event.payload);
        tool.last_seq = tool.last_seq.max(event.seq);
    }
    if let Some(prior) = trimmed_prior {
        for tool in activities
            .iter_mut()
            .flat_map(|activity| &mut activity.tool_calls)
        {
            if let Some(previous) = prior
                .iter()
                .flat_map(|activity| &activity.tool_calls)
                .find(|previous| previous.tool_call_id == tool.tool_call_id)
            {
                tool.edit.clone_from(&previous.edit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session_projection::SessionProjection;
    use harness_core::event::EventEnvelopeV1;
    use serde_json::json;

    #[test]
    fn trimmed_child_slice_keeps_proposal_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let mut projection = SessionProjection::default();
        let mut seq = 0;
        let event = |seq, kind, data, correlation| -> Result<EventEnvelopeV1, serde_json::Error> {
            serde_json::from_value(
                json!({"schema_version": 1, "event_id": format!("edit-{seq}"),
                "seq": seq, "run_id": "run", "mono_ms": seq,
                "actor": {"kind": "worker", "agent_id": "child"},
                "correlation_id": correlation, "payload": {"event_type": kind, "data": data}}),
            )
        };
        for (kind, data, correlation) in [
            (
                "provider_request_started",
                json!({"request_id":"child-turn", "provider_id":"mock", "model_id":"model", "prompt_summary":"edit", "request_digest":"request"}),
                "child-turn",
            ),
            (
                "tool_call_requested",
                json!({"tool_call_id":"tool", "tool_id":"edit.hashline_apply", "args_summary":"{}", "args_digest":"args"}),
                "child-turn",
            ),
            (
                "edit_proposed",
                json!({"edit_id":"edit", "path":"demo.txt", "summary":"retained proposal", "patch_digest":"patch"}),
                "tool",
            ),
        ] {
            seq += 1;
            projection.ingest_inline_event(event(seq, kind, data, correlation)?, false);
        }
        projection.memory_caps.max_events = 3;
        seq += 1;
        projection.ingest_inline_event(
            event(
                seq,
                "user_message_submitted",
                json!({"request_id":"next-turn", "text":"next prompt"}),
                "next-turn",
            )?,
            false,
        );
        seq += 1;
        projection.ingest_inline_event(
            event(
                seq,
                "tool_call_started",
                json!({"tool_call_id":"tool"}),
                "child-turn",
            )?,
            false,
        );
        seq += 1;
        projection.ingest_inline_event(
            event(
                seq,
                "edit_applied",
                json!({"edit_id":"edit", "path":"demo.txt", "new_file_digest":"new-file"}),
                "tool",
            )?,
            false,
        );
        assert_eq!(projection.canonical_projection_error(), None);
        assert!(projection
            .events()
            .all(|event| !matches!(event.payload, EventV1::EditProposed(_))));
        let edit = projection
            .activities
            .iter()
            .flat_map(|activity| &activity.tool_calls)
            .find_map(|tool| tool.edit.as_ref())
            .ok_or("missing retained edit")?;
        assert_eq!(edit.status, EditDisplayStatus::Applied);
        assert_eq!(edit.summary.as_deref(), Some("retained proposal"));
        assert_eq!(edit.patch_digest.as_deref(), Some("patch"));
        assert_eq!(edit.new_file_digest.as_deref(), Some("new-file"));
        let expected = edit.clone();

        // Rewinding a later prompt must not erase this surviving edit's
        // metadata merely because its proposal is outside the capped slice.
        projection.memory_caps.max_events = 5;
        projection.ingest_inline_event(
            event(
                7,
                "user_message_submitted",
                json!({"request_id":"discarded", "text":"discarded prompt"}),
                "discarded",
            )?,
            false,
        );
        projection.ingest_inline_event(
            event(
                8,
                "conversation_rewound",
                json!({"target_seq":7, "request_id":"discarded"}),
                "discarded",
            )?,
            false,
        );
        let edit = projection
            .activities
            .iter()
            .flat_map(|activity| &activity.tool_calls)
            .find_map(|tool| tool.edit.as_ref())
            .ok_or("missing edit after rewind")?;
        assert_eq!(edit, &expected);
        Ok(())
    }
}
