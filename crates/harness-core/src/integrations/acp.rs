//! ACP connection lifecycle foundation (state machine + stub transport).
//!
//! Full ACP protocol framing, IDE/editor transports, and remote workspaces are
//! **not** implemented here. This module owns the connection state machine and a
//! testable transport seam so higher layers can wire a real protocol later.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Connection state for an ACP agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AcpConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed { reason: String },
}

impl AcpConnectionState {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Failed { .. } => "failed",
        }
    }

    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    pub const fn is_terminal_idle(&self) -> bool {
        matches!(self, Self::Disconnected | Self::Failed { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim full ACP protocol).
    pub fn one_line(&self) -> String {
        match self {
            Self::Disconnected => "ACP: disconnected".to_string(),
            Self::Connecting => "ACP: connecting".to_string(),
            Self::Connected => "ACP: connected".to_string(),
            Self::Failed { reason } => format!("ACP: failed ({reason})"),
        }
    }
}

impl fmt::Display for AcpConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { reason } => write!(f, "failed({reason})"),
            other => f.write_str(other.as_str()),
        }
    }
}

/// Errors from the ACP connection state machine.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AcpError {
    #[error("ACP connect is not allowed from state `{from}`")]
    InvalidConnectState { from: String },
    #[error("ACP disconnect is not allowed from state `{from}`")]
    InvalidDisconnectState { from: String },
    #[error("ACP operation requires Connected state (current={current})")]
    NotConnected { current: String },
    #[error("ACP session bind requires Connected state (current={current})")]
    SessionBindNotConnected { current: String },
    #[error("ACP session already bound (session_id={session_id})")]
    SessionAlreadyBound { session_id: String },
    #[error("ACP agent name must be non-empty")]
    EmptyAgentName,
    #[error("ACP transport failed: {0}")]
    Transport(String),
    #[error("ACP operation aborted: {0}")]
    OperationAborted(String),
}

/// Bound ACP session metadata (local bookkeeping; not full ACP protocol).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSessionInfo {
    pub session_id: String,
    pub agent_name: String,
}

impl AcpSessionInfo {
    /// Operator-facing one-line session identity.
    pub fn one_line(&self) -> String {
        format!(
            "ACP session: id=`{}` agent=`{}`",
            self.session_id, self.agent_name
        )
    }
}

/// Operator-facing snapshot of an ACP connection (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpConnectionSummary {
    pub state: String,
    pub session_id: Option<String>,
    pub agent_name: Option<String>,
    pub bound: bool,
}

impl AcpConnectionSummary {
    pub fn one_line(&self) -> String {
        match (self.session_id.as_deref(), self.agent_name.as_deref()) {
            (Some(session_id), Some(agent_name)) => format!(
                "ACP: state={} session=`{}` agent=`{}`",
                self.state, session_id, agent_name
            ),
            _ => format!("ACP: state={} session=none", self.state),
        }
    }

    pub const fn is_bound(&self) -> bool {
        self.bound
    }
}

/// Offline-stubbable ACP transport.
///
/// Implementations must not assume a live network. Production transports can
/// wrap stdio/socket/JSON-RPC later without changing the state machine.
pub trait AcpTransport {
    fn connect(&mut self) -> Result<(), String>;
    fn disconnect(&mut self) -> Result<(), String>;
    /// Perform a framed operation while connected.
    ///
    /// On mid-flight disconnect or transport loss, return `Err` — the session
    /// must not report success.
    fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, String>;
}

