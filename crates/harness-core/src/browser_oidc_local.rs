//! Local durable browser OIDC product (file-backed; no live IdP).
//!
//! Implements start → complete flow with durable state and redacted receipts
//! under `.agent-harness/browser-oidc/` so operator surfaces exercise a real
//! OIDC lifecycle without network SSO.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Relative state path under a workspace.
pub const BROWSER_OIDC_STATE_REL: &str = ".agent-harness/browser-oidc/state.json";
/// Relative receipt path written after complete.
pub const BROWSER_OIDC_RECEIPT_REL: &str = ".agent-harness/browser-oidc/complete.receipt.json";

/// Durable browser OIDC session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalBrowserOidcState {
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub authorization_code: Option<String>,
    pub subject: Option<String>,
    pub started: bool,
    pub completed: bool,
    pub seq: u64,
}

/// Start outcome for local OIDC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LocalOidcStartResult {
    Started {
        issuer: String,
        client_id: String,
        authorization_code: String,
    },
    Failed {
        reason: String,
    },
}

impl LocalOidcStartResult {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Started { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Started {
                issuer, client_id, ..
            } => format!("local browser OIDC start: issuer={issuer} client={client_id}"),
            Self::Failed { reason } => format!("local browser OIDC start: failed ({reason})"),
        }
    }
}

/// Complete outcome for local OIDC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LocalOidcCompleteResult {
    Completed {
        subject: String,
        issuer: String,
        /// Redacted code hint (never full secret).
        code_hint: String,
    },
    Failed {
        reason: String,
    },
}

impl LocalOidcCompleteResult {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Completed {
                subject,
                issuer,
                code_hint,
            } => format!(
                "local browser OIDC complete: subject={subject} issuer={issuer} code={code_hint}"
            ),
            Self::Failed { reason } => format!("local browser OIDC complete: failed ({reason})"),
        }
    }
}

/// Errors from local OIDC product paths.
#[derive(Debug)]
pub enum LocalBrowserOidcError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    NotStarted,
    CodeMismatch,
    EmptyInput {
        field: &'static str,
    },
}

impl std::fmt::Display for LocalBrowserOidcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "io error at {}: {source}", path.display()),
            Self::Serialize { path, source } => {
                write!(f, "serialize error at {}: {source}", path.display())
            }
            Self::NotStarted => write!(f, "OIDC flow not started"),
            Self::CodeMismatch => write!(f, "authorization code mismatch"),
            Self::EmptyInput { field } => write!(f, "empty input for {field}"),
        }
    }
}

impl std::error::Error for LocalBrowserOidcError {}

fn redact_code(code: &str) -> String {
    if code.len() <= 4 {
        return "****".to_string();
    }
    format!("{}…", &code[..4])
}

/// File-backed browser OIDC session.
#[derive(Debug, Clone)]
pub struct LocalBrowserOidc {
    root: PathBuf,
    state: LocalBrowserOidcState,
}

impl LocalBrowserOidc {
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, LocalBrowserOidcError> {
        let root = workspace_root.as_ref().to_path_buf();
        let state_path = root.join(BROWSER_OIDC_STATE_REL);
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent).map_err(|source| LocalBrowserOidcError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let state = if state_path.is_file() {
            let body =
                fs::read_to_string(&state_path).map_err(|source| LocalBrowserOidcError::Io {
                    path: state_path.clone(),
                    source,
                })?;
            serde_json::from_str(&body).map_err(|source| LocalBrowserOidcError::Serialize {
                path: state_path,
                source,
            })?
        } else {
            LocalBrowserOidcState::default()
        };
        Ok(Self { root, state })
    }

    pub fn state(&self) -> &LocalBrowserOidcState {
        &self.state
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(BROWSER_OIDC_STATE_REL)
    }

    fn receipt_path(&self) -> PathBuf {
        self.root.join(BROWSER_OIDC_RECEIPT_REL)
    }

    fn persist(&self) -> Result<(), LocalBrowserOidcError> {
        let path = self.state_path();
        let body = serde_json::to_string_pretty(&self.state).map_err(|source| {
            LocalBrowserOidcError::Serialize {
                path: path.clone(),
                source,
            }
        })?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, format!("{body}\n")).map_err(|source| LocalBrowserOidcError::Io {
            path: temp.clone(),
            source,
        })?;
        fs::rename(&temp, &path).map_err(|source| LocalBrowserOidcError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(())
    }

