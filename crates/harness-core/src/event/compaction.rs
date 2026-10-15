pub use harness_compaction::{CompactionMethod, ManualMode, ShakeMode};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// `ArtifactWritten` metadata `kind` of a stored native compaction payload. Exports and
/// catalog listings skip artifacts of this kind.
pub const NATIVE_ARTIFACT_KIND: &str = "native_compaction";

/// Reads a compaction method, treating a value this build does not know as absent so an
/// older or newer journal never fails to replay.
pub fn lenient_method<'de, D>(deserializer: D) -> Result<Option<CompactionMethod>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

/// A provider-native compaction payload stored as a private artifact. The event never
/// carries the payload itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePayloadRef {
    /// Run-relative path, `artifacts/native-<blake3>.bin`.
    pub path: String,
    pub digest: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCompactionMeta {
    pub lane: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    /// Wire protocol the payload belongs to: `responses`, `anthropic` or `chat`.
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub payload: Option<NativePayloadRef>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveRecord {
    pub text: String,
    pub text_head: String,
    pub text_tail: String,
    pub truncated_chars: u64,
    pub shape: String,
    pub chars: u64,
    pub frames: Vec<ArchiveFrameRecord>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveFrameRecord {
    pub path: String,
    pub digest: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub chars: u64,
    /// Nominal grid of the frame, independent of its pixel size.
    pub columns: u32,
    pub rows: u32,
    pub shape: Option<String>,
    pub detail: Option<String>,
    pub variant: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPrunedEvent {
    pub agent_id: String,
    pub kind: PruneKindRecord,
    pub edits: Vec<ContextEdit>,
    pub tokens_saved: u32,
    pub artifact_path: Option<String>,
    pub warning: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PruneKindRecord {
    Superseded,
    Useless,
    Aged,
    Shake,
    Images,
    Thinking,
    Rescue,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextEdit {
    ToolResult {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
        tool_call_id: String,
        text: String,
        pruned_at_ms: u64,
        /// Also drop the result's attachments (the prune kinds do, a shake does not).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        drop_attachments: bool,
    },
    TextRange {
        seq: u64,
        start: u32,
        end: u32,
        text: String,
        /// Output tokens this edit removed from an assistant entry.
        #[serde(default, skip_serializing_if = "is_zero")]
        elided_output_tokens: u32,
    },
    DropReasoning {
        seq: u64,
        #[serde(default, skip_serializing_if = "is_zero")]
        elided_output_tokens: u32,
    },
    DropAttachments {
        seq: u64,
        tool_call_id: Option<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionStage {
    pub reason: String,
    #[serde(default, deserialize_with = "lenient_method")]
    pub method: Option<CompactionMethod>,
    pub phase: CompactionPhase,
    pub tokens_before: Option<u32>,
    pub tokens_after: Option<u32>,
    pub summarized_tokens: Option<u32>,
    pub kept_tokens: Option<u32>,
    pub fixed_tokens: Option<u32>,
    pub threshold_tokens: Option<u32>,
    pub window_tokens: Option<u32>,
    pub first_kept_seq: Option<u64>,
    pub windows_done: u32,
    pub windows_total: u32,
    pub frames_done: u32,
    pub frames_total: u32,
    pub thumbnails: Vec<FrameThumbnail>,
    pub message: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPhase {
    Preparing,
    Summarizing,
    TurnPrefix,
    ShortSummary,
    Archiving,
    Remote,
    Handoff,
    Shaking,
    Committing,
    Done,
    Failed,
    Cancelled,
    Skipped,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameThumbnail {
    pub width: u16,
    pub height: u16,
    pub bits: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCompactionEvent {
    pub agent_id: String,
    pub summary: String,
    pub first_kept_event_seq: u64,
    pub first_kept_request_id: Option<String>,
    pub first_kept_entry_id: Option<crate::ids::EntryId>,
    pub tokens_before: u32,
    pub tokens_after: Option<u32>,
    #[serde(alias = "summary_generation_usage")]
    pub summary_usage: Option<harness_providers::CompletionUsage>,
    pub summary_provider_id: Option<String>,
    pub summary_model_id: Option<String>,
    #[serde(default)]
    pub read_files: Vec<String>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    pub task_intent: Option<String>,
    pub current_intent: Option<super::UiIntentReceivedEvent>,
    pub trigger_reason: String,
    #[serde(default)]
    pub from_hook: bool,
    #[serde(default)]
    pub short_summary: Option<String>,
    #[serde(default, deserialize_with = "lenient_method")]
    pub method: Option<CompactionMethod>,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub provider_replay_through_seq: Option<u64>,
    #[serde(default)]
    pub native: Option<NativeCompactionMeta>,
    #[serde(default)]
    pub archive: Option<ArchiveRecord>,
    /// Free-form details a hook or the notes rollover attached to the summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    /// Redacted data a hook asked to keep across compactions, at most 64 KiB serialized.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preserve_data: BTreeMap<String, Value>,
    /// The earlier compaction this one replaces, when a rescue writes a second boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes_seq: Option<u64>,
    /// Pairs this record with the lifecycle events of the pass that produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSummaryEvent {
    pub agent_id: String,
    pub summary: String,
    pub from_event_seq: u64,
    #[serde(default)]
    pub read_files: Vec<String>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default)]
    pub from_hook: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Append-only amendment of an earlier compaction record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionPatchedEvent {
    pub agent_id: String,
    pub compaction_seq: u64,
    pub patch: CompactionPatch,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompactionPatch {
    /// A warning discovered after the compaction was committed.
    Warning { text: String },
    /// The reasoning-free view of the summary for one reader model.
    ReaderView {
        reader_key: String,
        summary: String,
        archive: Option<ArchiveRecord>,
    },
}
/// One revision of the notes a model keeps across context windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextNotesEvent {
    pub agent_id: String,
    pub version: u8,
    pub text: String,
}

fn is_zero(count: &u32) -> bool {
    *count == 0
}

// Older journals and the unchanged TUI still decode these records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionRequestedEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub trigger_reason: String,
    pub through_seq: u64,
    pub through_request_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub tokens_before: Option<u32>,
    pub tokens_before_estimate: Option<u32>,
    pub estimate_source: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionWrittenEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub artifact_path: String,
    pub artifact_digest: Option<String>,
    pub artifact_bytes: u64,
    pub trigger_reason: String,
    pub through_seq: u64,
    pub through_request_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub tokens_before: Option<u32>,
    pub tokens_before_estimate: Option<u32>,
    pub tokens_after_estimate: Option<u32>,
    pub summary_tokens_estimate: Option<u32>,
    pub compacted_turns: Option<u32>,
    pub reduction_tokens_estimate: Option<u32>,
    pub reduction_percent_estimate: Option<u32>,
    pub estimate_source: Option<String>,
    pub summary_source: Option<crate::agent::ProviderCompactionSummarySource>,
    pub preserved_turns: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionAppliedEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub through_seq: u64,
    pub through_request_id: Option<String>,
    pub tokens_before_estimate: Option<u32>,
    pub tokens_after_estimate: Option<u32>,
    pub summary_tokens_estimate: Option<u32>,
    pub compacted_turns: Option<u32>,
    pub preserved_turns: Option<u32>,
    pub reduction_tokens_estimate: Option<u32>,
    pub reduction_percent_estimate: Option<u32>,
    pub estimate_source: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionFailedEvent {
    pub agent_id: String,
    pub trigger_reason: String,
    pub reason: String,
    pub checkpoint_id: Option<String>,
    pub through_seq: Option<u64>,
    pub through_request_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_method_replays_as_absent_and_patches_round_trip() -> Result<(), serde_json::Error> {
        let mut value = serde_json::to_value(SessionCompactionEvent::default())?;
        value["method"] = json!("a_method_from_the_future");
        let event: SessionCompactionEvent = serde_json::from_value(value.clone())?;
        assert_eq!(event.method, None);
        value["method"] = json!("remote");
        let event: SessionCompactionEvent = serde_json::from_value(value)?;
        assert_eq!(event.method, Some(CompactionMethod::Remote));
        let patch = CompactionPatchedEvent {
            agent_id: "root".into(),
            compaction_seq: 7,
            patch: CompactionPatch::ReaderView {
                reader_key: "anthropic:model".into(),
                summary: "text".into(),
                archive: None,
            },
        };
        let restored: CompactionPatchedEvent =
            serde_json::from_value(serde_json::to_value(&patch)?)?;
        assert_eq!(restored, patch);
        Ok(())
    }
}
