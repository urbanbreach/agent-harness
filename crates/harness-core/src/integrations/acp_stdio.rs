//! Stdio-based ACP transport (real subprocess JSON-RPC peer).
//!
//! Spawns a subprocess for the ACP server and communicates via stdin/stdout
//! using newline-delimited JSON-RPC. Suitable for real ACP agent-mode
//! operation with any subprocess that implements the ACP protocol.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

use super::acp::{
    bind_acp_session_outcome, connect_acp_outcome, AcpBindOutcome, AcpConnectOutcome,
    AcpConnection, AcpConnectionState, AcpConnectionSummary, AcpSessionInfo, AcpTransport,
};

/// Stdio-based ACP transport that spawns a subprocess and communicates
/// via stdin/stdout using newline-delimited JSON-RPC.
pub struct StdioAcpTransport {
    command: String,
    child: Option<Child>,
    connected: bool,
}

impl StdioAcpTransport {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            child: None,
            connected: false,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }
}

impl std::fmt::Debug for StdioAcpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioAcpTransport")
            .field("command", &self.command)
            .field("connected", &self.connected)
            .finish()
    }
}

impl Clone for StdioAcpTransport {
    fn clone(&self) -> Self {
        Self {
            command: self.command.clone(),
            child: None,
            connected: false,
        }
    }
}

impl AcpTransport for StdioAcpTransport {
    fn connect(&mut self) -> Result<(), String> {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &self.command]);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn ACP server `{}`: {e}", self.command))?;
        self.child = Some(child);
        self.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.connected = false;
        Ok(())
    }

    fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        if !self.connected {
            return Err("not connected".to_string());
        }
        let child = self.child.as_mut().ok_or("no subprocess")?;
        let stdin = child.stdin.as_mut().ok_or("no stdin")?;
        let stdout = child.stdout.as_mut().ok_or("no stdout")?;
        stdin
            .write_all(payload)
            .map_err(|e| format!("write to stdin: {e}"))?;
        stdin
            .write_all(b"\n")
            .map_err(|e| format!("write newline: {e}"))?;
        stdin.flush().map_err(|e| format!("flush stdin: {e}"))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("read from stdout: {e}"))?;
        if n == 0 {
            return Err("subprocess closed stdout (no response)".to_string());
        }
        if line.ends_with('\n') {
            line.pop();
        }
        Ok(line.into_bytes())
    }
}

impl Drop for StdioAcpTransport {
    fn drop(&mut self) {
        if self.connected {
            let _ = self.disconnect();
        }
    }
}

/// Stdio-transport ACP agent-mode product result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdioAcpAgentModeProduct {
    pub last_connect: AcpConnectOutcome,
    pub last_bind: AcpBindOutcome,
    pub operate_ok: bool,
    pub summary: AcpConnectionSummary,
    pub state: AcpConnectionState,
    pub session: Option<AcpSessionInfo>,
    pub command: String,
}

impl StdioAcpAgentModeProduct {
    pub fn meets_agent_mode_contract(&self) -> bool {
        self.last_connect.is_connected()
            && self.last_bind.is_bound()
            && self.operate_ok
            && self.summary.is_bound()
            && self
                .session
                .as_ref()
                .is_some_and(|s| !s.session_id.is_empty() && !s.agent_name.is_empty())
    }
}

/// Run stdio-based ACP connect -> bind -> operate product with the given command.
pub fn run_stdio_acp_agent_mode_product(command: &str) -> StdioAcpAgentModeProduct {
    let transport = StdioAcpTransport::new(command);
    let mut acp = AcpConnection::new(transport);
    let last_connect = connect_acp_outcome(&mut acp);
    let last_bind = bind_acp_session_outcome(&mut acp, "harness.probe.agent");
    let operate_ok = acp
        .transport_mut()
        .operate(b"{\"method\":\"initialize\"}")
        .is_ok();
    let _ = acp.transport_mut().disconnect();
    StdioAcpAgentModeProduct {
        last_connect,
        last_bind,
        operate_ok,
        summary: acp.summary(),
        state: acp.state().clone(),
        session: acp.session().cloned(),
        command: command.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_acp_product_connects_and_operates_via_subprocess() {
        // arrange
        // act
        let product = run_stdio_acp_agent_mode_product("cat");

        // assert
        assert!(product.meets_agent_mode_contract(), "{product:?}");
        assert!(product.operate_ok);
    }

    #[test]
    fn stdio_acp_with_invalid_command_fails_connect() {
        // arrange
        // act
        let product = run_stdio_acp_agent_mode_product("exit 1");

        // assert
        assert!(!product.meets_agent_mode_contract());
    }

    #[test]
    fn stdio_acp_operate_without_connect_fails() {
        // arrange
        let mut transport = StdioAcpTransport::new("cat");

        // act
        let err = transport.operate(b"test").expect_err("must fail");

        // assert
        assert!(err.contains("not connected"));
    }
}
