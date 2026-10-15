//! Claude token-count reconstruction: an independent engine for the measured count-only
//! vocabularies of the public ctok project (MIT, revision pinned in `data/manifest.json`).
//!
//! Counting is: normalize, rewrite into a marked stream (word, case and byte markers), then take
//! a minimum-cost tiling over the measured pieces plus UTF-8 byte fallback. Boundary rules between
//! the message frame and the content differ per generation, see [`Frame`]. The result is a count
//! of content only; the fixed message frame the provider adds is not included.

mod chars;
mod stream;
mod tile;

use serde::Deserialize;

use super::pack::decompress;
use chars::is_ascii_frame_space;

/// How a generation's message frame meets the content edges.
pub(super) struct Frame {
    fold_quotes: bool,
    allcaps_min: Option<usize>,
    /// The frame ends in a word-start marker that absorbs one leading space.
    frame_bow: bool,
    tail: Tail,
}

enum Tail {
    /// The frame absorbs the trailing newline run as far as one token reaches. `appended` newlines
    /// belong to the frame itself.
    Ladder { appended: usize },
    /// The frame absorbs all trailing raw ASCII whitespace.
    Free,
}

impl Frame {
    pub(super) const V3: Self = Self {
        fold_quotes: true,
        allcaps_min: Some(4),
        frame_bow: true,
        tail: Tail::Ladder { appended: 2 },
    };
    pub(super) const V4_7: Self = Self {
        fold_quotes: false,
        allcaps_min: None,
        frame_bow: true,
        tail: Tail::Ladder { appended: 2 },
    };
    /// Opus 5: v4.7 vocabulary, no word-start in the frame, trailing ASCII whitespace is free.
    pub(super) const V5: Self = Self {
        fold_quotes: false,
        allcaps_min: None,
        frame_bow: false,
        tail: Tail::Free,
    };
    /// Sonnet/fable 5: no word-start in the frame, but the trailing newline ladder remains with no
    /// frame newlines of its own.
    pub(super) const V5_SONNET: Self = Self {
        fold_quotes: false,
        allcaps_min: None,
        frame_bow: false,
        tail: Tail::Ladder { appended: 0 },
    };

    const fn strips_newlines_here(&self) -> bool {
        matches!(self.tail, Tail::Ladder { .. })
    }
}

#[derive(Deserialize)]
struct Payload {
    pieces: Vec<String>,
    contractions: Vec<String>,
    bytes: Vec<String>,
}

pub(super) struct Model {
    vocab: tile::Vocab,
}

impl Model {
    pub(super) fn load(packed: &[u8]) -> Option<Self> {
        let payload: Payload = serde_json::from_slice(&decompress(packed)?).ok()?;
        let vocab = tile::Vocab::new(&payload.pieces, &payload.contractions, &payload.bytes)?;
        Some(Self { vocab })
    }

    /// Content token count of `text` under `frame`.
    pub(super) fn count(&self, text: &str, frame: &Frame) -> u32 {
        let (norm, trailing_newlines) = match frame.tail {
            Tail::Ladder { .. } => {
                let norm = stream::normalize(text, frame.fold_quotes);
                let newlines = norm.len() - norm.trim_end_matches('\n').len();
                (norm, newlines)
            }
            // The frame absorbs raw ASCII whitespace, so strip before normalization: a
            // non-ASCII space folded to U+0020 is not free at the end.
            Tail::Free => (
                stream::normalize(
                    text.trim_end_matches(is_ascii_frame_space),
                    frame.fold_quotes,
                ),
                0,
            ),
        };
        let stream = stream::stream(&norm, frame, text.starts_with(' '));
        let tail = self.frame_tail(trailing_newlines, frame);
        if stream.is_empty() {
            return tail;
        }
        self.vocab.tile(&stream) + tail
    }

    /// What a content-final run of `n` frame-absorbed newlines costs beyond the frame's own
    /// trailing token. It is not monotonic in `n`: one token can span content and frame newlines.
    fn frame_tail(&self, n: usize, frame: &Frame) -> u32 {
        match frame.tail {
            Tail::Ladder { appended } if n > 0 => {
                self.vocab.tile_newlines(n + appended).saturating_sub(1)
            }
            _ => 0,
        }
    }
}