    /// Start a local OIDC authorization flow (issues a local auth code).
    pub fn start(
        &mut self,
        issuer: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Result<LocalOidcStartResult, LocalBrowserOidcError> {
        let issuer = issuer.into();
        let client_id = client_id.into();
        if issuer.trim().is_empty() {
            return Err(LocalBrowserOidcError::EmptyInput { field: "issuer" });
        }
        if client_id.trim().is_empty() {
            return Err(LocalBrowserOidcError::EmptyInput { field: "client_id" });
        }
        let authorization_code = format!("loc-code-{}", self.state.seq.saturating_add(1));
        self.state.issuer = Some(issuer.clone());
        self.state.client_id = Some(client_id.clone());
        self.state.authorization_code = Some(authorization_code.clone());
        self.state.started = true;
        self.state.completed = false;
        self.state.subject = None;
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;
        Ok(LocalOidcStartResult::Started {
            issuer,
            client_id,
            authorization_code,
        })
    }

    /// Complete the flow with the issued authorization code.
    pub fn complete(
        &mut self,
        authorization_code: impl Into<String>,
    ) -> Result<LocalOidcCompleteResult, LocalBrowserOidcError> {
        if !self.state.started {
            return Err(LocalBrowserOidcError::NotStarted);
        }
        let code = authorization_code.into();
        let expected = self.state.authorization_code.as_deref().unwrap_or_default();
        if code != expected {
            return Err(LocalBrowserOidcError::CodeMismatch);
        }
        let issuer = self
            .state
            .issuer
            .clone()
            .unwrap_or_else(|| "local-issuer".to_string());
        let subject = format!("sub-{}", self.state.seq.saturating_add(1));
        self.state.subject = Some(subject.clone());
        self.state.completed = true;
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;

        let code_hint = redact_code(&code);
        let result = LocalOidcCompleteResult::Completed {
            subject: subject.clone(),
            issuer: issuer.clone(),
            code_hint: code_hint.clone(),
        };
        let receipt = serde_json::json!({
            "status": "completed",
            "subject": subject,
            "issuer": issuer,
            "codeHint": code_hint,
        });
        let receipt_path = self.receipt_path();
        let body = serde_json::to_string_pretty(&receipt).map_err(|source| {
            LocalBrowserOidcError::Serialize {
                path: receipt_path.clone(),
                source,
            }
        })?;
        fs::write(&receipt_path, format!("{body}\n")).map_err(|source| {
            LocalBrowserOidcError::Io {
                path: receipt_path,
                source,
            }
        })?;
        Ok(result)
    }
}

/// Full product walk: start → complete with durable state + receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBrowserOidcProduct {
    pub start: LocalOidcStartResult,
    pub complete: LocalOidcCompleteResult,
    pub state_path: PathBuf,
    pub receipt_path: PathBuf,
}

impl LocalBrowserOidcProduct {
    pub fn meets_contract(&self) -> bool {
        self.start.is_success()
            && self.complete.is_success()
            && self.state_path.is_file()
            && self.receipt_path.is_file()
    }
}

/// Run local browser OIDC product under `workspace_root`.
pub fn run_local_browser_oidc_product(
    workspace_root: impl AsRef<Path>,
) -> Result<LocalBrowserOidcProduct, LocalBrowserOidcError> {
    let root = workspace_root.as_ref();
    let mut oidc = LocalBrowserOidc::open(root)?;
    let start = oidc.start("https://issuer.local/oidc", "harness-local-client")?;
    let code = match &start {
        LocalOidcStartResult::Started {
            authorization_code, ..
        } => authorization_code.clone(),
        LocalOidcStartResult::Failed { reason } => {
            return Ok(LocalBrowserOidcProduct {
                start: LocalOidcStartResult::Failed {
                    reason: reason.clone(),
                },
                complete: LocalOidcCompleteResult::Failed {
                    reason: reason.clone(),
                },
                state_path: root.join(BROWSER_OIDC_STATE_REL),
                receipt_path: root.join(BROWSER_OIDC_RECEIPT_REL),
            });
        }
    };
    let complete = oidc.complete(code)?;
    Ok(LocalBrowserOidcProduct {
        start,
        complete,
        state_path: root.join(BROWSER_OIDC_STATE_REL),
        receipt_path: root.join(BROWSER_OIDC_RECEIPT_REL),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_oidc_product_writes_state_and_receipt() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let product = run_local_browser_oidc_product(root).expect("product");
        assert!(product.meets_contract(), "{product:?}");
        let reopened = LocalBrowserOidc::open(root).expect("reopen");
        assert!(reopened.state().started);
        assert!(reopened.state().completed);
        assert!(reopened.state().subject.is_some());
        let receipt = fs::read_to_string(root.join(BROWSER_OIDC_RECEIPT_REL)).expect("receipt");
        assert!(receipt.contains("\"status\": \"completed\""));
        assert!(!receipt.contains("loc-code-"));
    }

    #[test]
    fn complete_without_start_fails_closed() {
        let dir = tempdir().expect("tempdir");
        let mut oidc = LocalBrowserOidc::open(dir.path()).expect("open");
        let err = oidc.complete("nope").expect_err("must fail");
        assert!(matches!(err, LocalBrowserOidcError::NotStarted));
    }

    #[test]
    fn wrong_code_fails_closed() {
        let dir = tempdir().expect("tempdir");
        let mut oidc = LocalBrowserOidc::open(dir.path()).expect("open");
        oidc.start("https://issuer.local", "client").expect("start");
        let err = oidc.complete("wrong-code").expect_err("must fail");
        assert!(matches!(err, LocalBrowserOidcError::CodeMismatch));
    }
}
