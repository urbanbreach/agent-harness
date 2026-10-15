//! Ordered archive projection: head text, surviving frames newest-first under a payload
//! budget, gap notices and tail text.
use super::{archive_plain, import::legacy_crash_risk, source::ArchiveFrameSource};
use crate::attachment_transport::AttachmentMetadata;
use crate::coord::context::ArchiveProjection;
use crate::event::ArchiveRecord;
use harness_providers::MessageContentPart;

/// Archive frames never exceed this many images, whatever the deployment allows.
const MAX_ARCHIVE_IMAGES: u32 = 80;

/// What a surface may include: base64 bytes and image count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) struct FrameLimits {
    pub bytes: u64,
    pub images: u32,
}

impl FrameLimits {
    /// Transcript views resolve every frame and never byte-cap.
    pub(in crate::coord) const fn transcript() -> Self {
        Self {
            bytes: u64::MAX,
            images: u32::MAX,
        }
    }

    /// Provider requests: the deployment's archive byte budget and the images the rest of
    /// the request leaves free. A legacy archive that could crash a reader gets no bytes.
    pub(in crate::coord) fn provider(
        record: &ArchiveRecord,
        archive_bytes: u64,
        free_images: u32,
    ) -> Self {
        Self {
            bytes: if legacy_crash_risk(record) {
                0
            } else {
                archive_bytes
            },
            images: free_images.min(MAX_ARCHIVE_IMAGES),
        }
    }
}

/// Text surfaces of an archive summary around its frames.
pub(in crate::coord) struct ArchiveText<'a> {
    pub guide: &'a str,
    pub head: &'a str,
    pub tail: &'a str,
    pub truncated: u64,
}

enum Slot {
    Frame(AttachmentMetadata),
    /// Missing or unreadable artifact.
    Unavailable,
    /// Left out by a limit; the amount is the base64 size when a byte budget caused it.
    Omitted(u64),
}

fn base64_len(size: u64) -> u64 {
    size.div_ceil(3).saturating_mul(4)
}

/// Resolves a frame whose bytes may still be on disk. The artifact must exist at the
/// recorded size before its bytes are read.
fn resolve(
    frame: &AttachmentMetadata,
    source: Option<&dyn ArchiveFrameSource>,
) -> Option<AttachmentMetadata> {
    if frame.bytes().is_ok() {
        return Some(frame.clone());
    }
    let source = source?;
    if source.size(frame)? != frame.size {
        return None;
    }
    source.load(frame)
}

/// Newest-first selection; oldest-first output. Dropped frames are never loaded.
fn select(
    frames: &[AttachmentMetadata],
    frame_count: usize,
    source: Option<&dyn ArchiveFrameSource>,
    limits: FrameLimits,
) -> Vec<Slot> {
    let mut slots: Vec<Slot> = (0..frame_count).map(|_| Slot::Unavailable).collect();
    let (mut room, mut images) = (limits.bytes, limits.images);
    for index in (0..frame_count).rev() {
        let Some(frame) = frames.get(index).filter(|frame| frame.size > 0) else {
            continue;
        };
        let cost = base64_len(frame.size);
        let over = cost > room;
        if over || images == 0 {
            slots[index] = Slot::Omitted(if over { cost } else { 0 });
            continue;
        }
        if let Some(frame) = resolve(frame, source) {
            room -= cost;
            images -= 1;
            slots[index] = Slot::Frame(frame);
        }
    }
    slots
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1_000 {
        return format!("{bytes} B");
    }
    let (unit, suffix) = if bytes >= 1_000_000 {
        (1_000_000, "MB")
    } else {
        (1_000, "KB")
    };
    let tenth = unit / 10;
    let rounded = bytes / tenth + u64::from(bytes % tenth >= tenth / 2);
    format!("{}.{} {suffix}", rounded / 10, rounded % 10)
}

fn grouped_count(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

fn append_text(parts: &mut Vec<MessageContentPart>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(MessageContentPart::Text(previous)) = parts.last_mut() {
        previous.push_str(text);
    } else {
        parts.push(MessageContentPart::Text(text.to_owned()));
    }
}

fn gap_notice(unavailable: bool, count: usize, omitted: u64) -> String {
    let plural = if count == 1 { "" } else { "s" };
    let count = grouped_count(count as u64);
    if unavailable {
        return format!(
            "-------------- {count} archived image frame{plural} unavailable here --------------"
        );
    }
    let note = if omitted > 0 {
        format!(
            " {} of base64 exceeded the per-request archive payload budget.",
            format_bytes(omitted)
        )
    } else {
        String::new()
    };
    format!("-------------- archive image middle omitted\n{count} archived image frame{plural} could not be included.{note} The compacted summary and visible text edges remain available.\n--------------")
}

/// Ordered text and frames of an archive summary for one surface. Frames are sized from
/// their metadata, selected newest-first within `limits`, and only the selected ones are
/// loaded through `source`; a frame that cannot be resolved becomes a chronological
/// unavailable slot.
pub(in crate::coord) fn archive_projection(
    text: &ArchiveText<'_>,
    frames: &[AttachmentMetadata],
    frame_count: usize,
    source: Option<&dyn ArchiveFrameSource>,
    limits: FrameLimits,
) -> ArchiveProjection {
    let slots = select(frames, frame_count, source, limits);
    let images = slots.iter().any(|slot| matches!(slot, Slot::Frame(_)));
    let gaps = slots.iter().any(|slot| !matches!(slot, Slot::Frame(_)));
    let mut head = text.guide.to_owned();
    if !text.head.is_empty() {
        head.push_str("\n\n");
        head.push_str(&archive_plain(text.head));
    }
    if images {
        head.push_str("\n-------------- imaged middle below\n");
    }
    let mut parts = vec![MessageContentPart::Text(head)];
    let mut attachments = Vec::new();
    let mut slots = slots.into_iter().peekable();
    while let Some(slot) = slots.next() {
        let (unavailable, mut count, mut omitted) = match slot {
            Slot::Frame(frame) => {
                parts.push(MessageContentPart::Attachment(attachments.len()));
                attachments.push(frame);
                continue;
            }
            Slot::Unavailable => (true, 1, 0),
            Slot::Omitted(bytes) => (false, 1, bytes),
        };
        loop {
            match slots.peek() {
                Some(Slot::Unavailable) if unavailable => {}
                Some(Slot::Omitted(bytes)) if !unavailable => {
                    omitted = omitted.saturating_add(*bytes)
                }
                _ => break,
            }
            slots.next();
            count += 1;
        }
        append_text(&mut parts, &gap_notice(unavailable, count, omitted));
    }
    let mut end = String::new();
    if images {
        end.push_str("-------------- imaged middle above\n");
    } else if text.truncated > 0 || gaps {
        end.push_str("\n-------------- middle history omitted above\n");
    }
    end.push_str(&archive_plain(text.tail));
    append_text(&mut parts, &end);
    let text = parts
        .iter()
        .filter_map(|part| match part {
            MessageContentPart::Text(text) => Some(text.as_str()),
            MessageContentPart::Attachment(_) => None,
        })
        .collect::<String>();
    ArchiveProjection {
        text,
        parts,
        attachments,
    }
}
