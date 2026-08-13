#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptFooterLifecycle {
    Settled,
    Permission,
    Question,
    Retry,
    Responding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptLifecycleState {
    Queued,
    Responding,
    Retrying {
        attempt: u32,
        max_attempts: u32,
        elapsed_ms: Option<u64>,
    },
    Cancelled,
    Failed,
    Recovered,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptFooterMetadata {
    pub(in crate::ui) duration_ms: Option<u64>,
    pub(in crate::ui) total_tokens: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptFooterContent {
    Settled,
    Permission {
        tool_id: String,
        label: String,
        metadata: TranscriptFooterMetadata,
    },
    Question {
        label: String,
        metadata: TranscriptFooterMetadata,
    },
    Retry {
        attempt: u32,
        metadata: TranscriptFooterMetadata,
    },
    Responding {
        metadata: TranscriptFooterMetadata,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptPromptState {
    Idle,
    Selected,
    ActiveThinking,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptBlockContent {
    UserMessage {
        text: String,
        queued: bool,
        wall_clock: Option<String>,
        state: TranscriptPromptState,
    },
    AssistantBody {
        text: String,
        streaming: bool,
        wall_clock: Option<String>,
        has_tools: bool,
    },
    Reasoning {
        text: String,
        active: bool,
        expanded: bool,
        duration_ms: Option<u64>,
        motion_enabled: bool,
    },
    Tool {
        family: TranscriptToolFamily,
        ids: Vec<String>,
        policy: TranscriptToolPolicy,
        subagent: Option<TranscriptSubagentPolicy>,
    },
    Footer {
        lifecycle: TranscriptFooterLifecycle,
        state: TranscriptLifecycleState,
        content: TranscriptFooterContent,
    },
    Error {
        message: String,
    },
    Compaction {
        branch_summary: bool,
        summary: String,
        tokens_before: Option<u32>,
        read_files: Vec<String>,
        modified_files: Vec<String>,
    },
    #[cfg(test)]
    Synthetic {
        value: String,
    },
}
use super::*;
