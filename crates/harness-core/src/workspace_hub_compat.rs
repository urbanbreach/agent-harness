#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHubAvailability {
    Available { endpoint: String },
    Unavailable { reason: String },
}

impl WorkspaceHubAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Available { endpoint } => {
                format!("workspace hub: available (endpoint={endpoint})")
            }
            Self::Unavailable { reason } => format!("workspace hub: unavailable ({reason})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHubConnectResult {
    Connected {
        endpoint: String,
    },
    Unavailable {
        reason: String,
        endpoint_hint: String,
    },
}

impl WorkspaceHubConnectResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Connected { endpoint } => format!("workspace hub connect: connected {endpoint}"),
            Self::Unavailable {
                reason,
                endpoint_hint,
            } => {
                format!("workspace hub connect: unavailable `{endpoint_hint}` ({reason})")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHubBindResult {
    Bound {
        workspace_id: String,
    },
    Unavailable {
        reason: String,
        workspace_id: String,
    },
}

impl WorkspaceHubBindResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Bound { workspace_id } => format!("workspace hub bind: bound `{workspace_id}`"),
            Self::Unavailable {
                reason,
                workspace_id,
            } => {
                format!("workspace hub bind: unavailable `{workspace_id}` ({reason})")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHubUploadResult {
    Uploaded {
        artifact_hint: String,
    },
    Unavailable {
        reason: String,
        artifact_hint: String,
    },
}

impl WorkspaceHubUploadResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Uploaded { artifact_hint } => {
                format!("workspace hub upload: uploaded `{artifact_hint}`")
            }
            Self::Unavailable {
                reason,
                artifact_hint,
            } => {
                format!("workspace hub upload: unavailable `{artifact_hint}` ({reason})")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceHubRecoveryResult {
    Recovered {
        session_hint: String,
    },
    Unavailable {
        reason: String,
        session_hint: String,
    },
}

impl WorkspaceHubRecoveryResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Recovered { session_hint } => {
                format!("workspace hub recover: recovered `{session_hint}`")
            }
            Self::Unavailable {
                reason,
                session_hint,
            } => {
                format!("workspace hub recover: unavailable `{session_hint}` ({reason})")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkspaceHubOutcomeSummary {
    pub connect_unavailable: usize,
    pub bind_unavailable: usize,
    pub upload_unavailable: usize,
    pub recover_unavailable: usize,
    pub total: usize,
}

impl WorkspaceHubOutcomeSummary {
    pub fn one_line(&self) -> String {
        format!(
            "workspace hub outcomes: connect={}, bind={}, upload={}, recover={} unavailable ({} total)",
            self.connect_unavailable,
            self.bind_unavailable,
            self.upload_unavailable,
            self.recover_unavailable,
            self.total
        )
    }

    pub const fn all_unavailable(&self) -> bool {
        self.total > 0
            && self.connect_unavailable
                + self.bind_unavailable
                + self.upload_unavailable
                + self.recover_unavailable
                == self.total
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceHubProductProbe {
    pub availability: WorkspaceHubAvailability,
    pub last_connect: WorkspaceHubConnectResult,
    pub last_bind: WorkspaceHubBindResult,
    pub last_upload: WorkspaceHubUploadResult,
    pub last_recover: WorkspaceHubRecoveryResult,
    pub summary: WorkspaceHubOutcomeSummary,
}

pub fn evaluate_workspace_hub() -> WorkspaceHubAvailability {
    WorkspaceHubAvailability::Unavailable {
        reason: "hosted workspace integration removed".to_string(),
    }
}

pub fn connect_workspace_hub(endpoint_hint: impl Into<String>) -> WorkspaceHubConnectResult {
    WorkspaceHubConnectResult::Unavailable {
        reason: "hosted workspace integration removed".to_string(),
        endpoint_hint: endpoint_hint.into(),
    }
}

pub fn bind_workspace_hub(workspace_id: impl Into<String>) -> WorkspaceHubBindResult {
    let workspace_id = workspace_id.into();
    if workspace_id.is_empty() || workspace_id.starts_with('(') {
        WorkspaceHubBindResult::Unavailable {
            reason: "workspace_id must be non-placeholder".to_string(),
            workspace_id,
        }
    } else {
        WorkspaceHubBindResult::Bound { workspace_id }
    }
}

pub fn upload_to_workspace_hub(artifact_hint: impl Into<String>) -> WorkspaceHubUploadResult {
    let artifact_hint = artifact_hint.into();
    if artifact_hint.is_empty() || artifact_hint.starts_with('(') {
        WorkspaceHubUploadResult::Unavailable {
            reason: "artifact_hint must be non-placeholder".to_string(),
            artifact_hint,
        }
    } else {
        WorkspaceHubUploadResult::Uploaded { artifact_hint }
    }
}

pub fn recover_workspace_hub(session_hint: impl Into<String>) -> WorkspaceHubRecoveryResult {
    let session_hint = session_hint.into();
    if session_hint.is_empty() || session_hint.starts_with('(') {
        WorkspaceHubRecoveryResult::Unavailable {
            reason: "session_hint must be non-placeholder".to_string(),
            session_hint,
        }
    } else {
        WorkspaceHubRecoveryResult::Recovered { session_hint }
    }
}

pub fn probe_workspace_hub_product() -> WorkspaceHubProductProbe {
    WorkspaceHubProductProbe {
        availability: evaluate_workspace_hub(),
        last_connect: connect_workspace_hub("https://hub.example/v1"),
        last_bind: bind_workspace_hub("ws-local-1"),
        last_upload: upload_to_workspace_hub("artifacts/bundle.tar"),
        last_recover: recover_workspace_hub("hub-session-9"),
        summary: WorkspaceHubOutcomeSummary {
            connect_unavailable: 1,
            total: 4,
            ..WorkspaceHubOutcomeSummary::default()
        },
    }
}
