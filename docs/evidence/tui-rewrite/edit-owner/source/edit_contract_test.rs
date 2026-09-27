use super::*;

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
            .events.iter()
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
