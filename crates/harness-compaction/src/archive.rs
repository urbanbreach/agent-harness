//! Deterministic, source-backed history archives. Images and source are private
//! artifacts; callers own persistence, cancellation, and provider fit checks.
mod budget;
mod canvas;
mod fonts;
mod layout;
mod normalize;
mod pages;
mod plan;
mod policy;
mod probe;
mod render;
mod scope;
mod serialize;
mod shapes;
mod silver;
mod summary;

pub use budget::{byte_frame_cap, plan_frame_budget, rescue_frame_budget, FrameBudgetInput};
pub use harness_providers::attachment_protocol::{host_edge_limit, image_limits, ImageLimits};
pub use layout::{capacity, paginate_grid, render_many, wrap_doc};
pub use normalize::{elide_data_urls, font_coverage, normalize_text, FontCoverage};
pub use plan::build_archive;
pub use policy::ReaderPolicy;
pub use probe::{resolve_shape_for_text, resolve_shape_for_text_on};
pub use render::{render_frame, thumbnail};
pub use scope::{has_reasoning_legend, scoped_suffix, strip_reasoning};
pub use serialize::{serialize_for_archive, SerializeOptions};
pub use shapes::{frame_tokens, resolve_shape, resolve_shape_on, ArchiveShape, Ink, ShapeHost};
pub use summary::lead_in_with_files;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveSource {
    pub text: String,
    pub truncated_chars: u64,
}

#[derive(Debug)]
pub struct ArchivePlanInput {
    pub previous: Option<ArchiveSource>,
    pub new_text: String,
    pub frame_budget: u32,
    pub shape: ArchiveShape,
    pub max_frames: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFrame {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub chars: u64,
}

/// Nominal geometry and shape of one rendered frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMeta {
    /// Cell columns of one grid (one document column for two-column shapes).
    pub columns: u32,
    /// Unique-line rows, independent of the PNG height and repeat copies.
    pub rows: u32,
    pub shape: &'static str,
    pub font: &'static str,
    /// `sent` for sentence hues, `bw` for black.
    pub variant: &'static str,
    pub detail: Option<&'static str>,
}

#[derive(Debug)]
pub struct ArchiveOutput {
    pub text_head: String,
    pub text_tail: String,
    pub frames: Vec<RenderedFrame>,
    /// Metadata of each frame, aligned with `frames`.
    pub frame_meta: Vec<FrameMeta>,
    pub source: ArchiveSource,
    /// Framed characters plus both text edges (UTF-16 length).
    pub total_chars: u64,
    pub short_summary: String,
    pub lead_in: String,
}

impl ArchiveOutput {
    pub(crate) fn empty(truncated_chars: u64) -> Self {
        Self {
            text_head: String::new(),
            text_tail: String::new(),
            frames: Vec::new(),
            frame_meta: Vec::new(),
            source: ArchiveSource {
                text: String::new(),
                truncated_chars,
            },
            total_chars: 0,
            short_summary: String::new(),
            lead_in: String::new(),
        }
    }

    /// An archive of text edges only, with its guide: the fail-closed shape when old
    /// pixels cannot be reused.
    pub fn text_only(text_head: String, text_tail: String, truncated_chars: u64) -> Self {
        let mut output = Self::empty(truncated_chars);
        output.source.text = match (text_head.is_empty(), text_tail.is_empty()) {
            (false, false) => format!("{text_head}\u{2588}{text_tail}"),
            (false, true) => text_head.clone(),
            _ => text_tail.clone(),
        };
        output.text_head = text_head;
        output.text_tail = text_tail;
        output.total_chars = utf16_chars(&output.text_head) + utf16_chars(&output.text_tail);
        let shape = resolve_shape("", "", None);
        output.lead_in = summary::lead_in(&output, shape, &[], 0);
        output
    }
}

fn utf16_chars(text: &str) -> u64 {
    u64::try_from(text.encode_utf16().count()).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u16,
    pub height: u16,
    pub bits: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("Invalid archive frame geometry")]
    InvalidGeometry,
    #[error("Archive frame budget must be positive")]
    EmptyFrameBudget,
    #[error("Archive PNG encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests;
