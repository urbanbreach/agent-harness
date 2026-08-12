#![allow(clippy::expect_used, reason = "protocol fixture tests fail fast")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

use harness_testkit::parity::{CursorState, SemanticFrame};
use harness_testkit::tui_fidelity_fixture::{
    Packet2FixtureServer, DELTA_COUNT, DISCLOSURE_BODY, DISCLOSURE_SENTINEL, STREAM_SENTINEL,
};
use harness_testkit::tui_fidelity_runner::{semantic_click_bytes, RunnerError};

#[test]
fn failed_tool_then_paced_stream_is_protocol_complete() {
    let server = Packet2FixtureServer::start().expect("fixture starts");
    let authority = server
        .base_url()
        .trim_start_matches("http://")
        .trim_end_matches("/v1")
        .to_owned();

    let first = request(
        &authority,
        serde_json::json!({
            "model":"fixture","stream":true,
            "messages":[{"role":"user","content":"start"}],
            "tools":[{"type":"function","function":{"name":"shell_command","parameters":{"type":"object"}}}]
        }),
    );
    assert!(first.contains("packet2-call"));
    assert!(first.contains("shell_command"));

    let second = request(
        &authority,
        serde_json::json!({
            "model":"fixture","stream":true,
            "messages":[
                {"role":"assistant","tool_calls":[{"id":"packet2-call","type":"function","function":{"name":"shell_command","arguments":"{}"}}]},
                {"role":"tool","tool_call_id":"packet2-call","content":format!("{DISCLOSURE_SENTINEL}\n{DISCLOSURE_BODY}\nline-2\nexit 7")}
            ]
        }),
    );
    let trace = server.finish().expect("fixture completes");

    assert!(second.contains(STREAM_SENTINEL));
    assert_eq!(second.matches("chat.completion.chunk").count(), DELTA_COUNT);
    assert!(second.matches('\u{200b}').count() >= DELTA_COUNT.saturating_sub(1));
    assert_eq!(trace.delta_count, DELTA_COUNT);
    assert!(trace.last_delta_micros.expect("last delta") >= 5_900_000);
    write_evidence(
        "trace.json",
        &serde_json::to_vec_pretty(&trace).expect("trace JSON"),
    );
}

#[test]
fn click_text_fails_when_sentinel_is_absent() {
    let frame = SemanticFrame::new(20, 4, CursorState::hidden(0, 0));
    let mut mouse_bytes = Vec::new();

    let result =
        semantic_click_bytes(&frame, DISCLOSURE_SENTINEL, 0).map(|bytes| mouse_bytes.extend(bytes));

    assert!(matches!(
        result,
        Err(RunnerError::SemanticTargetMissing { .. })
    ));
    assert!(mouse_bytes.is_empty());
    write_evidence("zero-mouse-bytes.txt", b"0\n");
}

#[test]
fn click_text_resolves_current_cell_and_emits_sgr_down_up() {
    let mut frame = SemanticFrame::new(30, 4, CursorState::hidden(0, 0));
    for (offset, character) in "TARGET".chars().enumerate() {
        let col = 5 + u16::try_from(offset).expect("short target");
        frame
            .set_cell(
                harness_testkit::parity::SemanticCell::blank(2, col)
                    .with_grapheme(character.to_string(), 1),
            )
            .expect("target cell");
    }

    let bytes = semantic_click_bytes(&frame, "TARGET", 2).expect("target resolves");

    assert_eq!(bytes, b"\x1b[<0;8;3M\x1b[<0;8;3m");
}

fn request(authority: &str, body: serde_json::Value) -> String {
    let body = serde_json::to_vec(&body).expect("request JSON");
    let mut stream = TcpStream::connect(authority).expect("connect fixture");
    write!(stream, "POST /v1/chat/completions HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).expect("headers");
    stream.write_all(&body).expect("body");
    stream.flush().expect("flush");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("response");
    response
}

fn write_evidence(name: &str, bytes: &[u8]) {
    let Some(root) = std::env::var_os("PACKET2_FIXTURE_EVIDENCE").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&root).expect("evidence root");
    std::fs::write(root.join(name), bytes).expect("evidence file");
}
