use serde::{Deserialize, Serialize};

/// Transient observations of the current child attempt; never written to history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentProgressEvent {
    pub child_id: String,
    pub attempt_id: String,
    pub generation: u64,
    pub duration_ms: u64,
    pub turn_count: u32,
    pub tool_call_count: u32,
    pub tokens_used: Option<u64>,
    pub context_window_tokens: Option<u64>,
    pub context_usage_pct: Option<u8>,
    pub tools_used: Vec<String>,
    pub error_count: u32,
}
