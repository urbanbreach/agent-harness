//! ACP lifecycle foundation tests (T10).
//!
//! Happy / edge / adjacent coverage for the offline ACP connection state machine.

use harness_core::integrations::{
    AcpConnection, AcpConnectionState, AcpConnectionSummary, AcpError, MockAcpTransport,
};
use harness_core::UnwrapOrAbort;

#[test]
fn acp_connect_happy_path_reaches_connected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);

    // When
    session.connect().expect("connect should succeed");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Connected);
    assert!(session.transport().connected);
}

#[test]
fn acp_connect_failure_ends_in_failed_not_connected() {
    // arrange
    // act
    // assert
    // Given
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    transport.fail_connect_reason = "refused".to_string();
    let mut session = AcpConnection::new(transport);

    // When
    let err = session.connect().expect_err("connect must fail");

    // Then
    assert_eq!(err, AcpError::Transport("refused".to_string()));
    assert_eq!(
        session.state(),
        &AcpConnectionState::Failed {
            reason: "refused".to_string()
        }
    );
    assert!(!session.state().is_connected());
}

#[test]
fn acp_disconnect_from_connected_returns_to_disconnected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");

    // When
    session.disconnect().expect("disconnect");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);
    assert!(!session.transport().connected);
}

#[test]
fn acp_reconnect_from_failed_can_recover() {
    // arrange
    // act
    // assert
    // Given: prior failed connect
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    let mut session = AcpConnection::new(transport);
    let _ = session.connect();
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));

    // When: clear failure and reconnect
    session.transport_mut().fail_connect = false;
    session.reconnect().expect("reconnect");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Connected);
}

#[test]
fn acp_disconnect_during_operation_is_not_success() {
    // arrange
    // act
    // assert
    // Given: connected session that will drop mid-operate
    let mut transport = MockAcpTransport::new();
    transport.disconnect_on_next_operate = true;
    let mut session = AcpConnection::new(transport);
    session.connect().expect("connect");

    // When
    let err = session
        .operate(b"ping")
        .expect_err("mid-op disconnect must not succeed");

    // Then: Failed/Disconnected, never Connected success
    assert!(matches!(err, AcpError::OperationAborted(_)));
    assert!(
        matches!(
            session.state(),
            AcpConnectionState::Disconnected | AcpConnectionState::Failed { .. }
        ),
        "expected Disconnected or Failed, got {}",
        session.state()
    );
    assert!(!session.state().is_connected());
}

#[test]
fn acp_transport_error_during_operation_marks_failed() {
    // arrange
    // act
    // assert
    // Given
    let mut transport = MockAcpTransport::new();
    transport.fail_on_next_operate = true;
    transport.fail_operate_reason = "io error".to_string();
    let mut session = AcpConnection::new(transport);
    session.connect().expect("connect");

    // When
    let err = session.operate(b"work").expect_err("operate must fail");

    // Then
    assert_eq!(err, AcpError::OperationAborted("io error".to_string()));
    assert_eq!(
        session.state(),
        &AcpConnectionState::Failed {
            reason: "io error".to_string()
        }
    );
}

#[test]
fn acp_operate_while_disconnected_is_rejected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());

    // When
    let err = session.operate(b"x").expect_err("must reject");

    // Then
    assert!(matches!(err, AcpError::NotConnected { .. }));
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);
}

#[test]
fn acp_connect_while_connected_is_rejected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");

    // When
    let err = session.connect().expect_err("double connect");

    // Then
    assert!(matches!(err, AcpError::InvalidConnectState { .. }));
    assert_eq!(session.state(), &AcpConnectionState::Connected);
}

#[test]
fn acp_bind_session_while_connected_assigns_session_id() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");
    assert!(session.session().is_none());

    // When
    let bound = session.bind_session("default").expect("bind");

    // Then
    assert_eq!(bound.agent_name, "default");
    assert_eq!(bound.session_id, "acp-session-1");
    assert_eq!(
        session.session().map(|s| s.session_id.as_str()),
        Some("acp-session-1")
    );
}

#[test]
fn acp_bind_session_while_disconnected_is_rejected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());

    // When
    let err = session.bind_session("default").expect_err("must reject");

    // Then
    assert!(matches!(err, AcpError::SessionBindNotConnected { .. }));
    assert!(session.session().is_none());
}

#[test]
fn acp_bind_session_rejects_empty_agent_name_and_double_bind() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");

    // When / Then empty name
    let empty_err = session.bind_session("   ").expect_err("empty");
    assert!(matches!(empty_err, AcpError::EmptyAgentName));

    // When / Then double bind
    session.bind_session("default").expect("first bind");
    let second = session.bind_session("explore").expect_err("double bind");
    assert!(matches!(
        second,
        AcpError::SessionAlreadyBound {
            session_id
        } if session_id == "acp-session-1"
    ));
}

#[test]
fn acp_disconnect_clears_bound_session() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");
    session.bind_session("default").expect("bind");
    assert!(session.session().is_some());

    // When
    session.disconnect().expect("disconnect");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);
    assert!(session.session().is_none());
}

#[test]
fn acp_operator_diagnostics_cover_state_session_and_summary() {
    // arrange
    // act
    // assert
    // Given: disconnected → connected+bound → failed with session retained
    let mut session = AcpConnection::new(MockAcpTransport::new());
    assert_eq!(
        session.summary(),
        AcpConnectionSummary {
            state: "disconnected".to_string(),
            session_id: None,
            agent_name: None,
            bound: false,
        }
    );
    assert!(session.state().one_line().contains("ACP: disconnected"));
    assert!(!session.summary().is_bound());

    // When: connect + bind
    session.connect().expect("connect");
    session.bind_session("default").expect("bind");
    let bound_summary = session.summary();
    let session_line = session.session().expect("bound").one_line();

    // Then
    assert!(session.state().one_line().contains("ACP: connected"));
    assert!(bound_summary.is_bound());
    assert!(bound_summary.one_line().contains("state=connected"));
    assert!(bound_summary.one_line().contains("session=`acp-session-1`"));
    assert!(bound_summary.one_line().contains("agent=`default`"));
    assert!(session_line.contains("id=`acp-session-1`"));
    assert!(session_line.contains("agent=`default`"));

    // When: transport error marks failed but keeps session for inspection
    session.transport_mut().fail_on_next_operate = true;
    let _ = session.operate(b"ping").expect_err("operate fails");
    assert!(session.state().one_line().contains("ACP: failed"));
    assert!(session.summary().is_bound());
    assert!(session.summary().one_line().contains("state=failed"));
}
