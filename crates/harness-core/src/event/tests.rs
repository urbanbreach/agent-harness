use super::{
    ActorKind, EventActor, EventBuilder, EventContext, EventV1, PermissionDecision,
    PermissionRequestedArgs, ToolCallRequestedEvent,
};
use crate::clock::FakeClock;
use crate::redact::DefaultRedactor;
use crate::UnwrapOrAbort;
use serde_json::json;

#[test]
fn run_started_snapshot_is_stable_in_deterministic_mode() {
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
                tool_call_id: Some("toolcall_001".to_string()),
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
