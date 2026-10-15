//! On-demand archive frame bytes: restore keeps frame metadata only, and a request reads
//! exactly the frames that survive the payload budget.
use crate::attachment_transport::AttachmentMetadata;
use std::path::PathBuf;

/// Resolves archive frame artifacts without loading them up front.
pub(in crate::coord) trait ArchiveFrameSource: Send + Sync {
    /// Decoded size of the frame artifact, or `None` when it is missing or unreadable.
    fn size(&self, frame: &AttachmentMetadata) -> Option<u64>;
    /// The frame with verified bytes loaded, or `None` when it cannot be read.
    fn load(&self, frame: &AttachmentMetadata) -> Option<AttachmentMetadata>;
}

/// Frames stored as private attachment artifacts of a run directory.
pub(in crate::coord) struct RunDirFrames {
    run_dir: PathBuf,
}

impl RunDirFrames {
    pub(in crate::coord) fn new(run_dir: PathBuf) -> Self {
        Self { run_dir }
    }

    fn path(&self, frame: &AttachmentMetadata) -> Option<PathBuf> {
        let digest = frame
            .content_ref
            .strip_prefix("attachment:blake3:")
            .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))?;
        Some(self.run_dir.join(format!("artifacts/{digest}.attachment")))
    }
}

impl ArchiveFrameSource for RunDirFrames {
    fn size(&self, frame: &AttachmentMetadata) -> Option<u64> {
        let file = crate::store::open_private_file(&self.path(frame)?).ok()?;
        let len = file.metadata().ok()?.len();
        (len <= harness_providers::attachment_protocol::MAX_ATTACHMENT_BYTES as u64).then_some(len)
    }

    fn load(&self, frame: &AttachmentMetadata) -> Option<AttachmentMetadata> {
        let max = harness_providers::attachment_protocol::MAX_ATTACHMENT_BYTES as u64;
        let bytes = crate::store::read_private_bytes(&self.path(frame)?, max)
            .ok()
            .flatten()?;
        frame.clone().with_bytes(bytes).ok()
    }
}
