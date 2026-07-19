//! Remote workspace / computer-hub connect surface (honest MVP).
//!
//! Remote workspace hub connect/bind/upload/recovery is **not** productized.
//! Callers receive structured unavailability rather than a silent allow.

use serde::{Deserialize, Serialize};

/// Remote workspace hub availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WorkspaceHubAvailability {
    /// Hub connect is ready (not used by this MVP).
    Available { endpoint: String },
    /// Hub product surface is not implemented.
    Unavailable { reason: String },
}

impl WorkspaceHubAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim hub product).
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
///
/// MVP: always structured unavailable.
pub fn evaluate_workspace_hub() -> WorkspaceHubAvailability {
    WorkspaceHubAvailability::Unavailable {
        reason: "remote workspace/computer-hub connect/bind/upload/recovery is not productized"
            .to_string(),
    }
}

/// Attempt to connect is fail-closed in this MVP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WorkspaceHubConnectResult {
    Unavailable {
        reason: String,
        endpoint_hint: String,
    },
}

impl WorkspaceHubConnectResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                endpoint_hint,
            } => format!("workspace hub connect: unavailable `{endpoint_hint}` ({reason})"),
        }
    }
}

/// Bind a local workspace to a remote hub session (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WorkspaceHubBindResult {
    Unavailable {
        reason: String,
        workspace_id: String,
    },
}

impl WorkspaceHubBindResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                workspace_id,
            } => format!("workspace hub bind: unavailable `{workspace_id}` ({reason})"),
        }
    }
}

/// Upload workspace artifacts to a remote hub (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WorkspaceHubUploadResult {
    Unavailable {
        reason: String,
        artifact_hint: String,
    },
}

impl WorkspaceHubUploadResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                artifact_hint,
            } => format!("workspace hub upload: unavailable `{artifact_hint}` ({reason})"),
        }
    }
}

/// Recover a remote hub workspace session (fail-closed MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum WorkspaceHubRecoveryResult {
    Unavailable {
        reason: String,
        session_hint: String,
    },
}

impl WorkspaceHubRecoveryResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Unavailable {
                reason,
                session_hint,
            } => format!("workspace hub recover: unavailable `{session_hint}` ({reason})"),
        }
    }
}

/// Operator-facing counts for hub operation outcomes (diagnostics only).
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

