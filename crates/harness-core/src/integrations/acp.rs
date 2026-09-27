use serde::{Deserialize, Serialize};
mod mock;
pub use mock::MockAcpTransport;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AcpConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed { reason: String },
}
impl AcpConnectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Failed { .. } => "failed",
        }
    }
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
    pub fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected)
    }
    pub fn is_terminal_idle(&self) -> bool {
        matches!(self, Self::Disconnected | Self::Failed { .. })
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Failed { reason } => format!("ACP: failed ({reason})"),
            other => format!("ACP: {}", other.as_str()),
        }
    }
}
impl std::fmt::Display for AcpConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AcpError {
    #[error("ACP connect is not allowed from `{from}`")]
    InvalidConnectState { from: String },
    #[error("ACP disconnect is not allowed from `{from}`")]
    InvalidDisconnectState { from: String },
    #[error("ACP operation requires Connected state (current={current})")]
    NotConnected { current: String },
    #[error("ACP session bind requires Connected state (current={current})")]
    SessionBindNotConnected { current: String },
    #[error("ACP session already bound ({session_id})")]
    SessionAlreadyBound { session_id: String },
    #[error("ACP agent name must be non-empty")]
    EmptyAgentName,
    #[error("ACP transport failed: {0}")]
    Transport(String),
    #[error("ACP operation aborted: {0}")]
    OperationAborted(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSessionInfo {
    pub session_id: String,
    pub agent_name: String,
}
impl AcpSessionInfo {
    pub fn one_line(&self) -> String {
        format!(
            "ACP session: id=`{}` agent=`{}`",
            self.session_id, self.agent_name
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpConnectionSummary {
    pub state: String,
    pub session_id: Option<String>,
    pub agent_name: Option<String>,
    pub bound: bool,
}
impl AcpConnectionSummary {
    pub fn one_line(&self) -> String {
        match (&self.session_id, &self.agent_name) {
            (Some(id), Some(name)) => {
                format!("ACP: state={} session=`{id}` agent=`{name}`", self.state)
            }
            _ => format!("ACP: state={} session=none", self.state),
        }
    }
    pub fn is_bound(&self) -> bool {
        self.bound
    }
}
pub trait AcpTransport {
    fn connect(&mut self) -> Result<(), String>;
    fn disconnect(&mut self) -> Result<(), String>;
    fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, String>;
}
pub struct AcpConnection<T: AcpTransport> {
    state: AcpConnectionState,
    transport: T,
    session: Option<AcpSessionInfo>,
    next_session_seq: u64,
}
impl<T: AcpTransport> AcpConnection<T> {
    pub fn new(transport: T) -> Self {
        Self {
            state: AcpConnectionState::Disconnected,
            transport,
            session: None,
            next_session_seq: 0,
        }
    }
    pub fn state(&self) -> &AcpConnectionState {
        &self.state
    }
    pub fn session(&self) -> Option<&AcpSessionInfo> {
        self.session.as_ref()
    }
    pub fn transport(&self) -> &T {
        &self.transport
    }
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }
    pub fn summary(&self) -> AcpConnectionSummary {
        AcpConnectionSummary {
            state: self.state.as_str().into(),
            session_id: self.session.as_ref().map(|s| s.session_id.clone()),
            agent_name: self.session.as_ref().map(|s| s.agent_name.clone()),
            bound: self.session.is_some(),
        }
    }
    /// Local session bookkeeping; this is not an ACP initialize handshake.
    pub fn bind_session(
        &mut self,
        agent_name: impl Into<String>,
    ) -> Result<&AcpSessionInfo, AcpError> {
        if !self.state.is_connected() {
            return Err(AcpError::SessionBindNotConnected {
                current: self.state.to_string(),
            });
        }
        if let Some(s) = &self.session {
            return Err(AcpError::SessionAlreadyBound {
                session_id: s.session_id.clone(),
            });
        }
        let agent_name = agent_name.into();
        let agent_name = agent_name.trim();
        if agent_name.is_empty() {
            return Err(AcpError::EmptyAgentName);
        }
        if agent_name.len() > 256
            || agent_name.chars().any(char::is_control)
            || crate::redact::redact_artifact_text(agent_name) != agent_name
        {
            return Err(AcpError::OperationAborted("invalid agent name".into()));
        }
        self.next_session_seq = self
            .next_session_seq
            .checked_add(1)
            .ok_or_else(|| AcpError::OperationAborted("session counter exhausted".into()))?;
        Ok(self.session.insert(AcpSessionInfo {
            session_id: format!("acp_session_{}", self.next_session_seq),
            agent_name: agent_name.into(),
        }))
    }
    pub fn connect(&mut self) -> Result<(), AcpError> {
        if !self.state.is_terminal_idle() {
            return Err(AcpError::InvalidConnectState {
                from: self.state.to_string(),
            });
        }
        self.state = AcpConnectionState::Connecting;
        self.session = None;
        match self.transport.connect() {
            Ok(()) => {
                self.state = AcpConnectionState::Connected;
                Ok(())
            }
            Err(_) => Err(self.fail("connect failed")),
        }
    }
    pub fn disconnect(&mut self) -> Result<(), AcpError> {
        self.session = None;
        if self.state.is_disconnected() {
            return Ok(());
        }
        let result = self.transport.disconnect();
        self.state = AcpConnectionState::Disconnected;
        result.map_err(|_| AcpError::Transport("disconnect failed".into()))
    }
    pub fn reconnect(&mut self) -> Result<(), AcpError> {
        self.disconnect()?;
        self.connect()
    }
    pub fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, AcpError> {
        if !self.state.is_connected() {
            return Err(AcpError::NotConnected {
                current: self.state.to_string(),
            });
        }
        if payload.len() > 1024 * 1024 {
            return Err(AcpError::OperationAborted("payload exceeds 1 MiB".into()));
        }
        match self.transport.operate(payload) {
            Ok(response) if response.len() <= 1024 * 1024 => Ok(response),
            Ok(_) => Err(self.fail("response exceeds 1 MiB")),
            Err(_) => Err(self.fail("operation failed")),
        }
    }
    fn fail(&mut self, reason: &str) -> AcpError {
        let _ = self.transport.disconnect();
        self.session = None;
        self.state = AcpConnectionState::Failed {
            reason: reason.into(),
        };
        AcpError::Transport(reason.into())
    }
}
impl<T: AcpTransport> Drop for AcpConnection<T> {
    fn drop(&mut self) {
        let _ = self.transport.disconnect();
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AcpConnectOutcome {
    Connected,
    Failed { reason: String },
}
impl AcpConnectOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Connected => "ACP connect: ok".into(),
            Self::Failed { reason } => format!("ACP connect: failed ({reason})"),
        }
    }
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AcpBindOutcome {
    Bound {
        session_id: String,
        agent_name: String,
    },
    Failed {
        reason: String,
    },
}
impl AcpBindOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Bound {
                session_id,
                agent_name,
            } => format!("ACP bind: ok session=`{session_id}` agent=`{agent_name}`"),
            Self::Failed { reason } => format!("ACP bind: failed ({reason})"),
        }
    }
    pub fn is_bound(&self) -> bool {
        matches!(self, Self::Bound { .. })
    }
}
pub fn connect_acp_outcome<T: AcpTransport>(
    connection: &mut AcpConnection<T>,
) -> AcpConnectOutcome {
    match connection.connect() {
        Ok(()) => AcpConnectOutcome::Connected,
        Err(e) => AcpConnectOutcome::Failed {
            reason: e.to_string(),
        },
    }
}
pub fn bind_acp_session_outcome<T: AcpTransport>(
    connection: &mut AcpConnection<T>,
    name: impl Into<String>,
) -> AcpBindOutcome {
    match connection.bind_session(name) {
        Ok(s) => AcpBindOutcome::Bound {
            session_id: s.session_id.clone(),
            agent_name: s.agent_name.clone(),
        },
        Err(e) => AcpBindOutcome::Failed {
            reason: e.to_string(),
        },
    }
}
