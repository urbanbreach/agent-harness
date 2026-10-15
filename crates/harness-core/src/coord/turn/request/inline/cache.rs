//! Frame rendering behind the per-tool-call and single system caches.
use crate::attachment_transport::{AttachmentDimensions, AttachmentMetadata};
use crate::coord::context::archive::inline_cache::{FrameSink, RenderEntry, RenderKey, Rendered};
use harness_compaction::archive::{self, ArchiveShape, RenderedFrame};
use std::sync::Arc;

/// Most frames the system prompt may take.
pub(super) const SYSTEM_FRAME_CAP: u32 = 6;

/// Fingerprint of text built from byte fragments.
pub(super) fn fingerprint(fragments: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for fragment in fragments {
        hasher.update(&(fragment.len() as u64).to_le_bytes());
        hasher.update(fragment);
    }
    *hasher.finalize().as_bytes()
}

/// Stable short label of a tool call id for frame attachment ids.
pub(super) fn tool_label(id: &str) -> String {
    let hash = blake3::hash(id.as_bytes()).to_hex();
    format!("tool-{}", &hash.as_str()[..12])
}

/// Renders text with the model's resolved shape, through the frame sink when one is set
/// and locally otherwise.
pub(super) struct Renderer<'a> {
    pub shape: &'a ArchiveShape,
    pub sink: Option<&'a dyn FrameSink>,
}

impl Renderer<'_> {
    /// The cached render of `text`, rendering again when the text, the complete shape and
    /// price, the sink generation or the frame bound changed. `bound` is the most frames
    /// the caller can use; more frames is a cached "too many".
    pub(super) fn render(
        &self,
        slot: &mut Option<RenderEntry>,
        text_key: [u8; 32],
        text: impl FnOnce() -> String,
        bound: u32,
        label: &str,
    ) -> Rendered {
        let key = RenderKey {
            text: text_key,
            shape: *self.shape,
            generation: self.sink.map_or(0, FrameSink::generation),
            bound,
        };
        if let Some(entry) = slot.as_ref().filter(|entry| entry.key == key) {
            return entry.rendered.clone();
        }
        let rendered = self.rasterize(&text(), bound, label);
        *slot = Some(RenderEntry {
            key,
            rendered: rendered.clone(),
        });
        rendered
    }

    fn rasterize(&self, text: &str, bound: u32, label: &str) -> Rendered {
        let shape = self.shape;
        let limit = usize::try_from(bound).unwrap_or(usize::MAX);
        if self.sink.is_none() && shape.columns != 2 {
            // Pagination is exact for grid shapes: refuse before any pixel work.
            let normalized = archive::normalize_text(text, shape.font);
            if archive::paginate_grid(&normalized, shape).len() > limit {
                return Rendered::TooMany;
            }
        }
        let probe = Some(bound.saturating_add(1));
        let frames = self
            .sink
            .and_then(|sink| sink.rasterize(text, shape, probe))
            .map_or_else(|| archive::render_many(text, shape, probe).ok(), Some);
        let Some(frames) = frames else {
            return Rendered::Frames(Arc::from([]));
        };
        if frames.len() > limit {
            return Rendered::TooMany;
        }
        Rendered::Frames(
            frames
                .iter()
                .enumerate()
                .map(|(index, frame)| metadata(label, index + 1, frame, shape))
                .collect(),
        )
    }
}

fn metadata(
    label: &str,
    number: usize,
    frame: &RenderedFrame,
    shape: &ArchiveShape,
) -> AttachmentMetadata {
    let mut frame_metadata = AttachmentMetadata::from_bytes(
        format!("inline-{label}-{number}"),
        "image/png",
        None,
        &frame.png,
        Some(AttachmentDimensions::new(frame.width, frame.height)),
    );
    frame_metadata.detail = shape.detail.map(str::to_owned);
    frame_metadata
}

/// Frames of a render; none when it failed or was too large.
pub(super) fn frames_of(rendered: &Rendered) -> Option<&Arc<[AttachmentMetadata]>> {
    match rendered {
        Rendered::Frames(frames) if !frames.is_empty() => Some(frames),
        _ => None,
    }
}
