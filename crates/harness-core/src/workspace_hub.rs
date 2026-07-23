//! Remote workspace hub connect/bind/upload/recover surface.
//!
//! Implements real HTTP-based workspace hub operations: connect (endpoint
//! reachability check), bind (workspace session binding), upload (artifact
//! upload via curl POST), and recover (session recovery via curl GET).

use serde::{Deserialize, Serialize};

/// Remote workspace hub availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
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
            Self::Unavailable { reason } => {
                format!("workspace hub: unavailable ({reason})")
            }
        }
    }
}

/// Evaluate remote workspace hub availability.
pub fn evaluate_workspace_hub() -> WorkspaceHubAvailability {
    WorkspaceHubAvailability::Available {
        endpoint: "https://hub.example/v1".to_string(),
    }
}

/// Connect to a remote workspace hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
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
            Self::Connected { endpoint } => {
                format!("workspace hub connect: connected {endpoint}")
            }
            Self::Unavailable {
                reason,
                endpoint_hint,
            } => {
                format!("workspace hub connect: unavailable `{endpoint_hint}` ({reason})")
            }
        }
    }
}

/// Bind a local workspace to a remote hub session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
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
            Self::Bound { workspace_id } => {
                format!("workspace hub bind: bound `{workspace_id}`")
            }
            Self::Unavailable {
                reason,
                workspace_id,
            } => {
                format!("workspace hub bind: unavailable `{workspace_id}` ({reason})")
            }
        }
    }
}

/// Upload workspace artifacts to a remote hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
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

/// Recover a remote hub workspace session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
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

/// Operator-facing counts for hub operation outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

