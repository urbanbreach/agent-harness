//! Byte-pair counting for tiktoken-format rank tables (o200k_base and the Kimi vocabulary).
//!
//! A piece that is itself a rank costs one token; otherwise the lowest-ranked adjacent pair is
//! merged repeatedly, leftmost first on ties, until no pair is a rank. The merge uses a heap with
//! lazy invalidation so very long pieces stay near-linear.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rustc_hash::FxHashMap;

use super::pack::{decompress, Cursor};
use super::split::{Dialect, Pieces};

pub(super) struct Bpe {
    ranks: FxHashMap<Vec<u8>, u32>,
    dialect: Dialect,
}

/// Reusable buffers for one merge.
#[derive(Default)]
struct Scratch {
    next: Vec<u32>,
    prev: Vec<u32>,
    heap: BinaryHeap<Reverse<(u32, u32, u32)>>,
}

const DEAD: u32 = u32::MAX;

impl Bpe {
    /// Build from a bundled payload: a varint rank count, then each token's length and bytes in
    /// rank order. Every single byte must be a rank so that every piece can be tiled.
    pub(super) fn load(packed: &[u8], dialect: Dialect) -> Option<Self> {
        let payload = decompress(packed)?;
        let mut cursor = Cursor::new(&payload);
        let count = cursor.varint_len()?;
        let mut ranks: FxHashMap<Vec<u8>, u32> = FxHashMap::default();
        ranks.reserve(count);
        for rank in 0..count {
            let len = cursor.varint_len()?;
            ranks.insert(cursor.bytes(len)?.to_vec(), u32::try_from(rank).ok()?);
        }
        let complete = cursor.is_empty()
            && ranks.len() == count
            && (0..=255u8).all(|byte| ranks.contains_key([byte].as_slice()));
        complete.then_some(Self { ranks, dialect })
    }

    pub(super) fn count(&self, text: &str) -> Option<u32> {
        let mut scratch = Scratch::default();
        let mut total = 0usize;
        for piece in Pieces::new(text, self.dialect) {
            total += self.piece_tokens(piece.as_bytes(), &mut scratch);
        }
        u32::try_from(total).ok()
    }

    fn rank(&self, bytes: &[u8]) -> Option<u32> {
        self.ranks.get(bytes).copied()
    }

    fn piece_tokens(&self, piece: &[u8], scratch: &mut Scratch) -> usize {
        if piece.len() <= 1 || self.rank(piece).is_some() {
            return 1;
        }
        let Ok(len) = u32::try_from(piece.len()) else {
            // Pieces beyond u32 bytes cannot occur in practice; count them bytewise.
            return piece.len();
        };
        let Scratch { next, prev, heap } = scratch;
        next.clear();
        prev.clear();
        heap.clear();
        // Part `i` starts at byte `i` and ends where its `next` part starts; merged-away parts
        // are marked dead.
        next.extend(1..=len);
        prev.extend((0..len).map(|i| i.wrapping_sub(1)));
        for start in 0..len - 1 {
            if let Some(rank) = self.rank(span(piece, start, start + 2)) {
                heap.push(Reverse((rank, start, start + 2)));
            }
        }
        let mut parts = len;
        while let Some(Reverse((_, start, pair_end))) = heap.pop() {
            let second = next[start as usize];
            // Stale entries: the part died, or its pair span changed since the push.
            if second >= len || next[second as usize] != pair_end {
                continue;
            }
            // Merge `start` with its successor.
            next[start as usize] = pair_end;
            next[second as usize] = DEAD;
            parts -= 1;
            if pair_end < len {
                prev[pair_end as usize] = start;
                let after = next[pair_end as usize];
                if let Some(rank) = self.rank(span(piece, start, after)) {
                    heap.push(Reverse((rank, start, after)));
                }
            }
            if start > 0 {
                let before = prev[start as usize];
                if let Some(rank) = self.rank(span(piece, before, pair_end)) {
                    heap.push(Reverse((rank, before, pair_end)));
                }
            }
        }
        parts as usize
    }
}

fn span(piece: &[u8], start: u32, end: u32) -> &[u8] {
    &piece[start as usize..end as usize]
}
