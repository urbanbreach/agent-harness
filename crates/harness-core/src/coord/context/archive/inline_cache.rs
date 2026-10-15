//! Inline imaging render caches (system prompt and tool results) and request-time state.
use crate::attachment_transport::AttachmentMetadata;
use harness_compaction::archive::{ArchiveShape, RenderedFrame};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Rasterizes text into frames, possibly deferring the real pixels. A sink that returns
/// `None` hands the work back to the synchronous local rasterizer.
pub(in crate::coord) trait FrameSink: Send + Sync {
    /// Changes whenever frames this sink already returned must be rendered again, for
    /// example when deferred placeholders were replaced by real pixels.
    fn generation(&self) -> u64;
    fn rasterize(
        &self,
        text: &str,
        shape: &ArchiveShape,
        max_frames: Option<u32>,
    ) -> Option<Vec<RenderedFrame>>;
}

/// Everything a render depends on: the text, the complete resolved shape and its model
/// price (not only the shape id), the sink generation and the frame count bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) struct RenderKey {
    pub text: [u8; 32],
    pub shape: ArchiveShape,
    pub generation: u64,
    pub bound: u32,
}

/// Outcome of one render.
#[derive(Debug, Clone)]
pub(in crate::coord) enum Rendered {
    /// The frames; empty when the text renders to nothing or rendering failed.
    Frames(Arc<[AttachmentMetadata]>),
    /// The text needs more frames than the bound allows.
    TooMany,
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct RenderEntry {
    pub key: RenderKey,
    pub rendered: Rendered,
}

/// What the cache remembers of one tool result.
#[derive(Debug)]
pub(in crate::coord) struct ToolSlot {
    /// Fingerprint of the candidate text.
    pub text: [u8; 32],
    /// Pricing domain of `tokens`.
    pub domain: u64,
    pub tokens: u32,
    pub render: Option<RenderEntry>,
}

/// Recorder-lifetime state of the savings journal; it survives turns because the cache
/// does.
#[derive(Debug, Default)]
pub(in crate::coord) struct SavingsState {
    pub inner: Mutex<SavingsInner>,
}

#[derive(Debug, Default)]
pub(in crate::coord) struct SavingsInner {
    /// Tool call ids already offered to the journal, whatever session or model.
    pub seen: HashSet<String>,
    /// The directory was created successfully.
    pub dir_ready: bool,
}

/// Render caches of the outgoing-request imaging transformer.
#[derive(Default)]
pub(in crate::coord) struct InlineCache {
    /// Per tool call id.
    pub tools: HashMap<String, ToolSlot>,
    /// The single system prompt entry.
    pub system: Option<RenderEntry>,
    /// Rewrite times of retracted swaps: `(entry seq, tool call id)` to the time the
    /// retraction rewrote previously sent bytes. The id is empty for a system carrier.
    pub stamps: BTreeMap<(u64, String), u64>,
    /// Deferred rasterization; local synchronous rendering when unset.
    pub sink: Option<Arc<dyn FrameSink>>,
    pub savings: Arc<SavingsState>,
    /// Harness data directory the savings journal lives in.
    pub data_dir: Option<PathBuf>,
    /// Fingerprint, pricing domain and text tokens of the system target.
    pub system_tokens: Option<([u8; 32], u64, u32)>,
}

impl InlineCache {
    /// The time a retraction last rewrote the swapped entry, for the history fold into
    /// `Item.pruned_at_ms` (tool results: pass the call id) and the carrier's rewrite time
    /// (system carrier: pass `None`).
    pub(in crate::coord) fn stamp_for(&self, seq: u64, call_id: Option<&str>) -> Option<u64> {
        self.stamps
            .get(&(seq, call_id.unwrap_or_default().to_owned()))
            .copied()
    }

    pub(in crate::coord) fn stamp(&mut self, seq: u64, call_id: Option<&str>, at_ms: u64) {
        let slot = self
            .stamps
            .entry((seq, call_id.unwrap_or_default().to_owned()))
            .or_insert(0);
        *slot = (*slot).max(at_ms);
    }
}
