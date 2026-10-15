use pricing::square_price;

mod pricing;
mod resolve;
pub use pricing::frame_tokens;
pub(super) use resolve::{is_explicit, resolve_silver};
pub use resolve::{resolve_shape, resolve_shape_on, ShapeHost};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Black,
    Sentence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pricing {
    Unknown,
    AnthropicHigh,
    AnthropicOld,
    Google,
    OpenAi,
    OpenAiLimited,
    OpenAiUnlimited,
    Grok,
}

/// Frame geometry, ink and billing of one archive shape. `detail` is the image detail
/// hint carried to OpenAI-family wires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveShape {
    pub id: &'static str,
    pub font: &'static str,
    pub cell_width: u32,
    pub cell_height: u32,
    pub line_repeat: u32,
    pub edge: u32,
    pub frame_token_estimate: u32,
    pub verified: bool,
    pub ink: Ink,
    pub columns: u32,
    pub stopword_dim: bool,
    pub stretch: bool,
    pub detail: Option<&'static str>,
    pub(super) pricing: Pricing,
}

const fn shape(
    id: &'static str,
    font: &'static str,
    w: u32,
    h: u32,
    repeat: u32,
    ink: Ink,
    stretch: bool,
    columns: u32,
    dim: bool,
    edge: u32,
) -> ArchiveShape {
    ArchiveShape {
        id,
        font,
        cell_width: w,
        cell_height: h,
        line_repeat: repeat,
        edge,
        frame_token_estimate: 5024,
        verified: false,
        ink,
        columns,
        stopword_dim: dim,
        stretch,
        detail: None,
        pricing: Pricing::Unknown,
    }
}
const BASE: [ArchiveShape; 17] = [
    shape("8x8r-bw", "8x8", 8, 8, 2, Ink::Black, true, 1, false, 1568),
    shape(
        "8x8r-sent",
        "8x8",
        8,
        8,
        2,
        Ink::Sentence,
        true,
        1,
        false,
        1568,
    ),
    shape("8x8u-bw", "8x8", 8, 8, 1, Ink::Black, true, 1, false, 1568),
    shape(
        "8x8u-sent",
        "8x8",
        8,
        8,
        1,
        Ink::Sentence,
        true,
        1,
        false,
        1568,
    ),
    shape("6x6u-bw", "8x8", 6, 6, 1, Ink::Black, true, 1, false, 1568),
    shape(
        "6x6u-sent",
        "8x8",
        6,
        6,
        1,
        Ink::Sentence,
        true,
        1,
        false,
        1568,
    ),
    shape("5x8-bw", "5x8", 5, 8, 1, Ink::Black, true, 1, false, 2576),
    shape(
        "5x8-sent",
        "5x8",
        5,
        8,
        1,
        Ink::Sentence,
        true,
        1,
        false,
        2576,
    ),
    shape(
        "6x12-dim",
        "6x12",
        6,
        12,
        1,
        Ink::Black,
        true,
        1,
        true,
        1568,
    ),
    shape(
        "8x13-bw",
        "8x13",
        8,
        13,
        1,
        Ink::Black,
        true,
        1,
        false,
        1568,
    ),
    shape(
        "8on16-bw",
        "8x13",
        8,
        16,
        1,
        Ink::Black,
        false,
        1,
        false,
        1568,
    ),
    shape(
        "8on22-bw",
        "8x13",
        8,
        22,
        1,
        Ink::Black,
        false,
        1,
        false,
        1568,
    ),
    shape(
        "11on16-bw",
        "8x13",
        11,
        16,
        1,
        Ink::Black,
        false,
        1,
        false,
        1568,
    ),
    shape(
        "silver16-bw",
        "silver",
        16,
        16,
        1,
        Ink::Black,
        true,
        1,
        false,
        1568,
    ),
    shape(
        "doc-8on16-bw",
        "8x13",
        8,
        16,
        1,
        Ink::Black,
        false,
        2,
        false,
        1568,
    ),
    shape(
        "doc-8on16-sent",
        "8x13",
        8,
        16,
        1,
        Ink::Sentence,
        false,
        2,
        false,
        1568,
    ),
    shape(
        "doc-8on16-sent-dim",
        "8x13",
        8,
        16,
        1,
        Ink::Sentence,
        false,
        2,
        true,
        1568,
    ),
];
const PRICING: [Pricing; 8] = [
    Pricing::Unknown,
    Pricing::AnthropicHigh,
    Pricing::AnthropicOld,
    Pricing::Google,
    Pricing::OpenAi,
    Pricing::OpenAiLimited,
    Pricing::OpenAiUnlimited,
    Pricing::Grok,
];
const EDGES: [u32; 4] = [0, 1932, 2048, 1024];
const DETAIL: Option<&str> = Some("original");
/// Table size: shapes x host/model edges x billing lineages x verified x detail hint.
const COUNT: usize = 17 * 4 * 8 * 2 * 2;

/// The planned price of one square frame: the raw price when verified, never below the
/// conservative unverified price otherwise.
const fn planned(raw: u32, verified: bool) -> u32 {
    if verified || raw > 5024 {
        raw
    } else {
        5024
    }
}

const fn resolved() -> [ArchiveShape; COUNT] {
    let mut out = [BASE[0]; COUNT];
    let mut index = 0;
    while index < COUNT {
        let detail = index % 2;
        let verified = index / 2 % 2;
        let p = index / 4 % 8;
        let e = index / 32 % 4;
        let v = index / 128;
        let mut s = BASE[v];
        if e != 0 {
            s.edge = EDGES[e];
        }
        s.pricing = PRICING[p];
        s.verified = verified == 1;
        s.detail = if detail == 1 { DETAIL } else { None };
        s.frame_token_estimate = planned(square_price(s.edge, s.pricing), s.verified);
        out[index] = s;
        index += 1;
    }
    out
}
static RESOLVED: [ArchiveShape; COUNT] = resolved();

/// Index of `silver16-bw` in the base shape table.
pub(super) const SILVER: usize = 13;

/// The static shape for a table cell. `edge` indexes `EDGES` (0 keeps the variant's own).
pub(super) fn lookup(
    variant: usize,
    edge: usize,
    pricing: usize,
    verified: bool,
    detail: bool,
) -> &'static ArchiveShape {
    let index = (((variant * 4 + edge) * 8 + pricing) * 2 + usize::from(verified)) * 2
        + usize::from(detail);
    &RESOLVED[index.min(COUNT - 1)]
}

/// Whether `id` names a concrete variant.
pub(super) fn variant_index(id: &str) -> Option<usize> {
    BASE.iter().position(|s| s.id == id)
}

impl ArchiveShape {
    /// Price of one square frame at this edge by the billing lineage, before the
    /// conservative floor planning applies to unverified rules.
    pub fn raw_frame_tokens(&self) -> u32 {
        square_price(self.edge, self.pricing)
    }

    /// The same geometry at another edge, repriced with the lineage's resize rule.
    #[must_use]
    pub fn with_frame_size(mut self, edge: u32) -> Self {
        self.edge = edge;
        self.frame_token_estimate = planned(self.raw_frame_tokens(), self.verified);
        self
    }
}