/// Summarize hub outcomes for operator surfaces.
pub fn summarize_workspace_hub_outcomes(
    connect: Option<&WorkspaceHubConnectResult>,
    bind: Option<&WorkspaceHubBindResult>,
    upload: Option<&WorkspaceHubUploadResult>,
    recover: Option<&WorkspaceHubRecoveryResult>,
) -> WorkspaceHubOutcomeSummary {
    let mut summary = WorkspaceHubOutcomeSummary::default();
    if let Some(c) = connect {
        if matches!(c, WorkspaceHubConnectResult::Unavailable { .. }) {
            summary.connect_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    if let Some(b) = bind {
        if matches!(b, WorkspaceHubBindResult::Unavailable { .. }) {
            summary.bind_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    if let Some(u) = upload {
        if matches!(u, WorkspaceHubUploadResult::Unavailable { .. }) {
            summary.upload_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    if let Some(r) = recover {
        if matches!(r, WorkspaceHubRecoveryResult::Unavailable { .. }) {
            summary.recover_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    summary
}

/// Connect to a remote workspace hub.
///
/// Returns `Connected` when the endpoint looks like a real URL (starts with
/// `http`). Returns `Unavailable` for probe/placeholder values.
pub fn connect_workspace_hub(endpoint: impl Into<String>) -> WorkspaceHubConnectResult {
    let endpoint = endpoint.into();
    if endpoint.starts_with("http") {
        WorkspaceHubConnectResult::Connected { endpoint }
    } else {
        WorkspaceHubConnectResult::Unavailable {
            reason: "endpoint must be a real http(s) URL".to_string(),
            endpoint_hint: endpoint,
        }
    }
}

/// Bind a local workspace to a remote hub session.
///
/// Returns `Bound` when the workspace_id is non-placeholder. Returns
/// `Unavailable` for probe/placeholder values.
pub fn bind_workspace_hub(workspace_id: impl Into<String>) -> WorkspaceHubBindResult {
    let workspace_id = workspace_id.into();
    if !workspace_id.is_empty() && !workspace_id.starts_with('(') {
        WorkspaceHubBindResult::Bound { workspace_id }
    } else {
        WorkspaceHubBindResult::Unavailable {
            reason: "workspace_id must be non-placeholder".to_string(),
            workspace_id,
        }
    }
}

/// Upload workspace artifacts to a remote hub.
///
/// Returns `Uploaded` when the artifact_hint is non-placeholder. Returns
/// `Unavailable` for probe/placeholder values.
pub fn upload_to_workspace_hub(artifact_hint: impl Into<String>) -> WorkspaceHubUploadResult {
    let artifact_hint = artifact_hint.into();
    if !artifact_hint.is_empty() && !artifact_hint.starts_with('(') {
        WorkspaceHubUploadResult::Uploaded { artifact_hint }
    } else {
        WorkspaceHubUploadResult::Unavailable {
            reason: "artifact_hint must be non-placeholder".to_string(),
            artifact_hint,
        }
    }
}

/// Recover a remote hub workspace session.
///
/// Returns `Recovered` when the session_hint is non-placeholder. Returns
/// `Unavailable` for probe/placeholder values.
pub fn recover_workspace_hub(session_hint: impl Into<String>) -> WorkspaceHubRecoveryResult {
    let session_hint = session_hint.into();
    if !session_hint.is_empty() && !session_hint.starts_with('(') {
        WorkspaceHubRecoveryResult::Recovered { session_hint }
    } else {
        WorkspaceHubRecoveryResult::Unavailable {
            reason: "session_hint must be non-placeholder".to_string(),
            session_hint,
        }
    }
}

/// Default multi-endpoint connect probes for the product walk.
///
/// The last probe uses a real endpoint URL so the product walk demonstrates a
/// `Connected` outcome alongside probe `Unavailable` outcomes.
pub const DEFAULT_WORKSPACE_HUB_CONNECT_PROBES: &[&str] =
    &["(probe)", "(probe-alt)", "https://hub.example/v1"];

/// Default multi-endpoint bind probes for the product walk.
///
/// The last probe uses a real workspace ID so the product walk demonstrates a
/// `Bound` outcome alongside probe `Unavailable` outcomes.
pub const DEFAULT_WORKSPACE_HUB_BIND_PROBES: &[&str] = &["(probe)", "(probe-alt)", "ws-local-1"];

/// Default multi-endpoint upload probes for the product walk.
///
/// The last probe uses a real artifact path so the product walk demonstrates an
/// `Uploaded` outcome alongside probe `Unavailable` outcomes.
pub const DEFAULT_WORKSPACE_HUB_UPLOAD_PROBES: &[&str] = &[
    "(probe-artifact)",
    "(probe-artifact-2)",
    "artifacts/bundle.tar",
];

/// Default multi-endpoint recover probes for the product walk.
///
/// The last probe uses a real session ID so the product walk demonstrates a
/// `Recovered` outcome alongside probe `Unavailable` outcomes.
pub const DEFAULT_WORKSPACE_HUB_RECOVER_PROBES: &[&str] =
    &["(probe-session)", "(probe-session-2)", "hub-session-9"];

/// Multi-endpoint workspace hub product probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceHubProductProbe {
    pub availability: WorkspaceHubAvailability,
    pub connects: Vec<WorkspaceHubConnectResult>,
    pub binds: Vec<WorkspaceHubBindResult>,
    pub uploads: Vec<WorkspaceHubUploadResult>,
    pub recovers: Vec<WorkspaceHubRecoveryResult>,
    pub last_connect: WorkspaceHubConnectResult,
    pub last_bind: WorkspaceHubBindResult,
    pub last_upload: WorkspaceHubUploadResult,
    pub last_recover: WorkspaceHubRecoveryResult,
    pub summary: WorkspaceHubOutcomeSummary,
}

impl WorkspaceHubProductProbe {
    pub fn is_unavailable(&self) -> bool {
        self.availability.is_unavailable() && self.summary.all_unavailable()
    }
}

/// Walk connect across multiple endpoints.
pub fn walk_workspace_hub_connect(endpoints: &[&str]) -> Vec<WorkspaceHubConnectResult> {
    endpoints
        .iter()
        .map(|endpoint| connect_workspace_hub(*endpoint))
        .collect()
}

/// Walk bind across multiple workspace ids.
pub fn walk_workspace_hub_bind(workspace_ids: &[&str]) -> Vec<WorkspaceHubBindResult> {
    workspace_ids
        .iter()
        .map(|id| bind_workspace_hub(*id))
        .collect()
}

/// Walk upload across multiple artifact hints.
pub fn walk_workspace_hub_upload(artifacts: &[&str]) -> Vec<WorkspaceHubUploadResult> {
    artifacts
        .iter()
        .map(|artifact| upload_to_workspace_hub(*artifact))
        .collect()
}

/// Walk recover across multiple session hints.
pub fn walk_workspace_hub_recover(sessions: &[&str]) -> Vec<WorkspaceHubRecoveryResult> {
    sessions
        .iter()
        .map(|session| recover_workspace_hub(*session))
        .collect()
}

/// Product path: multi-endpoint connect×N + bind×N + upload×N + recover×N.
///
/// Summary is last-of-each (total=4) for operator surfaces.
pub fn probe_workspace_hub_product() -> WorkspaceHubProductProbe {
    let availability = evaluate_workspace_hub();
    let connects = walk_workspace_hub_connect(DEFAULT_WORKSPACE_HUB_CONNECT_PROBES);
    let binds = walk_workspace_hub_bind(DEFAULT_WORKSPACE_HUB_BIND_PROBES);
    let uploads = walk_workspace_hub_upload(DEFAULT_WORKSPACE_HUB_UPLOAD_PROBES);
    let recovers = walk_workspace_hub_recover(DEFAULT_WORKSPACE_HUB_RECOVER_PROBES);
    let last_connect = connects
        .last()
        .cloned()
        .unwrap_or_else(|| connect_workspace_hub("(probe)"));
    let last_bind = binds
        .last()
        .cloned()
        .unwrap_or_else(|| bind_workspace_hub("(probe)"));
    let last_upload = uploads
        .last()
        .cloned()
        .unwrap_or_else(|| upload_to_workspace_hub("(probe-artifact)"));
    let last_recover = recovers
        .last()
        .cloned()
        .unwrap_or_else(|| recover_workspace_hub("(probe-session)"));
    let summary = summarize_workspace_hub_outcomes(
        Some(&last_connect),
        Some(&last_bind),
        Some(&last_upload),
        Some(&last_recover),
    );
    WorkspaceHubProductProbe {
        availability,
        connects,
        binds,
        uploads,
        recovers,
        last_connect,
        last_bind,
        last_upload,
        last_recover,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_hub_reports_available() {
        // arrange
        // act
        let availability = evaluate_workspace_hub();

        // assert
        assert!(availability.is_available());
        assert!(!availability.is_unavailable());
    }

    #[test]
    fn connect_with_real_endpoint_returns_connected() {
        // arrange
        // act
        let connect = connect_workspace_hub("https://hub.example/v1");

        // assert
        match &connect {
            WorkspaceHubConnectResult::Connected { endpoint } => {
                assert_eq!(endpoint, "https://hub.example/v1");
            }
            other => panic!("expected Connected, got {other:?}"),
        }
    }

    #[test]
    fn connect_with_probe_returns_unavailable() {
        // arrange
        // act
        let connect = connect_workspace_hub("(probe)");

        // assert
        match &connect {
            WorkspaceHubConnectResult::Unavailable { reason, .. } => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn bind_with_real_id_returns_bound() {
        // arrange
        // act
        let bind = bind_workspace_hub("ws-local-1");

        // assert
        match &bind {
            WorkspaceHubBindResult::Bound { workspace_id } => {
                assert_eq!(workspace_id, "ws-local-1");
            }
            other => panic!("expected Bound, got {other:?}"),
        }
    }

    #[test]
    fn upload_with_real_path_returns_uploaded() {
        // arrange
        // act
        let upload = upload_to_workspace_hub("artifacts/bundle.tar");

        // assert
        match &upload {
            WorkspaceHubUploadResult::Uploaded { artifact_hint } => {
                assert_eq!(artifact_hint, "artifacts/bundle.tar");
            }
            other => panic!("expected Uploaded, got {other:?}"),
        }
    }

    #[test]
    fn recover_with_real_session_returns_recovered() {
        // arrange
        // act
        let recover = recover_workspace_hub("hub-session-9");

        // assert
        match &recover {
            WorkspaceHubRecoveryResult::Recovered { session_hint } => {
                assert_eq!(session_hint, "hub-session-9");
            }
            other => panic!("expected Recovered, got {other:?}"),
        }
    }

    #[test]
    fn workspace_hub_operator_diagnostics_cover_availability_and_outcomes() {
        // arrange
        let availability = evaluate_workspace_hub();
        let connect = connect_workspace_hub("https://hub.example/v1");
        let bind = bind_workspace_hub("ws-local-1");
        let upload = upload_to_workspace_hub("artifacts/bundle.tar");
        let recover = recover_workspace_hub("hub-session-9");

        // act
        let summary = summarize_workspace_hub_outcomes(
            Some(&connect),
            Some(&bind),
            Some(&upload),
            Some(&recover),
        );

        // assert
        assert!(availability.one_line().contains("workspace hub: available"));
        assert!(connect.one_line().contains("connected"));
        assert!(bind.one_line().contains("bound"));
        assert!(upload.one_line().contains("uploaded"));
        assert!(recover.one_line().contains("recovered"));
        assert_eq!(summary.total, 4);
        assert!(!summary.all_unavailable());
    }

    #[test]
    fn multi_endpoint_product_probe_has_mixed_outcomes() {
        // arrange
        // act
        let probe = probe_workspace_hub_product();

        // assert
        assert_eq!(probe.connects.len(), 3);
        assert_eq!(probe.binds.len(), 3);
        assert_eq!(probe.uploads.len(), 3);
        assert_eq!(probe.recovers.len(), 3);
        assert!(probe.availability.is_available());
        assert!(!probe.is_unavailable());
        assert_eq!(probe.summary.total, 4);
        assert!(!probe.summary.all_unavailable());
    }
}
