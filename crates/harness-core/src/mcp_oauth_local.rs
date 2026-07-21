//! Local durable MCP OAuth + transport product (file-backed; no live remote).
//!
//! Implements begin → exchange → open with durable token state and receipts
//! under `.agent-harness/mcp-oauth/` so operator surfaces exercise a real
//! OAuth remote-transport lifecycle without network peers.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Relative state path under a workspace.
pub const MCP_OAUTH_STATE_REL: &str = ".agent-harness/mcp-oauth/state.json";
/// Relative receipt path written after open.
pub const MCP_OAUTH_RECEIPT_REL: &str = ".agent-harness/mcp-oauth/open.receipt.json";

/// Durable MCP OAuth session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalMcpOauthState {
    pub server_id: Option<String>,
    pub authorization_url: Option<String>,
    pub authorization_code: Option<String>,
    /// Redacted access token storage (never raw secret in logs).
    pub access_token_hint: Option<String>,
    pub endpoint: Option<String>,
    pub begun: bool,
    pub exchanged: bool,
    pub opened: bool,
    pub seq: u64,
}

/// Begin outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LocalMcpOauthBeginResult {
    Begun {
        server_id: String,
        authorization_url: String,
        authorization_code: String,
    },
    Failed {
        reason: String,
    },
}

impl LocalMcpOauthBeginResult {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Begun { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Begun {
                server_id,
                authorization_url,
                ..
            } => format!("local MCP OAuth begin: server={server_id} url={authorization_url}"),
            Self::Failed { reason } => format!("local MCP OAuth begin: failed ({reason})"),
        }
    }
}

/// Token exchange outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LocalMcpOauthExchangeResult {
    Exchanged {
        server_id: String,
        access_token_hint: String,
    },
    Failed {
        reason: String,
    },
}

impl LocalMcpOauthExchangeResult {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Exchanged { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Exchanged {
                server_id,
                access_token_hint,
            } => format!("local MCP OAuth exchange: server={server_id} token={access_token_hint}"),
            Self::Failed { reason } => format!("local MCP OAuth exchange: failed ({reason})"),
        }
    }
}

/// Remote transport open outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LocalMcpTransportOpenResult {
    Opened {
        server_id: String,
        endpoint: String,
        transport: String,
    },
    Failed {
        reason: String,
    },
}

impl LocalMcpTransportOpenResult {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Opened { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Opened {
                server_id,
                endpoint,
                transport,
            } => format!(
                "local MCP transport open: server={server_id} endpoint={endpoint} transport={transport}"
            ),
            Self::Failed { reason } => format!("local MCP transport open: failed ({reason})"),
        }
    }
}

/// Errors from local MCP OAuth product paths.
#[derive(Debug)]
pub enum LocalMcpOauthError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    NotBegun,
    NotExchanged,
    CodeMismatch,
    EmptyInput {
        field: &'static str,
    },
}

impl std::fmt::Display for LocalMcpOauthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "io error at {}: {source}", path.display()),
            Self::Serialize { path, source } => {
                write!(f, "serialize error at {}: {source}", path.display())
            }
            Self::NotBegun => write!(f, "MCP OAuth flow not begun"),
            Self::NotExchanged => write!(f, "MCP OAuth token not exchanged"),
            Self::CodeMismatch => write!(f, "authorization code mismatch"),
            Self::EmptyInput { field } => write!(f, "empty input for {field}"),
        }
    }
}

impl std::error::Error for LocalMcpOauthError {}

fn redact_token(token: &str) -> String {
    if token.len() <= 4 {
        return "****".to_string();
    }
    format!("{}…", &token[..4])
}

/// File-backed MCP OAuth + transport session.
#[derive(Debug, Clone)]
pub struct LocalMcpOauth {
    root: PathBuf,
    state: LocalMcpOauthState,
    /// Full access token kept only in memory (never written raw).
    access_token: Option<String>,
}

impl LocalMcpOauth {
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, LocalMcpOauthError> {
        let root = workspace_root.as_ref().to_path_buf();
        let state_path = root.join(MCP_OAUTH_STATE_REL);
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent).map_err(|source| LocalMcpOauthError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let state = if state_path.is_file() {
            let body =
                fs::read_to_string(&state_path).map_err(|source| LocalMcpOauthError::Io {
                    path: state_path.clone(),
                    source,
                })?;
            serde_json::from_str(&body).map_err(|source| LocalMcpOauthError::Serialize {
                path: state_path,
                source,
            })?
        } else {
            LocalMcpOauthState::default()
        };
        Ok(Self {
            root,
            state,
            access_token: None,
        })
    }

