//! Compaction summary generation, token estimation, and cut-point detection.
//!
//! Ports Pi's compaction logic into Rust. The coordinator owns compaction
//! lifecycle; this module provides pure helpers: token estimation,
//! cut-point detection, prompt building, conversation serialization,
//! and file operation formatting.

#[path = "compaction/active_path_snapshot.rs"]
mod active_path_snapshot;
#[cfg(test)]
#[path = "compaction/active_path_snapshot_tests.rs"]
mod active_path_snapshot_tests;
#[path = "compaction/branch_summary.rs"]
mod branch_summary;
#[path = "compaction/context_projection.rs"]
mod context_projection;
#[path = "compaction/cut_point.rs"]
mod cut_point;
#[path = "compaction/file_ops.rs"]
mod file_ops;
#[path = "compaction/snapshot.rs"]
mod snapshot;
#[path = "compaction/summary.rs"]
mod summary;
#[path = "compaction/tokens.rs"]
mod tokens;

pub use active_path_snapshot::build_active_path_compaction_snapshot;
pub use branch_summary::{
    collect_entries_for_branch_summary, generate_branch_summary, prepare_branch_entries,
    BranchPreparation, BranchSummaryResult, CollectEntriesResult, GenerateBranchSummaryOptions,
    BRANCH_SUMMARY_PREAMBLE, BRANCH_SUMMARY_PREFIX, BRANCH_SUMMARY_PROMPT, BRANCH_SUMMARY_SUFFIX,
};
pub use context_projection::{
    build_session_context, build_session_context_with_branch_summaries,
    estimate_session_context_tokens,
};
pub(crate) use cut_point::{
    estimate_typed_entries_tokens, find_safe_cut_point, SafeCutError, TypedCutPointPlan,
    TypedTextSplit,
};
pub use cut_point::{find_cut_point, find_manual_cut_point, CutPointResult};
pub use file_ops::{
    compute_file_lists, extract_file_ops_from_tool_call, merge_file_operations, FileOperation,
    FileOperations,
};
pub use snapshot::{
    ActiveCompactionBranch, ActivePathCompactionPlan, ActivePathCompactionSnapshot,
    ActivePathCompactionSnapshotInput, CompactionOwner, CompactionPlanBoundary,
    CompactionSnapshotEntry, CompactionSnapshotError, CurrentCompactionModel,
    LegacySourceSequences, OwnedSession, PendingCompactionPrompt, PriorActiveCompactionSummary,
    ToolPairIdentity,
};
pub use summary::{
    build_summarization_prompt, build_turn_prefix_prompt, format_file_operations,
    serialize_conversation, SUMMARIZATION_PROMPT, SUMMARIZATION_SYSTEM_PROMPT,
    TURN_PREFIX_SUMMARIZATION_PROMPT, UPDATE_SUMMARIZATION_PROMPT,
};
pub use tokens::{
    calculate_context_tokens, estimate_context_tokens, estimate_message_tokens,
    estimate_messages_tokens, estimate_text_tokens, ContextUsageEstimate,
};

use crate::config::CompactionSettings;
use crate::conversation::ConversationMessage;

/// Prepared compaction inputs computed from session state.
///
/// Contains the messages to summarize, optional turn-prefix messages for split
/// turns, file operations extracted from the compacted window, and the
/// compaction settings that govern the operation.
#[derive(Debug, Clone)]
pub struct CompactionPreparation {
    /// Sequence number of the first event to keep after compaction.
    pub first_kept_event_seq: u64,
    /// Request id of the first kept turn, if identifiable.
    pub first_kept_request_id: Option<String>,
    /// Messages that will be summarized and then discarded.
    pub messages_to_summarize: Vec<ConversationMessage>,
    /// Messages from the prefix of a split turn (empty when `is_split_turn` is false).
    pub turn_prefix_messages: Vec<ConversationMessage>,
    /// Whether the cut point falls inside a turn, requiring a turn-prefix summary.
    pub is_split_turn: bool,
    /// Estimated token count before compaction.
    pub tokens_before: u32,
    /// Summary from the previous compaction, for iterative update.
    pub previous_summary: Option<String>,
    /// File operations extracted from `messages_to_summarize` (and turn prefix if split).
    pub file_ops: FileOperations,
    /// Compaction settings from the runtime config.
    pub settings: CompactionSettings,
}

/// Result of a compaction operation.
///
/// Contains the generated summary, the boundary metadata, and the computed file
/// lists that were appended to the summary.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The generated compaction summary (including appended file operation tags).
    pub summary: String,
    /// Sequence number of the first event kept after compaction.
    pub first_kept_event_seq: u64,
    /// Request id of the first kept turn, if identifiable.
    pub first_kept_request_id: Option<String>,
    /// Estimated token count before compaction.
    pub tokens_before: u32,
    /// Files that were read (but not modified) during the compacted window.
    pub read_files: Vec<String>,
    /// Files that were edited or written during the compacted window.
    pub modified_files: Vec<String>,
}

#[cfg(test)]
#[path = "compaction/tests.rs"]
mod tests;
