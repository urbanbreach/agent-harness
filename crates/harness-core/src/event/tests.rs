use super::{
    ActorKind, BranchSummaryEvent, EventActor, EventBuilder, EventContext, EventV1,
    PermissionDecision, PermissionRequestedArgs, SessionCompactionEvent, ToolCallRequestedEvent,
};
use crate::clock::FakeClock;
use crate::redact::DefaultRedactor;
use crate::UnwrapOrAbort;
use serde_json::json;

#[test]
fn run_started_snapshot_is_stable_in_deterministic_mode() {
    // arrange
    // act
    // assert
    let clock = FakeClock::new();
    clock.advance(42);
    let redactor = DefaultRedactor::default();
    let builder = EventBuilder::new(&clock, &redactor, "run_123");

    let mut context = EventContext::new(
        1,
        EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
    );
    context.stream_key = Some("run:run_123".to_string());

    let envelope = builder
        .run_started(context, "golden_path", "/workspace/project")
        .unwrap_or_abort();

    insta::with_settings!({ snapshot_path => "../snapshots" }, {
        insta::assert_json_snapshot!("run_started_envelope_v1", envelope);
    });
}

#[test]
fn permission_requested_snapshot_is_stable_in_deterministic_mode() {
    // arrange
    // act
    // assert
    let clock = FakeClock::new();
    clock.advance(128);
    let redactor = DefaultRedactor::default();
    let builder = EventBuilder::new(&clock, &redactor, "run_123");

    let mut context = EventContext::new(
        2,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
    );
    context.correlation_id = Some("toolcall_001".to_string());
    context.stream_key = Some("permission:perm_001".to_string());

    let envelope = builder
        .permission_requested(
            context,
            PermissionRequestedArgs {
                permission_id: "perm_001".to_string(),
                kind: "edit".to_string(),
                tool_call_id: Some("toolcall_001".into()),
                summary: "Apply patch to file with Bearer abc.def".to_string(),
                request_digest: "req_90ac2e1e".to_string(),
                timeout_ms: 30_000,
                default_decision: PermissionDecision::Deny,
            },
        )
        .unwrap_or_abort();

    insta::with_settings!({ snapshot_path => "../snapshots" }, {
        insta::assert_json_snapshot!("permission_requested_envelope_v1", envelope);
    });
}

#[test]
fn tool_call_requested_uses_redacted_summary_and_digest() {
    // arrange
    // act
    // assert
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let builder = EventBuilder::new(&clock, &redactor, "run_123");

    let args = json!({
        "cmd": "curl https://example.invalid",
        "auth": "Bearer secret.value",
        "api_key": "sk-ABCDE12345ABCDE",
    });

    let envelope = builder
        .tool_call_requested(
            EventContext::new(
                3,
                EventActor::new(ActorKind::Worker, Some("agent-worker".to_string())),
            ),
            "toolcall_002",
            "shell.run",
            &args,
            None,
        )
        .unwrap_or_abort();

    let EventV1::ToolCallRequested(ToolCallRequestedEvent {
        args_summary,
        args_digest,
        ..
    }) = envelope.payload
    else {
        panic!("expected tool call requested payload")
    };

    assert!(!args_summary.contains("Bearer secret.value"));
    assert!(!args_summary.contains("sk-ABCDE12345ABCDE"));
    assert!(args_summary.contains("Bearer [REDACTED]"));
    assert!(args_summary.contains("[REDACTED_API_KEY]"));
    assert_eq!(args_digest.len(), 12);
}

#[test]
fn session_compaction_event_round_trips_through_serde_json() {
    // arrange
    // act
    // assert
    let event = SessionCompactionEvent {
        agent_id: "agent-build".to_string(),
        summary: "Compacted 12 turns of work on the auth module.".to_string(),
        first_kept_event_seq: 42,
        first_kept_request_id: Some("req_abc123".to_string()),
        tokens_before: 8_192,
        read_files: vec!["src/auth.rs".to_string(), "src/auth/session.rs".to_string()],
        modified_files: vec!["src/auth/session.rs".to_string()],
        trigger_reason: "token_budget_exceeded".to_string(),
        from_hook: false,
    };

    let payload = EventV1::SessionCompaction(event.clone());
    let json = serde_json::to_value(&payload).expect("serialize SessionCompaction");
    let round_trip: EventV1 = serde_json::from_value(json).expect("deserialize SessionCompaction");

    assert_eq!(payload, round_trip);
}

#[test]
fn session_compaction_event_serializes_with_snake_case_tag_and_skips_empty_fields() {
    // arrange
    // act
    // assert
    let event = SessionCompactionEvent {
        agent_id: "agent-build".to_string(),
        summary: "Empty files and no request id.".to_string(),
        first_kept_event_seq: 7,
        first_kept_request_id: None,
        tokens_before: 1_024,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        trigger_reason: "manual".to_string(),
        from_hook: true,
    };

    let payload = EventV1::SessionCompaction(event);
    let json = serde_json::to_value(&payload).expect("serialize");

    assert_eq!(json["event_type"], "session_compaction");
    assert!(json["data"].get("first_kept_request_id").is_none());
    assert!(json["data"].get("read_files").is_none());
    assert!(json["data"].get("modified_files").is_none());
}

#[test]
fn branch_summary_event_round_trips_through_serde_json() {
    // arrange
    // act
    // assert
    let event = BranchSummaryEvent {
        agent_id: "agent-explore".to_string(),
        summary: "Branch explored the crate structure and found 3 entry points.".to_string(),
        from_event_seq: 99,
        read_files: vec!["src/main.rs".to_string()],
        modified_files: vec!["src/config.rs".to_string(), "src/lib.rs".to_string()],
        from_hook: false,
    };

    let payload = EventV1::BranchSummary(event.clone());
    let json = serde_json::to_value(&payload).expect("serialize BranchSummary");
    let round_trip: EventV1 = serde_json::from_value(json).expect("deserialize BranchSummary");

    assert_eq!(payload, round_trip);
}

#[test]
fn branch_summary_event_serializes_with_snake_case_tag_and_skips_empty_fields() {
    // arrange
    // act
    // assert
    let event = BranchSummaryEvent {
        agent_id: "agent-explore".to_string(),
        summary: "No files touched.".to_string(),
        from_event_seq: 5,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        from_hook: true,
    };

    let payload = EventV1::BranchSummary(event);
    let json = serde_json::to_value(&payload).expect("serialize");

    assert_eq!(json["event_type"], "branch_summary");
    assert!(json["data"].get("read_files").is_none());
    assert!(json["data"].get("modified_files").is_none());
}