    pub fn state(&self) -> &LocalMcpOauthState {
        &self.state
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(MCP_OAUTH_STATE_REL)
    }

    fn receipt_path(&self) -> PathBuf {
        self.root.join(MCP_OAUTH_RECEIPT_REL)
    }

    fn persist(&self) -> Result<(), LocalMcpOauthError> {
        let path = self.state_path();
        let body = serde_json::to_string_pretty(&self.state).map_err(|source| {
            LocalMcpOauthError::Serialize {
                path: path.clone(),
                source,
            }
        })?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, format!("{body}\n")).map_err(|source| LocalMcpOauthError::Io {
            path: temp.clone(),
            source,
        })?;
        fs::rename(&temp, &path).map_err(|source| LocalMcpOauthError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(())
    }

    /// Begin an OAuth authorization flow for an MCP server.
    pub fn begin(
        &mut self,
        server_id: impl Into<String>,
        authorization_url: impl Into<String>,
    ) -> Result<LocalMcpOauthBeginResult, LocalMcpOauthError> {
        let server_id = server_id.into();
        let authorization_url = authorization_url.into();
        if server_id.trim().is_empty() {
            return Err(LocalMcpOauthError::EmptyInput { field: "server_id" });
        }
        if authorization_url.trim().is_empty() {
            return Err(LocalMcpOauthError::EmptyInput {
                field: "authorization_url",
            });
        }
        let authorization_code = format!("mcp-code-{}", self.state.seq.saturating_add(1));
        self.state.server_id = Some(server_id.clone());
        self.state.authorization_url = Some(authorization_url.clone());
        self.state.authorization_code = Some(authorization_code.clone());
        self.state.begun = true;
        self.state.exchanged = false;
        self.state.opened = false;
        self.state.access_token_hint = None;
        self.state.endpoint = None;
        self.access_token = None;
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;
        Ok(LocalMcpOauthBeginResult::Begun {
            server_id,
            authorization_url,
            authorization_code,
        })
    }

    /// Exchange authorization code for a local access token.
    pub fn exchange(
        &mut self,
        authorization_code: impl Into<String>,
    ) -> Result<LocalMcpOauthExchangeResult, LocalMcpOauthError> {
        if !self.state.begun {
            return Err(LocalMcpOauthError::NotBegun);
        }
        let code = authorization_code.into();
        let expected = self.state.authorization_code.as_deref().unwrap_or_default();
        if code != expected {
            return Err(LocalMcpOauthError::CodeMismatch);
        }
        let server_id = self
            .state
            .server_id
            .clone()
            .unwrap_or_else(|| "mcp-server".to_string());
        let token = format!("mcp-tok-{}", self.state.seq.saturating_add(1));
        let hint = redact_token(&token);
        self.access_token = Some(token);
        self.state.access_token_hint = Some(hint.clone());
        self.state.exchanged = true;
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;
        Ok(LocalMcpOauthExchangeResult::Exchanged {
            server_id,
            access_token_hint: hint,
        })
    }

    /// Open a local remote transport using the exchanged token.
    pub fn open_transport(
        &mut self,
        endpoint: impl Into<String>,
    ) -> Result<LocalMcpTransportOpenResult, LocalMcpOauthError> {
        if !self.state.exchanged || self.access_token.is_none() {
            return Err(LocalMcpOauthError::NotExchanged);
        }
        let endpoint = endpoint.into();
        if endpoint.trim().is_empty() {
            return Err(LocalMcpOauthError::EmptyInput { field: "endpoint" });
        }
        let server_id = self
            .state
            .server_id
            .clone()
            .unwrap_or_else(|| "mcp-server".to_string());
        let transport = "local-file".to_string();
        self.state.endpoint = Some(endpoint.clone());
        self.state.opened = true;
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;

        let receipt = serde_json::json!({
            "status": "opened",
            "serverId": server_id,
            "endpoint": endpoint,
            "transport": transport,
            "accessTokenHint": self.state.access_token_hint,
        });
        let receipt_path = self.receipt_path();
        let body = serde_json::to_string_pretty(&receipt).map_err(|source| {
            LocalMcpOauthError::Serialize {
                path: receipt_path.clone(),
                source,
            }
        })?;
        fs::write(&receipt_path, format!("{body}\n")).map_err(|source| LocalMcpOauthError::Io {
            path: receipt_path,
            source,
        })?;

        Ok(LocalMcpTransportOpenResult::Opened {
            server_id,
            endpoint,
            transport,
        })
    }
}

