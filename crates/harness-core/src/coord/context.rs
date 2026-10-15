use harness_compaction::{
    CompactionMethod, ImageMeta, Item, ItemKind, ItemPart, NoteKind, Origin, Stop, ToolFacts,
};
use harness_providers::{CompletionMessage, CompletionUsage, MessageRole};
use std::{collections::BTreeMap, sync::Arc};

/// One ordered block of an assistant entry, owned.
pub(super) type EntryPart = ItemPart<'static>;

#[derive(Clone, Default)]
pub(super) struct Context {
    /// Request-only render caches (reader views, inline imaging) shared by clones.
    pub render_caches: Arc<RenderCaches>,
    /// Resolves archive frame bytes on demand; `None` when every frame is in memory.
    pub frame_source: Option<Arc<dyn ArchiveFrameSource>>,
    pub semantic: bool,
    pub entries: Vec<Entry>,
    pub usage: Vec<crate::subagent::FinalizedProviderUsage>,
    pub native_context_usage: Option<crate::subagent::SubagentContextUsage>,
    pub unavailable: Option<crate::subagent::FinalizedStateUnavailable>,
    pub model_request: Option<Box<harness_providers::CompletionRequest>>,
}
#[derive(Clone)]
pub(super) struct Entry {
    pub message: CompletionMessage,
    pub seq: u64,
    pub turn: Option<String>,
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
    pub settled_reasoning: Vec<String>,
    pub raw_tool_result: Option<crate::subagent::FinalizedToolResult>,
    pub compaction: Option<Arc<CompactionState>>,
    pub branch_summary: Option<Arc<crate::event::BranchSummaryEvent>>,
    pub tool: ToolFacts,
    pub at_ms: Option<u64>,
    pub stop: Stop,
    /// Usage reported for the provider request that produced this assistant entry.
    /// Cleared when a compaction commits, because it then counts summarized history.
    pub usage: Option<CompletionUsage>,
    pub origin: Origin,
    pub note_kind: Option<NoteKind>,
    /// Ordered assistant blocks; empty for a flat message.
    pub parts: Vec<EntryPart>,
    pub images: Vec<ImageMeta>,
    /// When a reducer replaced this tool result, kept apart from `at_ms`.
    pub pruned_at_ms: Option<u64>,
    /// Output tokens earlier edits already removed from this assistant entry.
    pub elided_output_tokens: u32,
    /// Bumped by every mutating edit so cached counts invalidate. Wraps; never persisted.
    pub version: u32,
    /// `provider/model` of the response that produced an assistant entry; `None` for other
    /// entries or an unknown producer.
    pub model: Option<Arc<str>>,
    /// Memoized price of this entry; invalidated by `version`.
    pub price: PriceCache,
}
impl Entry {
    pub fn new(message: CompletionMessage, seq: u64, turn: Option<String>) -> Self {
        Self {
            message,
            seq,
            turn,
            attachments: Vec::new(),
            settled_reasoning: Vec::new(),
            raw_tool_result: None,
            compaction: None,
            branch_summary: None,
            tool: ToolFacts::default(),
            at_ms: None,
            stop: Stop::Complete,
            usage: None,
            origin: Origin::User,
            note_kind: None,
            parts: Vec::new(),
            images: Vec::new(),
            pruned_at_ms: None,
            elided_output_tokens: 0,
            version: 0,
            model: None,
            price: PriceCache::default(),
        }
    }
}
#[derive(Clone)]
pub(super) struct CompactionState {
    pub summary: String,
    pub short_summary: Option<String>,
    pub method: CompactionMethod,
    pub tokens_before: u32,
    pub tokens_after: Option<u32>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub warning: Option<String>,
    pub archive: Option<crate::event::ArchiveRecord>,
    pub native: Option<crate::event::NativeCompactionMeta>,
    /// A hook produced the summary instead of a summarizer.
    pub from_hook: bool,
    pub details: Option<serde_json::Value>,
    pub preserve_data: BTreeMap<String, serde_json::Value>,
    /// Journal sequence of the compaction event, or 0 for a draft that is not committed.
    pub commit_seq: u64,
    /// First retained entry's time minus one, else the first later entry's time.
    pub history_rewrite_at_ms: Option<u64>,
}
#[derive(Clone)]
pub(super) struct ArchiveProjection {
    pub text: String,
    pub parts: Vec<harness_providers::MessageContentPart>,
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
}

pub(super) mod archive;
mod tokens;
pub(super) use archive::{
    archive_projection, summary_entry, summary_text, ArchiveFrameSource, InlineCache, ReaderViews,
    RenderCaches, RunDirFrames,
};
pub(super) use tokens::{native_tokens_with, tokens_with, PriceCache};

impl Context {
    pub fn new(system: &str) -> Self {
        let mut context = Self::default();
        if !system.is_empty() {
            context.push(
                CompletionMessage::text(MessageRole::System, system),
                0,
                None,
            );
        }
        context
    }
    pub fn push(&mut self, message: CompletionMessage, seq: u64, turn: Option<&str>) {
        self.push_timed(message, seq, turn, None);
    }
    /// Appends an entry stamped with the time of the event that produced it.
    pub fn push_timed(
        &mut self,
        message: CompletionMessage,
        seq: u64,
        turn: Option<&str>,
        at_ms: Option<u64>,
    ) {
        let mut entry = Entry::new(message, seq, turn.map(str::to_owned));
        entry.at_ms = at_ms;
        self.entries.push(entry);
    }
    /// Index of the first entry after the standing prefix: the leading system messages and
    /// the hidden notes-window note that rides with them.
    pub fn body_start(&self) -> usize {
        self.entries
            .iter()
            .position(|e| !is_prefix(e))
            .unwrap_or(self.entries.len())
    }
    pub fn messages(&self) -> Vec<CompletionMessage> {
        self.entries.iter().map(|e| e.message.clone()).collect()
    }
    pub fn attachments(
        &self,
    ) -> std::collections::BTreeMap<usize, Vec<crate::attachment_transport::AttachmentMetadata>>
    {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.attachments.is_empty())
            .map(|(index, entry)| (index, entry.attachments.clone()))
            .collect()
    }
    pub fn discard_turn(&mut self, turn: &str) {
        self.entries.retain(|e| e.turn.as_deref() != Some(turn));
    }
}

/// A system message, or the notes-window note that sits ahead of the compaction summary.
fn is_prefix(entry: &Entry) -> bool {
    entry.message.role == MessageRole::System
        || (entry.note_kind == Some(NoteKind::NotesWindow) && entry.turn.is_none())
}
