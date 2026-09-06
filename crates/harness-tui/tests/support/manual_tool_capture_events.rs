use harness_core::event::{
    EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus,
};

use crate::capture_events::{envelope, ACTIVE_REQUEST_ID};

/// Synthetic display data only: none of these commands or tools is executed.
pub(crate) fn append_tools(events: &mut Vec<EventEnvelopeV1>, permission: bool) {
    let fixtures = [
        ("read", r#"{"filePath":"src/alpha.rs"}"#, "fn alpha() {}"),
        ("read", r#"{"filePath":"src/beta.rs"}"#, "fn beta() {}"),
        ("list", r#"{"path":"src"}"#, "alpha.rs\nbeta.rs\nmodules/"),
        ("bash", r#"{"command":"cargo test --workspace","description":"Check workspace tests"}"#, "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten"),
        ("edit", r#"{"filePath":"src/alpha.rs","oldString":"fn alpha() {}","newString":"fn alpha() { check(); }"}"#, "--- a/src/alpha.rs\n+++ b/src/alpha.rs\n@@ -1 +1 @@\n-fn alpha() {}\n+fn alpha() { check(); }"),
        ("mcp.docs.lookup", r#"{"query":"terminal cells"}"#, "Wide text: 中文终端, combining: e\u{301}, and ordinary output."),
    ];
    let mut seq = 4;
    for (index, (tool_id, args, output)) in fixtures.into_iter().enumerate() {
        let id = format!("capture-tool-{index}");
        for payload in [
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: id.clone().into(),
                tool_id: tool_id.to_string(),
                args_summary: args.to_string(),
                args_digest: format!("capture-args-{index}"),
                metadata: None,
            }),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: id.clone().into(),
            }),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: id.into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(output.to_string()),
                output_digest: Some(format!("capture-output-{index}")),
                output_json: (tool_id == "read").then(|| {
                    serde_json::json!({
                        "metadata": { "display": { "text": output, "lineStart": 1 } }
                    })
                }),
                metadata: None,
            }),
        ] {
            events.push(envelope(seq, ACTIVE_REQUEST_ID, payload));
            seq += 1;
        }
    }
    events.push(envelope(
        seq,
        ACTIVE_REQUEST_ID,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "capture-active".into(),
            tool_id: "bash".to_string(),
            args_summary: r#"{"command":"cargo check","description":"Check terminal parity"}"#
                .to_string(),
            args_digest: "capture-active-args".to_string(),
            metadata: None,
        }),
    ));
    events.push(envelope(
        seq + 1,
        ACTIVE_REQUEST_ID,
        if permission {
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "capture-permission".to_string(),
                kind: "bash".to_string(),
                tool_call_id: Some("capture-active".into()),
                summary: "Run cargo check".to_string(),
                request_digest: "capture-permission-digest".to_string(),
                timeout_ms: 300_000,
                default_decision: PermissionDecision::Deny,
            })
        } else {
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "capture-active".into(),
            })
        },
    ));
}
