use std::fs;

use harness_core::event::EventEnvelopeV1;
use harness_core::proj::{project_run_summary, RunStatus};
use harness_core::store::{EventStore, JsonlFileEventStore};
use harness_core::transcript_projection::{project_transcript, ProjectedMessageRole};
use harness_core::workflow::project_workflows;
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::test]
async fn checked_in_golden_event_log_replays_to_expected_projection_state() {
    let events = replay_golden_events_from_jsonl_store("replay_golden_session.jsonl").await;

    let run_summary = project_run_summary(events.iter()).expect("run summary projection");
    assert_eq!(run_summary.status, RunStatus::Finished);
    assert_eq!(run_summary.counts.total_events, 16);
    assert!(run_summary.pending_permissions.is_empty());
    assert!(run_summary.tasks_in_flight.is_empty());
    assert_eq!(
        run_summary.counts.by_type.get("permission_requested"),
        Some(&1)
    );
    assert_eq!(
        run_summary.counts.by_type.get("tool_call_finished"),
        Some(&1)
    );

    let transcript = project_transcript(&events).expect("transcript projection");
    assert_eq!(
        transcript.session.run_id.as_deref(),
        Some("run_golden_replay")
    );
    assert_eq!(
        transcript.session.run_name.as_deref(),
        Some("golden replay")
    );
    assert_eq!(transcript.session.max_seq, Some(16));

    let user_message = transcript
        .messages
        .iter()
        .find(|message| message.role == ProjectedMessageRole::User)
        .expect("projected user message");
    assert_eq!(user_message.request_id.as_deref(), Some("req_000001"));

    let transcript_json = serde_json::to_value(&transcript).expect("serialize transcript");
    assert_eq!(
        transcript_json.pointer("/session/status"),
        Some(&json!("finished"))
    );
    assert_eq!(
        transcript_json.pointer("/session/status_reason"),
        Some(&json!("golden complete"))
    );
    assert!(
        transcript_json
            .pointer("/messages")
            .and_then(serde_json::Value::as_array)
            .expect("messages array")
            .iter()
            .any(|message| message.to_string().contains("toolcall_000001")
                && message
                    .to_string()
                    .contains("Updated README deterministically")),
        "tool state should replay into transcript projection: {transcript_json}"
    );
    assert!(
        transcript_json
            .pointer("/messages")
            .and_then(serde_json::Value::as_array)
            .expect("messages array")
            .iter()
            .any(|message| message.to_string().contains("perm_000001")
                && message.to_string().contains("allow")),
        "permission state should replay into transcript projection: {transcript_json}"
    );

    let workflow_projection = project_workflows(events.iter().map(|event| &event.payload));
    let workflow = workflow_projection
        .workflows
        .get("wf_golden")
        .expect("workflow projection");
    assert_eq!(workflow.mode, "ralph");
    assert_eq!(workflow.owner, "operator");
    assert_eq!(workflow.status, "outcome.succeeded");
    assert!(workflow.terminal);
    assert_eq!(workflow.lane.as_deref(), Some("deterministic"));
}

async fn replay_golden_events_from_jsonl_store(name: &str) -> Vec<EventEnvelopeV1> {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let temp_dir = tempfile::tempdir().expect("golden replay tempdir");
    let run_dir = temp_dir.path().join("run_golden_replay");
    fs::create_dir_all(&run_dir).expect("golden replay run dir");
    fs::copy(&fixture_path, run_dir.join("events.jsonl")).unwrap_or_else(|error| {
        panic!(
            "failed to copy {} into replay store: {error}",
            fixture_path.display()
        )
    });

    let store = JsonlFileEventStore::open_existing(temp_dir.path(), "run_golden_replay", true)
        .expect("open golden JSONL event store");
    store
        .replay(1)
        .expect("build golden replay stream")
        .map(|event| event.expect("replay event"))
        .collect()
        .await
}
