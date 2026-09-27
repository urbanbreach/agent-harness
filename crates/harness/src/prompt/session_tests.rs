use harness_core::{
    event::EventEnvelopeV1,
    redact::DefaultRedactor,
    store::read_events,
    transcript_projection::{project_transcript, ProjectedPart},
};
use serde_json::json;

#[test]
fn legacy_export_keeps_settled_text_and_sequence_boundaries_without_private_payloads(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let session = root.path().join("sessions/legacy");
    std::fs::create_dir_all(&session)?;
    let payloads = [
        (
            "run_started",
            json!({"run_name":"legacy","workspace_root":root.path()}),
        ),
        (
            "agent_spawned",
            json!({"agent_id":"agent","profile":"default"}),
        ),
        (
            "user_message_submitted",
            json!({"request_id":"first","text":"Keep this turn"}),
        ),
        (
            "provider_request_started",
            json!({"request_id":"p1","provider_id":"mock","model_id":"mock","prompt_summary":"first","request_digest":"d"}),
        ),
        (
            "provider_stream_delta",
            json!({"request_id":"p1","delta":"Legacy settled answer"}),
        ),
        (
            "provider_reasoning_delta",
            json!({"request_id":"p1","delta":"hidden reasoning"}),
        ),
        (
            "assistant_message_finished",
            json!({"request_id":"p1","tool_call_count":0}),
        ),
        (
            "user_message_submitted",
            json!({"request_id":"second","text":"Discard this turn"}),
        ),
        (
            "provider_request_started",
            json!({"request_id":"p2","provider_id":"mock","model_id":"mock","prompt_summary":"second","request_digest":"d"}),
        ),
        (
            "provider_stream_delta",
            json!({"request_id":"p2","delta":"draft"}),
        ),
        (
            "assistant_message_finished",
            json!({"request_id":"p2","tool_call_count":1,"parts":[{"kind":"text","text":"Final answer"},{"kind":"reasoning","text":"hidden reasoning"}]}),
        ),
        (
            "tool_call_finished",
            json!({"tool_call_id":"tool","status":"succeeded","output_summary":"safe result","output_json":{"raw":"private tool payload"}}),
        ),
        (
            "conversation_rewound",
            json!({"target_seq":8,"request_id":"second"}),
        ),
        (
            "session_compaction",
            json!({"agent_id":"agent","summary":"summary","first_kept_event_seq":8,"tokens_before":100,"trigger_reason":"manual"}),
        ),
        (
            "branch_summary",
            json!({"agent_id":"agent","summary":"branch","from_event_seq":11}),
        ),
        (
            "compaction_requested",
            json!({"checkpoint_id":"c","agent_id":"agent","trigger_reason":"manual","through_seq":11}),
        ),
        (
            "compaction_written",
            json!({"checkpoint_id":"c","agent_id":"agent","artifact_path":"summary.txt","artifact_bytes":7,"trigger_reason":"manual","through_seq":11,"preserved_turns":1}),
        ),
        (
            "compaction_applied",
            json!({"checkpoint_id":"c","agent_id":"agent","through_seq":11}),
        ),
        (
            "compaction_failed",
            json!({"agent_id":"agent","trigger_reason":"manual","reason":"unavailable","through_seq":11}),
        ),
        ("run_finished", json!({"summary":"finished"})),
    ];
    let mut journal = String::new();
    for (index, (kind, data)) in payloads.into_iter().enumerate() {
        let event: EventEnvelopeV1 = serde_json::from_value(json!({
            "schema_version":1,"seq":index+1,"event_id":format!("e-{}",index+1),"run_id":"legacy","mono_ms":0,
            "actor":{"kind":"worker","agent_id":"agent"},"payload":{"event_type":kind,"data":data}
        }))?;
        journal.push_str(&serde_json::to_string(&event)?);
        journal.push('\n');
    }
    let source = session.join("events.jsonl");
    let output = root.path().join("export.jsonl");
    std::fs::write(&source, &journal)?;
    super::export(&source, &output, &DefaultRedactor::default())?;
    let exported = std::fs::read_to_string(&output)?;
    assert!(!exported.contains("hidden reasoning"));
    assert!(!exported.contains("private tool payload"));
    assert!(!exported.contains("provider_stream_delta"));
    let events = read_events(&output)?;
    assert_eq!(events.len(), 17);
    for event in &events {
        let payload = serde_json::to_value(&event.payload)?;
        for (field, expected) in [
            ("target_seq", 6),
            ("first_kept_event_seq", 6),
            ("from_event_seq", 8),
            ("through_seq", 8),
        ] {
            if let Some(value) = payload["data"].get(field) {
                assert_eq!(value, expected, "{field}");
            }
        }
    }
    let visible: Vec<_> = project_transcript(&events)?
        .messages
        .into_iter()
        .flat_map(|m| m.parts)
        .filter_map(|p| match p {
            ProjectedPart::Text(p) => Some(p.text),
            _ => None,
        })
        .collect();
    assert!(visible.iter().any(|text| text == "Legacy settled answer"));
    assert!(!visible.iter().any(|text| text.contains("Discard")));
    assert_eq!(std::fs::read_to_string(source)?, journal);
    Ok(())
}
