use harness_core::integrations::*;

#[test]
fn acp_failure_unbinds_the_session_and_reconnect_gets_a_new_identity(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut connection = AcpConnection::new(MockAcpTransport::new());
    assert!(connection.operate(b"hello").is_err());
    assert_eq!(connection.transport().operate_calls, 0);
    assert!(connection.bind_session("agent").is_err());
    connection.connect()?;
    let first = connection.bind_session("agent")?.session_id.clone();
    assert!(connection.bind_session("other").is_err());
    assert_eq!(connection.operate(b"hello")?, b"hello");
    connection.transport_mut().fail_on_next_operate = true;
    connection.transport_mut().fail_operate_reason = "access_token=hidden-value".into();
    let error = connection
        .operate(b"hello")
        .err()
        .ok_or("failure expected")?;
    assert!(!error.to_string().contains("hidden-value"));
    assert!(connection.session().is_none());
    assert!(!connection.transport().connected);
    connection.reconnect()?;
    assert_ne!(connection.bind_session("agent")?.session_id, first);
    connection.disconnect()?;
    assert!(!connection.summary().is_bound());
    assert!(connection.state().is_disconnected());
    Ok(())
}

#[cfg(unix)]
#[test]
fn stdio_transport_bounds_frames_and_cleans_up_an_unresponsive_peer(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::integrations::acp_stdio::StdioAcpTransport;
    use std::time::{Duration, Instant};
    let mut peer = StdioAcpTransport::new("cat");
    peer.connect()?;
    assert_eq!(peer.operate(br#"{"id":1}"#)?, br#"{"id":1}"#);
    assert_eq!(peer.operate(br#"{"id":2}"#)?, br#"{"id":2}"#);
    assert!(peer.operate(&vec![b'x'; 1024 * 1024 + 1]).is_err());
    peer.disconnect()?;
    assert!(!peer.is_connected());
    let mut stalled = StdioAcpTransport::with_timeout("sleep 30", Duration::from_millis(50));
    stalled.connect()?;
    let start = Instant::now();
    assert!(stalled.operate(b"hello").is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(!stalled.is_connected());
    Ok(())
}
