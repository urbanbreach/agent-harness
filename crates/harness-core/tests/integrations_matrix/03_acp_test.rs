// ---------------------------------------------------------------------------
// ACP family
// ---------------------------------------------------------------------------

#[test]
fn acp_boundary_e2e_stdio_transport_connects_and_operates_via_subprocess() {
    // arrange — a stdio ACP transport using `cat` as the subprocess
    // act — the agent mode product runs
    let product = run_stdio_acp_agent_mode_product("cat");

    // assert — the product meets the agent mode contract
    assert!(
        product.meets_agent_mode_contract(),
        "stdio ACP product must meet contract: {product:?}"
    );
    assert!(product.operate_ok);
}

#[test]
fn acp_bad_input_invalid_command_fails_connect() {
    // arrange — a stdio ACP transport with an invalid command
    // act — the agent mode product runs
    let product = run_stdio_acp_agent_mode_product("exit 1");

    // assert — the product does not meet the contract
    assert!(!product.meets_agent_mode_contract());
}

#[test]
fn acp_permission_denial_connect_failure_ends_in_failed_state() {
    // arrange — a mock ACP transport configured to fail connect
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    transport.fail_connect_reason = "probe-connect-denied".to_string();
    let mut session = AcpConnection::new(transport);

    // act — connect is attempted
    let err = session.connect().expect_err("connect must fail");

    // assert — the session is in Failed state, not Connected
    assert_eq!(err, AcpError::Transport("probe-connect-denied".to_string()));
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));
    assert!(!session.state().is_connected());
}

#[test]
fn acp_process_failure_transport_error_during_operation_marks_failed() {
    // arrange — a connected session that will fail on the next operate
    let mut transport = MockAcpTransport::new();
    transport.fail_on_next_operate = true;
    transport.fail_operate_reason = "io error".to_string();
    let mut session = AcpConnection::new(transport);
    session.connect().expect("connect");

    // act — operate is called
    let err = session.operate(b"work").expect_err("operate must fail");

    // assert — the session is in Failed state
    assert_eq!(err, AcpError::OperationAborted("io error".to_string()));
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));
}

#[test]
fn acp_cancellation_restart_reconnect_from_failed_recovers_to_connected() {
    // arrange — a session that previously failed to connect
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    let mut session = AcpConnection::new(transport);
    let _ = session.connect();
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));

    // act — the failure is cleared and reconnect is called
    session.transport_mut().fail_connect = false;
    session.reconnect().expect("reconnect");

    // assert — the session is back in Connected state
    assert_eq!(session.state(), &AcpConnectionState::Connected);
}

#[test]
fn acp_redaction_session_summary_does_not_expose_transport_secrets() {
    // arrange — a connected + bound ACP session
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");
    session.bind_session("default").expect("bind");

    // act — the session summary is serialized
    let summary = session.summary();
    let summary_json = serde_json::to_string(&summary).expect("serialize");

    // assert — the summary does not contain secret-like patterns
    assert!(!summary_json.contains("Bearer "));
    assert!(!summary_json.contains("sk-"));
    assert!(!summary_json.contains("password"));
    assert!(summary_json.contains("connected"));
    assert!(summary_json.contains("acp-session-1"));
}

