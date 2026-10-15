//! Tolerant import of stored archive records and the legacy crash-risk guard.
use crate::attachment_transport::AttachmentMetadata;
use crate::event::{ArchiveFrameRecord, ArchiveRecord};

/// Frames at or above this count, or archives at or above [`LEGACY_BYTES`], are large
/// enough to crash a reader when their frames carry no rendering metadata.
const LEGACY_FRAMES: usize = 16;
const LEGACY_BYTES: u64 = 3_000_000;
const LEGACY_TRUNCATED_CHARS: u64 = 1_000_000;
const LEGACY_SOURCE_UNITS: usize = 250_000;

fn frame_is_sound(frame: &ArchiveFrameRecord) -> bool {
    !frame.digest.is_empty()
        && !frame.path.is_empty()
        && frame.bytes > 0
        && frame.width > 0
        && frame.height > 0
}

fn units(text: &str) -> u64 {
    text.encode_utf16().count() as u64
}

/// Validates a stored archive: malformed frames are dropped, nonempty text regions are
/// kept and missing counters default. Returns `None` only when neither a frame nor any
/// nonempty text region survives.
pub(in crate::coord) fn import_archive(mut record: ArchiveRecord) -> Option<ArchiveRecord> {
    record.frames.retain(frame_is_sound);
    if record.frames.is_empty()
        && record.text.is_empty()
        && record.text_head.is_empty()
        && record.text_tail.is_empty()
    {
        return None;
    }
    if record.chars == 0 {
        record.chars = units(&record.text_head)
            .saturating_add(units(&record.text_tail))
            .saturating_add(record.frames.iter().map(|f| f.chars).sum::<u64>());
    }
    Some(record)
}

/// Frame attachments of a record, metadata only. Bytes resolve lazily through the
/// context's frame source when a request selects the frame.
pub(in crate::coord) fn archive_frame_attachments(
    record: &ArchiveRecord,
) -> Vec<AttachmentMetadata> {
    record
        .frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            AttachmentMetadata::new(
                format!("archive-{index}"),
                "image/png",
                frame.bytes,
                Some(crate::attachment_transport::AttachmentDimensions {
                    width: frame.width,
                    height: frame.height,
                }),
                format!("attachment:blake3:{}", frame.digest),
            )
        })
        .collect()
}

/// Whether a persisted archive predates frame metadata and is large enough to risk a
/// reader crash. Modern archives, whose frames name their shape, variant or grid, are
/// exempt.
pub(in crate::coord) fn legacy_crash_risk(record: &ArchiveRecord) -> bool {
    let legacy = record
        .frames
        .iter()
        .any(|f| f.shape.is_none() && f.variant.is_none() && f.columns == 0);
    if !legacy {
        return false;
    }
    let many = record.frames.len() >= LEGACY_FRAMES;
    let base64 = record.frames.iter().fold(0u64, |n, f| {
        n.saturating_add(f.bytes.div_ceil(3).saturating_mul(4))
    });
    if !(many || base64 >= LEGACY_BYTES) {
        return false;
    }
    many || record.truncated_chars >= LEGACY_TRUNCATED_CHARS
        || source_units(record) >= LEGACY_SOURCE_UNITS
}

fn source_units(record: &ArchiveRecord) -> usize {
    let count = |text: &str| text.encode_utf16().count();
    if record.text.is_empty() {
        count(&record.text_head) + 1 + count(&record.text_tail)
    } else {
        count(&record.text)
    }
}
