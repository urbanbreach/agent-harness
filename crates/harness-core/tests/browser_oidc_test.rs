//! Owner regression tests for browser OIDC real implementation wiring.
//!
//! Tests the real `launch_browser`, `listen_for_callback`, and
//! `exchange_code_for_token` implementations through the product probe
//! path and the `BrowserOidcFlow` state machine, using a mock HTTP
//! server for token exchange.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use harness_core::browser_oidc::*;
use harness_providers::UnwrapOrAbort;

fn find_free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_abort();
    let port = listener.local_addr().unwrap_or_abort().port();
    drop(listener);
    port
}

fn start_mock_token_server(status_line: String, body: String) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap_or_abort();
    let port = listener.local_addr().unwrap_or_abort().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap_or_abort();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    (port, handle)
}

fn send_fake_callback(port: u16, code: &str, state: &str) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap_or_abort();
    let request =
        format!("GET /callback?code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.flush();
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);
}

#[test]
fn exchange_code_for_token_succeeds_with_mock_server() {
    // arrange: a mock token endpoint returning a valid token response
    let token_json = r#"{"access_token":"test-access-token","token_type":"Bearer","id_token":"test-id-token","refresh_token":"test-refresh-token","expires_in":3600}"#;
    let (port, handle) = start_mock_token_server("200 OK".to_string(), token_json.to_string());

    // act: exchanging an authorization code for tokens
    let token_endpoint = format!("http://127.0.0.1:{port}/token");
    let result = exchange_code_for_token(
        &token_endpoint,
        "test-client",
        "http://127.0.0.1:8765/callback",
        "test-auth-code",
        "test-code-verifier",
    );

    // assert: the token response is parsed correctly
    let token = match result {
        Ok(t) => t,
        Err(e) => panic!("token exchange should succeed: {e}"),
    };
    assert_eq!(token.access_token, "test-access-token");
    assert_eq!(token.token_type, "Bearer");
    assert_eq!(token.id_token.as_deref(), Some("test-id-token"));
    assert_eq!(token.refresh_token.as_deref(), Some("test-refresh-token"));
    assert_eq!(token.expires_in, Some(3600));
    handle.join().unwrap_or_abort();
}

#[test]
fn exchange_code_for_token_fails_on_error_response() {
    // arrange: a mock token endpoint returning an error
    let error_json = r#"{"error":"invalid_grant"}"#;
    let (port, handle) =
        start_mock_token_server("400 Bad Request".to_string(), error_json.to_string());

    // act: exchanging an authorization code for tokens
    let token_endpoint = format!("http://127.0.0.1:{port}/token");
    let result = exchange_code_for_token(
        &token_endpoint,
        "test-client",
        "http://127.0.0.1:8765/callback",
        "bad-code",
        "test-code-verifier",
    );

    // assert: the exchange fails with an error
    let err = match result {
        Err(e) => e,
        Ok(v) => panic!("expected error, got token: {v:?}"),
    };
    assert!(!err.is_empty());
    handle.join().unwrap_or_abort();
}

#[test]
fn listen_for_callback_receives_code_and_state() {
    // arrange: a free port for the callback listener
    let port = find_free_port();

    // act: listening for a callback while a fake callback is sent
    let callback_port = port;
    let callback_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        send_fake_callback(callback_port, "test-code-123", "test-state-456");
    });
    let result = listen_for_callback(port, 5);

    // assert: the code and state are extracted from the callback
    let (code, state) = match result {
        Ok(v) => v,
        Err(e) => panic!("callback should be received: {e}"),
    };
    assert_eq!(code, "test-code-123");
    assert_eq!(state, "test-state-456");
    callback_handle.join().unwrap_or_abort();
}

#[test]
fn listen_for_callback_times_out_when_no_callback() {
    // arrange: a free port with no callback incoming
    let port = find_free_port();

    // act: listening with a 0-second timeout
    let result = listen_for_callback(port, 0);

    // assert: the listener times out
    let err = match result {
        Err(e) => e,
        Ok(v) => panic!("expected timeout, got: {v:?}"),
    };
    assert!(err.contains("timed out"));
}

