use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditProposedEvent {
    pub edit_id: String,
    pub path: String,
    pub summary: String,
    pub patch_digest: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditAppliedEvent {
    pub edit_id: String,
    pub path: String,
    pub new_file_digest: String,
    pub diff_rel_path: Option<String>,
    pub diff_digest: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRejectedEvent {
    pub edit_id: String,
    pub path: String,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactWrittenEvent {
    pub path: String,
    pub digest: String,
    pub bytes: u64,
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    pub tool_metadata: Option<super::ToolIdentityMetadata>,
    #[serde(default)]
    pub metadata: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolationDetectedEvent {
    pub policy: String,
    pub detail: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiIntentReceivedEvent {
    pub intent: String,
    pub params: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnapshotEvent {
    pub request_id: crate::ids::RequestId,
    pub artifact_path: String,
    pub artifact_digest: String,
    pub file_count: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRevertedEvent {
    pub request_id: crate::ids::RequestId,
    pub snapshot_request_id: String,
    pub restored_paths: Vec<String>,
    pub removed_paths: Vec<String>,
    pub failed_paths: Vec<WorkspaceRevertFailure>,
    pub conflicts: Vec<WorkspaceRevertFailure>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRevertFailure {
    pub path: String,
    pub reason: String,
}
