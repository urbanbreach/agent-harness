//! Local durable workspace hub product (file-backed; no remote peer).
//!
//! Provides real connect/bind/upload/recover side effects under
//! `.agent-harness/workspace-hub/` so operator surfaces can exercise a
//! complete hub lifecycle without a live network service.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Relative hub root under a workspace.
pub const WORKSPACE_HUB_ROOT_REL: &str = ".agent-harness/workspace-hub";
/// Relative path for the durable hub state journal.
pub const WORKSPACE_HUB_STATE_REL: &str = ".agent-harness/workspace-hub/state.json";

/// Durable hub state persisted under the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalWorkspaceHubState {
    pub endpoint: Option<String>,
    pub connected: bool,
    pub bound_workspace_id: Option<String>,
    pub uploads: Vec<LocalHubUploadRecord>,
    pub recoveries: Vec<LocalHubRecoveryRecord>,
    pub seq: u64,
}

/// One uploaded artifact record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHubUploadRecord {
    pub artifact_id: String,
    pub source_hint: String,
    pub stored_rel: String,
}

/// One recovery record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHubRecoveryRecord {
    pub session_hint: String,
    pub recovered_workspace_id: String,
}

/// Local hub operation outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LocalHubOpResult {
    Connected {
        endpoint: String,
    },
    Bound {
        workspace_id: String,
    },
    Uploaded {
        artifact_id: String,
        stored_rel: String,
    },
    Recovered {
        session_hint: String,
        workspace_id: String,
    },
    Failed {
        reason: String,
    },
}

impl LocalHubOpResult {
    pub const fn is_success(&self) -> bool {
        !matches!(self, Self::Failed { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Connected { endpoint } => {
                format!("local workspace hub: connected endpoint={endpoint}")
            }
            Self::Bound { workspace_id } => {
                format!("local workspace hub: bound workspace={workspace_id}")
            }
            Self::Uploaded {
                artifact_id,
                stored_rel,
            } => format!("local workspace hub: uploaded artifact={artifact_id} path={stored_rel}"),
            Self::Recovered {
                session_hint,
                workspace_id,
            } => format!(
                "local workspace hub: recovered session={session_hint} workspace={workspace_id}"
            ),
            Self::Failed { reason } => format!("local workspace hub: failed ({reason})"),
        }
    }
}

/// Errors from local hub product paths.
#[derive(Debug)]
pub enum LocalWorkspaceHubError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    NotConnected,
    EmptyInput {
        field: &'static str,
    },
}

impl std::fmt::Display for LocalWorkspaceHubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "io error at {}: {source}", path.display()),
            Self::Serialize { path, source } => {
                write!(f, "serialize error at {}: {source}", path.display())
            }
            Self::NotConnected => write!(f, "hub is not connected"),
            Self::EmptyInput { field } => write!(f, "empty input for {field}"),
        }
    }
}

impl std::error::Error for LocalWorkspaceHubError {}

/// File-backed local workspace hub.
#[derive(Debug, Clone)]
pub struct LocalWorkspaceHub {
    root: PathBuf,
    state: LocalWorkspaceHubState,
}

impl LocalWorkspaceHub {
    /// Open or create a hub under `workspace_root`.
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, LocalWorkspaceHubError> {
        let root = workspace_root.as_ref().join(WORKSPACE_HUB_ROOT_REL);
        fs::create_dir_all(&root).map_err(|source| LocalWorkspaceHubError::Io {
            path: root.clone(),
            source,
        })?;
        let state_path = workspace_root.as_ref().join(WORKSPACE_HUB_STATE_REL);
        let state = if state_path.is_file() {
            let body =
                fs::read_to_string(&state_path).map_err(|source| LocalWorkspaceHubError::Io {
                    path: state_path.clone(),
                    source,
                })?;
            serde_json::from_str(&body).map_err(|source| LocalWorkspaceHubError::Serialize {
                path: state_path,
                source,
            })?
        } else {
            LocalWorkspaceHubState::default()
        };
        Ok(Self {
            root: workspace_root.as_ref().to_path_buf(),
            state,
        })
    }

    pub fn state(&self) -> &LocalWorkspaceHubState {
        &self.state
    }

    pub fn is_connected(&self) -> bool {
        self.state.connected
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(WORKSPACE_HUB_STATE_REL)
    }

    fn persist(&self) -> Result<(), LocalWorkspaceHubError> {
        let path = self.state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| LocalWorkspaceHubError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(&self.state).map_err(|source| {
            LocalWorkspaceHubError::Serialize {
                path: path.clone(),
                source,
            }
        })?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, format!("{body}\n")).map_err(|source| LocalWorkspaceHubError::Io {
            path: temp.clone(),
            source,
        })?;
        fs::rename(&temp, &path).map_err(|source| LocalWorkspaceHubError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(())
    }

    /// Connect to a local hub endpoint (file-backed identity; no network).
    pub fn connect(
        &mut self,
        endpoint: impl Into<String>,
    ) -> Result<LocalHubOpResult, LocalWorkspaceHubError> {
        let endpoint = endpoint.into();
        if endpoint.trim().is_empty() {
            return Err(LocalWorkspaceHubError::EmptyInput { field: "endpoint" });
        }
        self.state.endpoint = Some(endpoint.clone());
        self.state.connected = true;
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;
        Ok(LocalHubOpResult::Connected { endpoint })
    }