#[test]
fn full_flow_completes_with_mock_token_endpoint() {
    // arrange: a mock token endpoint and a free callback port
    let token_json = r#"{"access_token":"flow-access-token","token_type":"Bearer","id_token":"flow-id-token","expires_in":7200}"#;
    let (token_port, token_handle) =
        start_mock_token_server("200 OK".to_string(), token_json.to_string());
    let callback_port = find_free_port();
    let issuer = format!("http://127.0.0.1:{token_port}");

    // act: driving a BrowserOidcFlow through start -> complete
    let mut flow = BrowserOidcFlow::new();
    assert_eq!(flow.phase(), BrowserOidcFlowPhase::Idle);
    flow.start(&issuer, "flow-client", callback_port);
    assert_eq!(flow.phase(), BrowserOidcFlowPhase::WaitingForCallback);

    let cb_port = callback_port;
    let callback_handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        send_fake_callback(cb_port, "flow-auth-code", "flow-state");
    });

    flow.complete(5);

    // assert: the flow reaches Completed with the correct token
    assert_eq!(flow.phase(), BrowserOidcFlowPhase::Completed);
    let token = match &flow {
        BrowserOidcFlow::Completed { token } => token,
        other => panic!("expected Completed, got {other:?}"),
    };
    assert_eq!(token.access_token, "flow-access-token");
    assert_eq!(token.token_type, "Bearer");
    assert_eq!(token.id_token.as_deref(), Some("flow-id-token"));
    assert_eq!(token.expires_in, Some(7200));

    callback_handle.join().unwrap_or_abort();
    token_handle.join().unwrap_or_abort();
}

#[test]
fn full_flow_fails_on_callback_timeout() {
    // arrange: a BrowserOidcFlow started with a free callback port
    let callback_port = find_free_port();
    let mut flow = BrowserOidcFlow::new();
    flow.start("https://issuer.example", "timeout-client", callback_port);
    assert_eq!(flow.phase(), BrowserOidcFlowPhase::WaitingForCallback);

    // act: completing with a 0-second timeout (no callback sent)
    flow.complete(0);

    // assert: the flow transitions to Failed
    assert_eq!(flow.phase(), BrowserOidcFlowPhase::Failed);
    let reason = match &flow {
        BrowserOidcFlow::Failed { reason } => reason,
        other => panic!("expected Failed, got {other:?}"),
    };
    assert!(reason.contains("timed out"));
}

#[test]
fn complete_browser_oidc_flow_returns_unavailable_for_probe_start() {
    // arrange: a probe start result (Unavailable)
    let start = start_browser_oidc_flow("(probe)", "(client)");

    // act: completing the probe start
    let complete = complete_browser_oidc_flow(&start, 0);

    // assert: the result is Unavailable without blocking
    match &complete {
        BrowserOidcCompleteResult::Unavailable { reason, .. } => {
            assert!(reason.contains("Unavailable"));
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn start_browser_oidc_flow_wires_launch_browser_for_real_values() {
    // arrange: a real issuer and client_id
    // act: starting the flow
    let start = start_browser_oidc_flow("https://issuer.example", "real-client");

    // assert: the result is Started with launch_browser having been called
    // (manual_url_fallback is Some if browser launch failed, None if it succeeded)
    match &start {
        BrowserOidcStartResult::Started {
            authorization_url,
            token_endpoint,
            client_id,
            port,
            ..
        } => {
            assert!(authorization_url.contains("https://issuer.example/authorize"));
            assert_eq!(token_endpoint, "https://issuer.example/token");
            assert_eq!(client_id, "real-client");
            assert_eq!(*port, DEFAULT_OIDC_CALLBACK_PORT);
        }
        other => panic!("expected Started, got {other:?}"),
    }
}
