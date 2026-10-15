//! Reasoning-free reader views of archive summaries.
use crate::attachment_transport::AttachmentMetadata;
use crate::coord::context::CompactionState;
use crate::event::ArchiveRecord;
use harness_compaction::archive::{has_reasoning_legend, ReaderPolicy};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::OnceCell;

/// The summary a reasoning-excluding reader sees in place of an archive that carries the
/// model's reasoning. The original summary, archive and frames are never changed.
#[derive(Debug, Clone)]
pub(in crate::coord) struct ReaderView {
    pub summary: String,
    pub archive: ArchiveRecord,
    /// Frame attachments: bytes loaded for a fresh render, metadata only for a view
    /// replayed from a journal patch.
    pub frames: Vec<AttachmentMetadata>,
    /// Rendering failed: text only, never persisted.
    pub fail_closed: bool,
}

type ViewCell = Arc<OnceCell<Arc<ReaderView>>>;

/// Views already rendered, persisted or replayed in this process, per compaction and
/// complete reader key. Each key renders at most once; concurrent callers share the cell.
#[derive(Default)]
pub(in crate::coord) struct ReaderViews {
    cells: HashMap<(u64, String), ViewCell>,
}

impl ReaderViews {
    /// The once-cell of one compaction and reader key.
    pub(in crate::coord) fn cell(&mut self, seq: u64, key: &str) -> ViewCell {
        Arc::clone(self.cells.entry((seq, key.to_owned())).or_default())
    }

    /// The view when it is ready; never starts a render.
    pub(in crate::coord) fn ready(&self, seq: u64, key: &str) -> Option<Arc<ReaderView>> {
        self.cells
            .get(&(seq, key.to_owned()))
            .and_then(|cell| cell.get().cloned())
    }

    /// Stores a replayed view; a later patch replaces an earlier one.
    pub(in crate::coord) fn seed(&mut self, seq: u64, key: &str, view: ReaderView) {
        self.cells.insert(
            (seq, key.to_owned()),
            Arc::new(OnceCell::new_with(Some(Arc::new(view)))),
        );
    }
}

/// Whether a reader under `policy` must see a reasoning-free view of this summary.
pub(in crate::coord) fn needs_reader_view(state: &CompactionState, policy: ReaderPolicy) -> bool {
    policy.excludes_archived_reasoning
        && state.archive.is_some()
        && has_reasoning_legend(&state.summary)
}
