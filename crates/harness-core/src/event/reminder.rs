use serde::{Deserialize, Serialize};

/// Model-only guidance the coordinator added to a turn's context.
///
/// The text is exactly what the model received. History reconstruction places it in the
/// owning turn after everything recorded before it, or opens a turn when it is the first
/// entry of a wake turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeReminderEvent {
    pub request_id: crate::ids::RequestId,
    pub kind: RuntimeReminderKind,
    pub text: String,
    /// What the reminder refers to: an instruction file, a command task, a schema digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeReminderKind {
    /// The agent ended its turn with open todo items.
    TodoContinuation,
    /// The agent repeated the same tool calls.
    LoopGuard,
    /// A streamed response was stopped for repetition and is being retried.
    StreamGuard,
    /// Project instructions scoped to a directory the agent touched.
    DirectoryInstructions,
    /// A background command finished.
    CommandCompleted,
    /// A subagent result did not match its requested output schema.
    OutputContract,
}

impl RuntimeReminderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TodoContinuation => "todo_continuation",
            Self::LoopGuard => "loop_guard",
            Self::StreamGuard => "stream_guard",
            Self::DirectoryInstructions => "directory_instructions",
            Self::CommandCompleted => "command_completed",
            Self::OutputContract => "output_contract",
        }
    }
}

/// Durable intent and acknowledgement for command notices not yet placed in context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CommandNoticeEvent {
    Queued {
        task_id: String,
        status: String,
        body: String,
    },
    Observed {
        task_id: String,
    },
    WakeAttempted,
}

/// A user message accepted for an agent's running turn and not yet in its context.
///
/// Delivery records the message as a `UserMessageSubmitted` correlated to the turn. A
/// message the turn never reached becomes a queued turn with the same request id, or is
/// cancelled when no turn can take it. A user interrupt returns it to the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteeringAcceptedEvent {
    pub request_id: crate::ids::RequestId,
    pub turn_id: String,
    pub text: String,
}

/// `TaskCancelled` reason for input the user took back into the editor after interrupting
/// a turn. Transcripts leave such input out, since the agent never saw it.
pub const RETURNED_TO_EDITOR_REASON: &str = "returned to the editor";
