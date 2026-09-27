use super::{ExecutionTimingMetadata, TaskLineageMetadata};
pub use crate::perm::PermissionDecision;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionStatus {
    Succeeded,
    Blocked,
    Failed,
    Skipped,
    #[default]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookExecutionMetadata {
    pub hook_name: String,
    pub status: HookExecutionStatus,
    pub hook_event: Option<String>,
    pub command_digest: Option<String>,
    pub output_digest: Option<String>,
    pub output_summary: Option<String>,
    pub duration_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventArtifactRef {
    pub path: String,
    pub digest: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolCallMetadata {
    pub canonical_tool_id: Option<String>,
    pub alias_source_tool_id: Option<String>,
    pub lineage: Option<TaskLineageMetadata>,
    pub artifact_refs: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
    pub timing: Option<ExecutionTimingMetadata>,
    pub hook_executions: Vec<HookExecutionMetadata>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolIdentityMetadata {
    pub canonical_tool_id: Option<String>,
    pub alias_source_tool_id: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResolvedToolIdentity {
    pub invoked_tool_id: Option<String>,
    pub effective_tool_id: Option<String>,
    pub canonical_tool_id: Option<String>,
    pub alias_source_tool_id: Option<String>,
}
impl ResolvedToolIdentity {
    pub fn from_tool_call(invoked: Option<&str>, metadata: Option<&ToolCallMetadata>) -> Self {
        Self::resolve(
            invoked,
            metadata.and_then(|m| m.canonical_tool_id.as_deref()),
            metadata.and_then(|m| m.alias_source_tool_id.as_deref()),
        )
    }
    pub fn from_tool_artifact(
        invoked: Option<&str>,
        metadata: Option<&ToolIdentityMetadata>,
    ) -> Self {
        Self::resolve(
            invoked,
            metadata.and_then(|m| m.canonical_tool_id.as_deref()),
            metadata.and_then(|m| m.alias_source_tool_id.as_deref()),
        )
    }
    fn resolve(invoked: Option<&str>, canonical: Option<&str>, alias: Option<&str>) -> Self {
        let clean = |s: Option<&str>| {
            s.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let invoked = clean(invoked);
        let canonical = clean(canonical);
        Self {
            effective_tool_id: canonical.clone().or_else(|| invoked.clone()),
            canonical_tool_id: if invoked.as_deref().is_some_and(|s| s.starts_with("mcp.")) {
                None
            } else {
                canonical
            },
            invoked_tool_id: invoked,
            alias_source_tool_id: clean(alias),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.invoked_tool_id.is_none()
            && self.effective_tool_id.is_none()
            && self.canonical_tool_id.is_none()
            && self.alias_source_tool_id.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Succeeded,
    Failed,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallLifecycleState {
    #[default]
    Pending,
    Running,
    Completed,
    Error,
}
impl ToolCallLifecycleState {
    pub fn from_finish_status(status: ToolCallStatus) -> Self {
        match status {
            ToolCallStatus::Succeeded => Self::Completed,
            ToolCallStatus::Failed => Self::Error,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallRequestedEvent {
    pub tool_call_id: crate::ids::ToolCallId,
    pub tool_id: String,
    pub args_summary: String,
    pub args_digest: String,
    pub metadata: Option<ToolCallMetadata>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallStartedEvent {
    pub tool_call_id: crate::ids::ToolCallId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallFinishedEvent {
    pub tool_call_id: crate::ids::ToolCallId,
    pub status: ToolCallStatus,
    pub output_summary: Option<String>,
    pub output_digest: Option<String>,
    pub output_json: Option<serde_json::Value>,
    pub metadata: Option<ToolCallMetadata>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequestedEvent {
    pub permission_id: String,
    pub kind: String,
    pub tool_call_id: Option<crate::ids::ToolCallId>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: PermissionDecision,
}
pub type PermissionRequestedArgs = PermissionRequestedEvent;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionResolvedEvent {
    pub permission_id: String,
    pub decision: PermissionDecision,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrantRecordedEvent {
    pub grant: crate::perm::PermissionGrant,
}