/// Full product walk: begin → exchange → open with durable state + receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMcpOauthProduct {
    pub begin: LocalMcpOauthBeginResult,
    pub exchange: LocalMcpOauthExchangeResult,
    pub open: LocalMcpTransportOpenResult,
    pub state_path: PathBuf,
    pub receipt_path: PathBuf,
}

impl LocalMcpOauthProduct {
    pub fn meets_contract(&self) -> bool {
        self.begin.is_success()
            && self.exchange.is_success()
            && self.open.is_success()
            && self.state_path.is_file()
            && self.receipt_path.is_file()
    }
}

/// Run local MCP OAuth + transport product under `workspace_root`.
pub fn run_local_mcp_oauth_product(
    workspace_root: impl AsRef<Path>,
) -> Result<LocalMcpOauthProduct, LocalMcpOauthError> {
    let root = workspace_root.as_ref();
    let mut oauth = LocalMcpOauth::open(root)?;
    let begin = oauth.begin("mcp-local-server", "https://mcp.local/oauth/authorize")?;
    let code = match &begin {
        LocalMcpOauthBeginResult::Begun {
            authorization_code, ..
        } => authorization_code.clone(),
        LocalMcpOauthBeginResult::Failed { reason } => {
            return Ok(LocalMcpOauthProduct {
                begin: LocalMcpOauthBeginResult::Failed {
                    reason: reason.clone(),
                },
                exchange: LocalMcpOauthExchangeResult::Failed {
                    reason: reason.clone(),
                },
                open: LocalMcpTransportOpenResult::Failed {
                    reason: reason.clone(),
                },
                state_path: root.join(MCP_OAUTH_STATE_REL),
                receipt_path: root.join(MCP_OAUTH_RECEIPT_REL),
            });
        }
    };
    let exchange = oauth.exchange(code)?;
    let open = oauth.open_transport("file://.agent-harness/mcp-oauth/transport")?;
    Ok(LocalMcpOauthProduct {
        begin,
        exchange,
        open,
        state_path: root.join(MCP_OAUTH_STATE_REL),
        receipt_path: root.join(MCP_OAUTH_RECEIPT_REL),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_mcp_oauth_product_writes_state_and_receipt() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let product = run_local_mcp_oauth_product(root).expect("product");
        assert!(product.meets_contract(), "{product:?}");
        let reopened = LocalMcpOauth::open(root).expect("reopen");
        assert!(reopened.state().begun);
        assert!(reopened.state().exchanged);
        assert!(reopened.state().opened);
        let receipt = fs::read_to_string(root.join(MCP_OAUTH_RECEIPT_REL)).expect("receipt");
        assert!(receipt.contains("\"status\": \"opened\""));
        assert!(!receipt.contains("mcp-tok-"));
    }

    #[test]
    fn oauth_state_survives_reopen_after_exchange() {
        // arrange
        let dir = tempdir().expect("tempdir");
        let mut oauth = LocalMcpOauth::open(dir.path()).expect("open");
        let begun = oauth
            .begin("srv-a", "https://auth.example/authorize")
            .expect("begin");
        let authorization_code = match begun {
            LocalMcpOauthBeginResult::Begun {
                authorization_code, ..
            } => authorization_code,
            other => panic!("expected Begun, got {other:?}"),
        };

        // act — exchange, drop, and reopen a fresh instance over the same workspace
        oauth.exchange(authorization_code).expect("exchange");
        drop(oauth);
        let reopened = LocalMcpOauth::open(dir.path()).expect("reopen");

        // assert — durable flow state persisted; token hint stays redacted
        let state = reopened.state();
        assert!(state.begun);
        assert!(state.exchanged);
        assert_eq!(state.server_id.as_deref(), Some("srv-a"));
        let hint = state.access_token_hint.as_deref().expect("hint persisted");
        assert!(!hint.contains("mcp-tok"));
    }

    #[test]
    fn exchange_without_begin_fails_closed() {
        let dir = tempdir().expect("tempdir");
        let mut oauth = LocalMcpOauth::open(dir.path()).expect("open");
        let err = oauth.exchange("nope").expect_err("must fail");
        assert!(matches!(err, LocalMcpOauthError::NotBegun));
    }

    #[test]
    fn open_without_exchange_fails_closed() {
        let dir = tempdir().expect("tempdir");
        let mut oauth = LocalMcpOauth::open(dir.path()).expect("open");
        oauth.begin("s", "https://mcp.local/auth").expect("begin");
        let err = oauth.open_transport("file://x").expect_err("must fail");
        assert!(matches!(err, LocalMcpOauthError::NotExchanged));
    }
}