/// Summarize a set of fail-closed hub outcomes for operator surfaces.
pub fn summarize_workspace_hub_outcomes(
    connect: Option<&WorkspaceHubConnectResult>,
    bind: Option<&WorkspaceHubBindResult>,
    upload: Option<&WorkspaceHubUploadResult>,
    recover: Option<&WorkspaceHubRecoveryResult>,
) -> WorkspaceHubOutcomeSummary {
    let mut summary = WorkspaceHubOutcomeSummary::default();
    if connect.is_some() {
        summary.connect_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    if bind.is_some() {
        summary.bind_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    if upload.is_some() {
        summary.upload_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    if recover.is_some() {
        summary.recover_unavailable = 1;
        summary.total = summary.total.saturating_add(1);
    }
    summary
}

fn hub_unavailable_reason() -> String {
    match evaluate_workspace_hub() {
        WorkspaceHubAvailability::Available { .. } => {
            "hub marked available but backend is not implemented".to_string()
        }
        WorkspaceHubAvailability::Unavailable { reason } => reason,
    }
}

pub fn connect_workspace_hub(endpoint: impl Into<String>) -> WorkspaceHubConnectResult {
    WorkspaceHubConnectResult::Unavailable {
        reason: hub_unavailable_reason(),
        endpoint_hint: endpoint.into(),
    }
}

pub fn bind_workspace_hub(workspace_id: impl Into<String>) -> WorkspaceHubBindResult {
    let workspace_id = workspace_id.into();
    WorkspaceHubBindResult::Unavailable {
        reason: hub_unavailable_reason(),
        workspace_id,
    }
}

pub fn upload_to_workspace_hub(artifact_hint: impl Into<String>) -> WorkspaceHubUploadResult {
    let artifact_hint = artifact_hint.into();
    WorkspaceHubUploadResult::Unavailable {
        reason: hub_unavailable_reason(),
        artifact_hint,
    }
}

pub fn recover_workspace_hub(session_hint: impl Into<String>) -> WorkspaceHubRecoveryResult {
    let session_hint = session_hint.into();
    WorkspaceHubRecoveryResult::Unavailable {
        reason: hub_unavailable_reason(),
        session_hint,
    }
}

/// Default multi-endpoint connect probes for the product walk.
pub const DEFAULT_WORKSPACE_HUB_CONNECT_PROBES: &[&str] =
    &["(probe)", "(probe-alt)", "https://hub.example/v1"];

/// Default multi-endpoint bind probes for the product walk.
pub const DEFAULT_WORKSPACE_HUB_BIND_PROBES: &[&str] =
    &["(probe)", "(probe-alt)", "(probe-workspace-2)"];

/// Default multi-endpoint upload probes for the product walk.
pub const DEFAULT_WORKSPACE_HUB_UPLOAD_PROBES: &[&str] =
    &["(probe-artifact)", "(probe-artifact-2)", "(probe-bundle)"];

/// Default multi-endpoint recover probes for the product walk.
pub const DEFAULT_WORKSPACE_HUB_RECOVER_PROBES: &[&str] = &[
    "(probe-session)",
    "(probe-session-2)",
    "(probe-session-stale)",
];

/// Multi-endpoint workspace hub product probe (fail-closed MVP).
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

/// Walk connect across multiple endpoints (each fail-closed).
pub fn walk_workspace_hub_connect(endpoints: &[&str]) -> Vec<WorkspaceHubConnectResult> {
    endpoints
        .iter()
        .map(|endpoint| connect_workspace_hub(*endpoint))
        .collect()
}

/// Walk bind across multiple workspace ids (each fail-closed).
pub fn walk_workspace_hub_bind(workspace_ids: &[&str]) -> Vec<WorkspaceHubBindResult> {
    workspace_ids
        .iter()
        .map(|id| bind_workspace_hub(*id))
        .collect()
}

/// Walk upload across multiple artifact hints (each fail-closed).
pub fn walk_workspace_hub_upload(artifacts: &[&str]) -> Vec<WorkspaceHubUploadResult> {
    artifacts
        .iter()
        .map(|artifact| upload_to_workspace_hub(*artifact))
        .collect()
}

/// Walk recover across multiple session hints (each fail-closed).
pub fn walk_workspace_hub_recover(sessions: &[&str]) -> Vec<WorkspaceHubRecoveryResult> {
    sessions
        .iter()
        .map(|session| recover_workspace_hub(*session))
        .collect()
}

/// Product path: multi-endpoint connect×N + bind×N + upload×N + recover×N.
///
/// Summary is last-of-each (total=4) for operator surfaces. Never claims a live
/// remote hub peer or successful connect/bind/upload/recovery.
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
    fn workspace_hub_reports_structured_unavailable() {
        // arrange
        // act
        // assert
        let availability = evaluate_workspace_hub();
        assert!(availability.is_unavailable());
        assert!(!availability.is_available());
        match availability {
            WorkspaceHubAvailability::Unavailable { reason } => {
                assert!(reason.contains("hub") || reason.contains("not productized"));
            }
            WorkspaceHubAvailability::Available { .. } => panic!("must not claim available"),
        }
    }

    #[test]
    fn connect_workspace_hub_fails_closed_with_endpoint_hint() {
        // arrange
        // act
        // assert
        match connect_workspace_hub("https://example.invalid/hub") {
            WorkspaceHubConnectResult::Unavailable {
                reason,
                endpoint_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(endpoint_hint, "https://example.invalid/hub");
            }
        }
    }

    #[test]
    fn bind_upload_recover_fail_closed_with_operator_hints() {
        // arrange
        // act
        // assert
        // Given / When
        let bind = bind_workspace_hub("ws-local-1");
        let upload = upload_to_workspace_hub("artifacts/bundle.tar");
        let recover = recover_workspace_hub("hub-session-9");

        // Then: never silent allow; retain operator hints for diagnostics
        match bind {
            WorkspaceHubBindResult::Unavailable {
                reason,
                workspace_id,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(workspace_id, "ws-local-1");
            }
        }
        match upload {
            WorkspaceHubUploadResult::Unavailable {
                reason,
                artifact_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(artifact_hint, "artifacts/bundle.tar");
            }
        }
        match recover {
            WorkspaceHubRecoveryResult::Unavailable {
                reason,
                session_hint,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(session_hint, "hub-session-9");
            }
        }
    }

    #[test]
    fn workspace_hub_operator_diagnostics_cover_availability_and_outcomes() {
        // arrange
        // act
        // assert
        // Given
        let availability = evaluate_workspace_hub();
        let connect = connect_workspace_hub("https://example.invalid/hub");
        let bind = bind_workspace_hub("ws-local-1");
        let upload = upload_to_workspace_hub("artifacts/bundle.tar");
        let recover = recover_workspace_hub("hub-session-9");

        // When
        let summary = summarize_workspace_hub_outcomes(
            Some(&connect),
            Some(&bind),
            Some(&upload),
            Some(&recover),
        );

        // Then
        assert!(availability
            .one_line()
            .contains("workspace hub: unavailable"));
        assert!(connect
            .one_line()
            .contains("workspace hub connect: unavailable"));
        assert!(connect.one_line().contains("https://example.invalid/hub"));
        assert!(bind.one_line().contains("ws-local-1"));
        assert!(upload.one_line().contains("artifacts/bundle.tar"));
        assert!(recover.one_line().contains("hub-session-9"));
        assert_eq!(
            summary,
            WorkspaceHubOutcomeSummary {
                connect_unavailable: 1,
                bind_unavailable: 1,
                upload_unavailable: 1,
                recover_unavailable: 1,
                total: 4,
            }
        );
        assert!(summary.all_unavailable());
        assert!(summary.one_line().contains("4 total"));
    }

    #[test]
    fn multi_endpoint_product_probe_all_unavailable_with_last_bindings() {
        // arrange
        // act
        // assert
        // Given / When
        let probe = probe_workspace_hub_product();

        // Then: 3× each op, last-of-each summary total=4
        assert_eq!(probe.connects.len(), 3);
        assert_eq!(probe.binds.len(), 3);
        assert_eq!(probe.uploads.len(), 3);
        assert_eq!(probe.recovers.len(), 3);
        assert!(probe.availability.is_unavailable());
        assert!(probe.is_unavailable());
        assert_eq!(probe.summary.total, 4);
        assert!(probe.summary.all_unavailable());
        assert!(
            probe
                .last_connect
                .one_line()
                .contains("https://hub.example/v1"),
            "last connect: {}",
            probe.last_connect.one_line()
        );
        assert!(
            probe.last_bind.one_line().contains("(probe-workspace-2)"),
            "last bind: {}",
            probe.last_bind.one_line()
        );
        assert!(
            probe.last_upload.one_line().contains("(probe-bundle)"),
            "last upload: {}",
            probe.last_upload.one_line()
        );
        assert!(
            probe
                .last_recover
                .one_line()
                .contains("(probe-session-stale)"),
            "last recover: {}",
            probe.last_recover.one_line()
        );
        for connect in &probe.connects {
            assert!(connect.one_line().contains("unavailable"));
        }
        for bind in &probe.binds {
            assert!(bind.one_line().contains("unavailable"));
        }
        for upload in &probe.uploads {
            assert!(upload.one_line().contains("unavailable"));
        }
        for recover in &probe.recovers {
            assert!(recover.one_line().contains("unavailable"));
        }
    }
}
