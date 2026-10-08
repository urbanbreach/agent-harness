//! Explicit fixtures for the preserved TUI diagnostics tests. Never called during runtime startup.
use super::*;
pub const PROBE_ACP_AGENT_NAME: &str = "harness.probe.agent";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockAcpAgentModeProduct {
    pub fail_connect: AcpConnectOutcome,
    pub fail_bind: AcpBindOutcome,
    pub last_connect: AcpConnectOutcome,
    pub last_bind: AcpBindOutcome,
    pub summary: AcpConnectionSummary,
    pub state: AcpConnectionState,
    pub session: Option<AcpSessionInfo>,
}
impl MockAcpAgentModeProduct {
    pub fn meets_agent_mode_contract(&self) -> bool {
        matches!(self.fail_connect, AcpConnectOutcome::Failed { .. })
            && matches!(self.fail_bind, AcpBindOutcome::Failed { .. })
            && self.last_connect.is_connected()
            && self.last_bind.is_bound()
            && self.summary.is_bound()
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.agent_name == PROBE_ACP_AGENT_NAME)
    }
}
pub fn run_mock_acp_agent_mode_product() -> MockAcpAgentModeProduct {
    let mut failed = AcpConnection::new(MockAcpTransport {
        fail_connect: true,
        ..MockAcpTransport::new()
    });
    let fail_connect = connect_acp_outcome(&mut failed);
    let fail_bind = bind_acp_session_outcome(&mut failed, PROBE_ACP_AGENT_NAME);
    let mut connection = AcpConnection::new(MockAcpTransport::new());
    let last_connect = connect_acp_outcome(&mut connection);
    let last_bind = bind_acp_session_outcome(&mut connection, PROBE_ACP_AGENT_NAME);
    MockAcpAgentModeProduct {
        fail_connect,
        fail_bind,
        last_connect,
        last_bind,
        summary: connection.summary(),
        state: connection.state().clone(),
        session: connection.session().cloned(),
    }
}
