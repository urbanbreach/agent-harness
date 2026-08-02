//! File-backed ACP transport (durable local peer; no live IDE).
//!
//! Frames are written under a transport directory so connect/operate leave
//! real filesystem side effects. Suitable for operator product walks and
//! offline ACP agent-mode proof without a network editor.

use std::fs;
use std::path::{Path, PathBuf};

use super::acp::{
    bind_acp_session_outcome, connect_acp_outcome, AcpBindOutcome, AcpConnectOutcome,
    AcpConnection, AcpConnectionState, AcpConnectionSummary, AcpSessionInfo, AcpTransport,
};

/// Relative transport root under a workspace.
pub const ACP_FILE_TRANSPORT_REL: &str = ".agent-harness/acp-transport";
/// Connect marker file name.
pub const ACP_CONNECTED_MARKER: &str = "connected.marker";
/// Frame log file name (append-only operate payloads).
pub const ACP_FRAME_LOG: &str = "frames.jsonl";

/// File-backed ACP transport with durable connect/operate side effects.
#[derive(Debug, Clone)]
pub struct FileAcpTransport {
    root: PathBuf,
    connected: bool,
    operate_calls: usize,
}

impl FileAcpTransport {
    /// Create a transport rooted at `workspace_root/.agent-harness/acp-transport`.
    pub fn new(workspace_root: impl AsRef<Path>) -> Self {
        Self {
            root: workspace_root.as_ref().join(ACP_FILE_TRANSPORT_REL),
            connected: false,
            operate_calls: 0,
        }
    }

    pub fn transport_root(&self) -> &Path {
        &self.root
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn operate_calls(&self) -> usize {
        self.operate_calls
    }

    fn marker_path(&self) -> PathBuf {
        self.root.join(ACP_CONNECTED_MARKER)
    }

    fn frame_log_path(&self) -> PathBuf {
        self.root.join(ACP_FRAME_LOG)
    }

    fn ensure_root(&self) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| format!("create acp transport root: {e}"))
    }
}

impl AcpTransport for FileAcpTransport {
    fn connect(&mut self) -> Result<(), String> {
        self.ensure_root()?;
        let marker = self.marker_path();
        fs::write(&marker, b"connected\n").map_err(|e| format!("write connect marker: {e}"))?;
        self.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        let marker = self.marker_path();
        if marker.is_file() {
            fs::remove_file(&marker).map_err(|e| format!("remove connect marker: {e}"))?;
        }
        self.connected = false;
        Ok(())
    }

    fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        if !self.connected {
            return Err("not connected".to_string());
        }
        self.ensure_root()?;
        self.operate_calls = self.operate_calls.saturating_add(1);
        let line = serde_json::json!({
            "seq": self.operate_calls,
            "payloadB64": base64_simple(payload),
            "len": payload.len(),
        });
        let mut entry = serde_json::to_string(&line).map_err(|e| format!("frame encode: {e}"))?;
        entry.push('\n');
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.frame_log_path())
            .map_err(|e| format!("open frame log: {e}"))?;
        file.write_all(entry.as_bytes())
            .map_err(|e| format!("write frame: {e}"))?;
        // Echo payload as framed response (local peer).
        Ok(payload.to_vec())
    }
}

/// Minimal base64 for durable frame logs (no extra dependency).
fn base64_simple(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

/// File-transport ACP agent-mode product result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAcpAgentModeProduct {
    pub connect: AcpConnectOutcome,
    pub bind: AcpBindOutcome,
    pub operate_ok: bool,
    pub summary: AcpConnectionSummary,
    pub state: AcpConnectionState,
    pub session: Option<AcpSessionInfo>,
    pub transport_root: PathBuf,
    pub frame_log_exists: bool,
    pub marker_exists: bool,
}

