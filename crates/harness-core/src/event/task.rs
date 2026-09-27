use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpawnedEvent {
    pub agent_id: String,
    pub profile: String,
    pub parent_agent_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStoppedEvent {
    pub agent_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskScheduleState {
    Queued,
    Started,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTerminalScope {
    AgentTurn,
    ToolCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskScheduledEvent {
    pub task_id: crate::ids::TaskId,
    pub state: TaskScheduleState,
    pub queue_key: Option<String>,
    pub metadata: Option<TaskScheduleMetadata>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskScheduleMetadata {
    pub lineage: Option<TaskLineageMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskLineageMetadata {
    pub parent_tool_call_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub parent_request_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub child_session_id: Option<String>,
    pub child_request_id: Option<String>,
    pub child_provider_id: Option<String>,
    pub child_model_id: Option<String>,
}
impl TaskLineageMetadata {
    pub fn non_empty_parent_session_id(&self) -> Option<&str> {
        self.parent_session_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionTimingMetadata {
    pub started_mono_ms: Option<u64>,
    pub finished_mono_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskCompletionMetadata {
    pub lineage: Option<TaskLineageMetadata>,
    pub task_scope: Option<TaskTerminalScope>,
    pub timing: Option<ExecutionTimingMetadata>,
    pub hook_executions: Vec<super::HookExecutionMetadata>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCancelledEvent {
    #[serde(default)]
    pub failure: bool,
    pub task_id: crate::ids::TaskId,
    pub reason: String,
    pub task_scope: Option<TaskTerminalScope>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCompletedEvent {
    pub task_id: crate::ids::TaskId,
    pub result_summary: String,
    pub result_digest: String,
    pub metadata: Option<TaskCompletionMetadata>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskResultLateEvent {
    pub task_id: crate::ids::TaskId,
    pub result_digest: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleDetectedEvent {
    pub task_id: crate::ids::TaskId,
    pub stale_for_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskNotificationStatus {
    Completed,
    Cancelled,
    Failed,
    TimedOut,
}
impl BackgroundTaskNotificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskNotificationEvent {
    pub parent_session_id: crate::ids::SessionId,
    pub parent_agent_id: Option<String>,
    pub child_session_id: crate::ids::SessionId,
    pub child_request_id: String,
    pub task_id: crate::ids::TaskId,
    pub description: String,
    pub status: BackgroundTaskNotificationStatus,
    pub summary: String,
    pub terminal_event_id: String,
    pub terminal_task_id: String,
    pub delivered_turn_request_id: Option<String>,
}