    /// Bind a workspace id while connected.
    pub fn bind(
        &mut self,
        workspace_id: impl Into<String>,
    ) -> Result<LocalHubOpResult, LocalWorkspaceHubError> {
        if !self.state.connected {
            return Err(LocalWorkspaceHubError::NotConnected);
        }
        let workspace_id = workspace_id.into();
        if workspace_id.trim().is_empty() {
            return Err(LocalWorkspaceHubError::EmptyInput {
                field: "workspace_id",
            });
        }
        self.state.bound_workspace_id = Some(workspace_id.clone());
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;
        Ok(LocalHubOpResult::Bound { workspace_id })
    }

    /// Upload artifact bytes into the hub store.
    pub fn upload(
        &mut self,
        artifact_hint: impl Into<String>,
        content: &[u8],
    ) -> Result<LocalHubOpResult, LocalWorkspaceHubError> {
        if !self.state.connected {
            return Err(LocalWorkspaceHubError::NotConnected);
        }
        let artifact_hint = artifact_hint.into();
        if artifact_hint.trim().is_empty() {
            return Err(LocalWorkspaceHubError::EmptyInput {
                field: "artifact_hint",
            });
        }
        let artifact_id = format!("artifact-{}", self.state.seq.saturating_add(1));
        let stored_rel = format!("uploads/{artifact_id}.bin");
        let store_path = self.root.join(WORKSPACE_HUB_ROOT_REL).join(&stored_rel);
        if let Some(parent) = store_path.parent() {
            fs::create_dir_all(parent).map_err(|source| LocalWorkspaceHubError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&store_path, content).map_err(|source| LocalWorkspaceHubError::Io {
            path: store_path.clone(),
            source,
        })?;
        self.state.uploads.push(LocalHubUploadRecord {
            artifact_id: artifact_id.clone(),
            source_hint: artifact_hint,
            stored_rel: stored_rel.clone(),
        });
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;
        Ok(LocalHubOpResult::Uploaded {
            artifact_id,
            stored_rel,
        })
    }

    /// Recover a prior session into a bound workspace id.
    pub fn recover(
        &mut self,
        session_hint: impl Into<String>,
    ) -> Result<LocalHubOpResult, LocalWorkspaceHubError> {
        if !self.state.connected {
            return Err(LocalWorkspaceHubError::NotConnected);
        }
        let session_hint = session_hint.into();
        if session_hint.trim().is_empty() {
            return Err(LocalWorkspaceHubError::EmptyInput {
                field: "session_hint",
            });
        }
        let workspace_id = self
            .state
            .bound_workspace_id
            .clone()
            .unwrap_or_else(|| format!("recovered-{session_hint}"));
        self.state.recoveries.push(LocalHubRecoveryRecord {
            session_hint: session_hint.clone(),
            recovered_workspace_id: workspace_id.clone(),
        });
        self.state.bound_workspace_id = Some(workspace_id.clone());
        self.state.seq = self.state.seq.saturating_add(1);
        self.persist()?;
        Ok(LocalHubOpResult::Recovered {
            session_hint,
            workspace_id,
        })
    }
}

/// Full product walk: connect → bind → upload → recover with durable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalWorkspaceHubProduct {
    pub connect: LocalHubOpResult,
    pub bind: LocalHubOpResult,
    pub upload: LocalHubOpResult,
    pub recover: LocalHubOpResult,
    pub state_path: PathBuf,
}

impl LocalWorkspaceHubProduct {
    pub fn meets_contract(&self) -> bool {
        self.connect.is_success()
            && self.bind.is_success()
            && self.upload.is_success()
            && self.recover.is_success()
            && self.state_path.is_file()
    }
}

/// Run local hub product under `workspace_root` with real filesystem side effects.
pub fn run_local_workspace_hub_product(
    workspace_root: impl AsRef<Path>,
) -> Result<LocalWorkspaceHubProduct, LocalWorkspaceHubError> {
    let root = workspace_root.as_ref();
    let mut hub = LocalWorkspaceHub::open(root)?;
    let connect = hub.connect("local://workspace-hub")?;
    let bind = hub.bind("ws-primary")?;
    let upload = hub.upload("probe-artifact", b"hub-upload-payload")?;
    let recover = hub.recover("probe-session")?;
    Ok(LocalWorkspaceHubProduct {
        connect,
        bind,
        upload,
        recover,
        state_path: root.join(WORKSPACE_HUB_STATE_REL),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_hub_product_writes_state_and_upload() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let product = run_local_workspace_hub_product(root).expect("product");
        assert!(product.meets_contract(), "{product:?}");
        assert!(root.join(WORKSPACE_HUB_STATE_REL).is_file());
        let reopened = LocalWorkspaceHub::open(root).expect("reopen");
        assert!(reopened.is_connected());
        assert_eq!(
            reopened.state().bound_workspace_id.as_deref(),
            Some("ws-primary")
        );
        assert_eq!(reopened.state().uploads.len(), 1);
        assert_eq!(reopened.state().recoveries.len(), 1);
        let stored = &reopened.state().uploads[0].stored_rel;
        let bytes = fs::read(root.join(WORKSPACE_HUB_ROOT_REL).join(stored)).expect("upload bytes");
        assert_eq!(bytes, b"hub-upload-payload");
    }

    #[test]
    fn bind_without_connect_fails_closed() {
        let dir = tempdir().expect("tempdir");
        let mut hub = LocalWorkspaceHub::open(dir.path()).expect("open");
        let err = hub.bind("ws").expect_err("must fail");
        assert!(matches!(err, LocalWorkspaceHubError::NotConnected));
    }
}