impl FileAcpAgentModeProduct {
    /// Product honesty: connected, bound, operate wrote durable frames.
    pub fn meets_agent_mode_contract(&self) -> bool {
        self.connect.is_connected()
            && self.bind.is_bound()
            && self.operate_ok
            && self.summary.is_bound()
            && self.marker_exists
            && self.frame_log_exists
            && self
                .session
                .as_ref()
                .is_some_and(|s| !s.session_id.is_empty() && !s.agent_name.is_empty())
    }
}

/// Run file-backed ACP connect → bind → operate product under `workspace_root`.
pub fn run_file_acp_agent_mode_product(
    workspace_root: impl AsRef<Path>,
) -> FileAcpAgentModeProduct {
    let root = workspace_root.as_ref();
    let transport = FileAcpTransport::new(root);
    let transport_root = transport.transport_root().to_path_buf();
    let mut acp = AcpConnection::new(transport);
    let connect = connect_acp_outcome(&mut acp);
    let bind = bind_acp_session_outcome(&mut acp, "harness.file.acp.agent");
    let operate_ok = acp
        .transport_mut()
        .operate(b"{\"method\":\"initialize\"}")
        .is_ok();
    let marker_exists = transport_root.join(ACP_CONNECTED_MARKER).is_file();
    let frame_log_exists = transport_root.join(ACP_FRAME_LOG).is_file();
    FileAcpAgentModeProduct {
        connect,
        bind,
        operate_ok,
        summary: acp.summary(),
        state: acp.state().clone(),
        session: acp.session().cloned(),
        transport_root,
        frame_log_exists,
        marker_exists,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn file_acp_product_writes_marker_and_frames() {
        // arrange
        // act
        // assert
        let dir = tempdir().expect("tempdir");
        let product = run_file_acp_agent_mode_product(dir.path());
        assert!(product.meets_agent_mode_contract(), "{product:?}");
        let log = fs::read_to_string(product.transport_root.join(ACP_FRAME_LOG)).expect("log");
        assert!(log.contains("\"seq\":1"));
        assert!(product.transport_root.join(ACP_CONNECTED_MARKER).is_file());
    }

    #[test]
    fn frames_journal_appends_one_line_per_operate() {
        // arrange
        let dir = tempdir().expect("tempdir");
        let mut transport = FileAcpTransport::new(dir.path());

        // act
        transport.connect().expect("connect");
        transport.operate(b"frame-one").expect("operate one");
        transport.operate(b"frame-two").expect("operate two");

        // assert — durable marker + one journal line per operate, honest counter
        assert!(transport.is_connected());
        assert_eq!(transport.operate_calls(), 2);
        let frames = std::fs::read_to_string(transport.transport_root().join(ACP_FRAME_LOG))
            .expect("frames journal written");
        assert_eq!(frames.lines().count(), 2);
    }

    #[test]
    fn file_acp_operate_is_local_echo_not_protocol_exchange() {
        // arrange — a connected local file transport
        let dir = tempdir().expect("tempdir");
        let mut transport = FileAcpTransport::new(dir.path());
        transport.connect().expect("connect");

        // act
        let response = transport.operate(b"ping-12").expect("operate");

        // assert — the mock echoes bytes verbatim and journals frame metadata
        assert_eq!(response, b"ping-12");
        let frames = std::fs::read_to_string(transport.transport_root().join(ACP_FRAME_LOG))
            .expect("frames journal");
        let entry: serde_json::Value =
            serde_json::from_str(frames.lines().next().expect("frame line")).expect("json frame");
        assert_eq!(entry["seq"], 1);
        assert_eq!(entry["len"], 7);
        assert!(entry["payloadB64"].is_string());
    }

    #[test]
    fn operate_without_connect_fails() {
        // arrange
        // act
        // assert
        let dir = tempdir().expect("tempdir");
        let mut transport = FileAcpTransport::new(dir.path());
        let err = transport.operate(b"x").expect_err("must fail");
        assert!(err.contains("not connected"));
    }
}