/// Coordinator-owned ACP session wrapper around a transport.
#[derive(Debug, Clone)]
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
            next_session_seq: 1,
        }
    }

    pub fn state(&self) -> &AcpConnectionState {
        &self.state
    }

    pub fn session(&self) -> Option<&AcpSessionInfo> {
        self.session.as_ref()
    }

    /// Operator-facing connection snapshot for CLI/status surfaces.
    pub fn summary(&self) -> AcpConnectionSummary {
        AcpConnectionSummary {
            state: self.state.as_str().to_string(),
            session_id: self.session.as_ref().map(|s| s.session_id.clone()),
            agent_name: self.session.as_ref().map(|s| s.agent_name.clone()),
            bound: self.session.is_some(),
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Bind a local session while Connected.
    ///
    /// This is coordinator-owned bookkeeping only — not full ACP initialize protocol.
    pub fn bind_session(
        &mut self,
        agent_name: impl Into<String>,
    ) -> Result<&AcpSessionInfo, AcpError> {
        if !self.state.is_connected() {
            return Err(AcpError::SessionBindNotConnected {
                current: self.state.to_string(),
            });
        }
        if let Some(existing) = &self.session {
            return Err(AcpError::SessionAlreadyBound {
                session_id: existing.session_id.clone(),
            });
        }
        let agent_name = agent_name.into();
        if agent_name.trim().is_empty() {
            return Err(AcpError::EmptyAgentName);
        }
        let session_id = format!("acp-session-{}", self.next_session_seq);
        self.next_session_seq = self.next_session_seq.saturating_add(1);
        self.session = Some(AcpSessionInfo {
            session_id,
            agent_name,
        });
        Ok(self.session.as_ref().expect("session just bound"))
    }

    /// Transition Disconnected|Failed → Connecting → Connected|Failed.
    pub fn connect(&mut self) -> Result<(), AcpError> {
        match &self.state {
            AcpConnectionState::Disconnected | AcpConnectionState::Failed { .. } => {}
            other => {
                return Err(AcpError::InvalidConnectState {
                    from: other.to_string(),
                });
            }
        }

        self.state = AcpConnectionState::Connecting;
        match self.transport.connect() {
            Ok(()) => {
                self.state = AcpConnectionState::Connected;
                Ok(())
            }
            Err(reason) => {
                self.state = AcpConnectionState::Failed {
                    reason: reason.clone(),
                };
                Err(AcpError::Transport(reason))
            }
        }
    }

    /// Transition Connected|Connecting|Failed → Disconnected (best-effort transport teardown).
    pub fn disconnect(&mut self) -> Result<(), AcpError> {
        match &self.state {
            AcpConnectionState::Disconnected => {
                self.session = None;
                Ok(())
            }
            AcpConnectionState::Connecting
            | AcpConnectionState::Connected
            | AcpConnectionState::Failed { .. } => {
                let transport_result = self.transport.disconnect();
                self.state = AcpConnectionState::Disconnected;
                self.session = None;
                transport_result.map_err(AcpError::Transport)
            }
        }
    }

    /// Disconnect (if needed) then connect again.
    pub fn reconnect(&mut self) -> Result<(), AcpError> {
        if !matches!(self.state, AcpConnectionState::Disconnected) {
            // Ignore transport teardown errors; reconnect still attempts a clean connect.
            let _ = self.disconnect();
        }
        self.connect()
    }

    /// Run an operation only while Connected.
    ///
    /// Mid-operation transport failure moves the session to
    /// [`AcpConnectionState::Failed`] (or [`AcpConnectionState::Disconnected`]
    /// when the transport reports a clean drop) and returns `Err` — never success.
    pub fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, AcpError> {
        if !self.state.is_connected() {
            return Err(AcpError::NotConnected {
                current: self.state.to_string(),
            });
        }

        match self.transport.operate(payload) {
            Ok(response) => Ok(response),
            Err(reason) => {
                let lower = reason.to_ascii_lowercase();
                if lower.contains("disconnect") || lower.contains("dropped") {
                    // Clean remote/local drop → Disconnected; still not success.
                    let _ = self.transport.disconnect();
                    self.state = AcpConnectionState::Disconnected;
                    self.session = None;
                    Err(AcpError::OperationAborted(reason))
                } else {
                    self.state = AcpConnectionState::Failed {
                        reason: reason.clone(),
                    };
                    // Keep session metadata on Failed so operators can inspect which
                    // session failed; disconnect/reconnect clears it.
                    Err(AcpError::OperationAborted(reason))
                }
            }
        }
    }
}

/// Structured operator-facing outcome for an ACP connect attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AcpConnectOutcome {
    Connected,
    Failed { reason: String },
}

impl AcpConnectOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Connected => "ACP connect: ok".to_string(),
            Self::Failed { reason } => format!("ACP connect: failed ({reason})"),
        }
    }

    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// Structured operator-facing outcome for an ACP session bind attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    pub const fn is_bound(&self) -> bool {
        matches!(self, Self::Bound { .. })
    }
}

/// Attempt ACP connect and return a structured operator-facing outcome.
pub fn connect_acp_outcome<T: AcpTransport>(conn: &mut AcpConnection<T>) -> AcpConnectOutcome {
    match conn.connect() {
        Ok(()) => AcpConnectOutcome::Connected,
        Err(err) => AcpConnectOutcome::Failed {
            reason: err.to_string(),
        },
    }
}

/// Attempt ACP session bind and return a structured operator-facing outcome.
pub fn bind_acp_session_outcome<T: AcpTransport>(
    conn: &mut AcpConnection<T>,
    agent_name: impl Into<String>,
) -> AcpBindOutcome {
    match conn.bind_session(agent_name) {
        Ok(session) => AcpBindOutcome::Bound {
            session_id: session.session_id.clone(),
            agent_name: session.agent_name.clone(),
        },
        Err(err) => AcpBindOutcome::Failed {
            reason: err.to_string(),
        },
    }
}

/// In-memory mock transport for offline unit tests.
#[derive(Debug, Clone, Default)]
pub struct MockAcpTransport {
    pub connected: bool,
    pub fail_connect: bool,
    pub fail_connect_reason: String,
    /// When true, the next `operate` fails with a disconnect-style error.
    pub disconnect_on_next_operate: bool,
    /// When true, the next `operate` fails with a non-disconnect transport error.
    pub fail_on_next_operate: bool,
    pub fail_operate_reason: String,
    pub operate_calls: usize,
    pub last_payload: Option<Vec<u8>>,
}

impl MockAcpTransport {
    pub fn new() -> Self {
        Self {
            fail_connect_reason: "mock connect failed".to_string(),
            fail_operate_reason: "mock operate failed".to_string(),
            ..Self::default()
        }
    }
}

impl AcpTransport for MockAcpTransport {
    fn connect(&mut self) -> Result<(), String> {
        if self.fail_connect {
            self.connected = false;
            return Err(self.fail_connect_reason.clone());
        }
        self.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        self.connected = false;
        Ok(())
    }

    fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        self.operate_calls = self.operate_calls.saturating_add(1);
        self.last_payload = Some(payload.to_vec());
        if !self.connected {
            return Err("not connected".to_string());
        }
        if self.disconnect_on_next_operate {
            self.disconnect_on_next_operate = false;
            self.connected = false;
            return Err("peer disconnected during operation".to_string());
        }
        if self.fail_on_next_operate {
            self.fail_on_next_operate = false;
            return Err(self.fail_operate_reason.clone());
        }
        Ok(payload.to_vec())
    }
}
