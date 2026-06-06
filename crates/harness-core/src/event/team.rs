use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamCreatedEvent {
    pub team_run_id: String,
    pub spec: TeamSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamSpec {
    pub version: u16,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<TeamMemberSelector>,
    pub members: Vec<TeamMemberSpec>,
    pub bounds: TeamBounds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamMemberSelector {
    Category { category: String },
    SubagentType { subagent_type: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMemberSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "TeamMemberRole::is_default_member")]
    pub role: TeamMemberRole,
    pub selector: TeamMemberSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeamMemberRole {
    #[default]
    Member,
    Research,
}

impl TeamMemberRole {
    pub fn is_default_member(&self) -> bool {
        matches!(self, Self::Member)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Research => "research",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamBounds {
    pub max_members: u32,
    pub max_parallel_members: u32,
    pub max_messages_per_run: u32,
    pub max_wall_clock_minutes: u32,
    pub max_member_turns: u32,
}

impl Default for TeamBounds {
    fn default() -> Self {
        Self {
            max_members: 8,
            max_parallel_members: 4,
            max_messages_per_run: 10_000,
            max_wall_clock_minutes: 120,
            max_member_turns: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMemberSpawnedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub agent_id: String,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamReference {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageKind {
    Message,
    Announcement,
    ShutdownRequest,
    ShutdownApproved,
    ShutdownRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMessage {
    pub version: u16,
    pub message_id: String,
    pub from: String,
    pub to: String,
    pub kind: TeamMessageKind,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<TeamReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamMessageSentEvent {
    pub team_run_id: String,
    pub message: TeamMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskStatus {
    Pending,
    Claimed,
    InProgress,
    Completed,
    Deleted,
}

impl TeamTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTask {
    pub version: u16,
    pub task_id: String,
    pub subject: String,
    pub description: String,
    pub status: TeamTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTaskCreatedEvent {
    pub team_run_id: String,
    pub task: TeamTask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamTaskUpdatedEvent {
    pub team_run_id: String,
    pub task_id: String,
    pub status: TeamTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamShutdownRequestedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub requester: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamShutdownApprovedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub approver: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamShutdownRejectedEvent {
    pub team_run_id: String,
    pub member_name: String,
    pub rejecter: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TeamDeletedEvent {
    pub team_run_id: String,
}
