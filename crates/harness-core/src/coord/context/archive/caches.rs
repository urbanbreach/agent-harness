//! Request-only render caches shared by every clone of a context.
use super::{
    import::{archive_frame_attachments, import_archive},
    inline_cache::InlineCache,
    reader::{ReaderView, ReaderViews},
};
use crate::event::ArchiveRecord;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Reader views and inline imaging renders. Nothing here is persisted state: durable
/// reader views live in the journal as compaction patches.
#[derive(Default)]
pub(in crate::coord) struct RenderCaches {
    pub reader: Mutex<ReaderViews>,
    pub inline: Mutex<InlineCache>,
}

impl RenderCaches {
    /// Replays a persisted reader view patch. The record is imported tolerantly; a patch
    /// whose archive is invalid is a text-only view. The newest patch per compaction and
    /// key wins.
    pub(in crate::coord) fn seed_reader_view(
        &self,
        compaction_seq: u64,
        reader_key: &str,
        summary: String,
        archive: Option<ArchiveRecord>,
    ) {
        let archive = archive.and_then(import_archive).unwrap_or_default();
        let view = ReaderView {
            frames: archive_frame_attachments(&archive),
            summary,
            archive,
            fail_closed: false,
        };
        self.reader_views().seed(compaction_seq, reader_key, view);
    }

    /// The reader view cache. A render that panicked cannot leave it inconsistent, so a
    /// poisoned lock is still usable.
    pub(in crate::coord) fn reader_views(&self) -> MutexGuard<'_, ReaderViews> {
        self.reader.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
