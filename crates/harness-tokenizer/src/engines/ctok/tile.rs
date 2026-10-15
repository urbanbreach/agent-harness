//! Minimum-cost tiling of the marked stream over the measured vocabulary.
//!
//! Every vocabulary piece and every byte-prefix token costs one token, so the count is the
//! number of tiles. Characters no piece covers fall back to a minimum tiling of their UTF-8 bytes.

use rustc_hash::{FxHashMap, FxHashSet};

use super::chars::{is_marker, unescape_marker};

/// Reverse trie: pieces are stored backwards because the DP ends a tile at each position and scans
/// candidate starts right to left.
struct ReverseTrie {
    edges: FxHashMap<u64, u32>,
    terminal: Vec<bool>,
}

impl ReverseTrie {
    fn new() -> Self {
        Self {
            edges: FxHashMap::default(),
            terminal: vec![false],
        }
    }

    fn key(node: u32, ch: char) -> u64 {
        u64::from(node) << 32 | u64::from(u32::from(ch))
    }

    fn insert(&mut self, piece: &str) -> Option<()> {
        let mut node = 0u32;
        for ch in piece.chars().rev() {
            let next = u32::try_from(self.terminal.len()).ok()?;
            let child = *self.edges.entry(Self::key(node, ch)).or_insert(next);
            if child == next {
                self.terminal.push(false);
            }
            node = child;
        }
        *self.terminal.get_mut(usize::try_from(node).ok()?)? = true;
        Some(())
    }

    fn child(&self, node: u32, ch: char) -> Option<u32> {
        self.edges.get(&Self::key(node, ch)).copied()
    }

    fn is_terminal(&self, node: u32) -> bool {
        self.terminal.get(node as usize).copied().unwrap_or(false)
    }
}

/// What a codepoint costs when no piece covers it: a minimum tiling of its UTF-8 bytes over the
/// byte-prefix tokens, every single byte costing one.
struct ByteFloor {
    tokens: FxHashSet<Vec<u8>>,
    max_len: usize,
}

impl ByteFloor {
    fn cost(&self, bytes: &[u8]) -> u32 {
        let mut best = vec![0u32; bytes.len() + 1];
        for end in 1..=bytes.len() {
            let mut cheapest = u32::MAX;
            for start in end.saturating_sub(self.max_len)..end {
                let allowed = end - start == 1 || self.tokens.contains(&bytes[start..end]);
                if allowed {
                    cheapest = cheapest.min(best[start] + 1);
                }
            }
            best[end] = cheapest;
        }
        best[bytes.len()]
    }
}

pub(super) struct Vocab {
    trie: ReverseTrie,
    units: FxHashSet<char>,
    bytes: ByteFloor,
}

impl Vocab {
    /// `pieces` are marked-stream spellings, `contractions` are the apostrophe suffixes whose glued
    /// spelling (`'t` followed by the end-of-word marker) is also a piece, and `byte_tokens` are the
    /// byte-prefix tokens as hex strings.
    pub(super) fn new(
        pieces: &[String],
        contractions: &[String],
        byte_tokens: &[String],
    ) -> Option<Self> {
        let mut trie = ReverseTrie::new();
        let mut units = FxHashSet::default();
        for piece in pieces {
            trie.insert(piece)?;
            let mut chars = piece.chars();
            if let (Some(only), None) = (chars.next(), chars.next())
                && !is_marker(only)
            {
                units.insert(only);
            }
        }
        for contraction in contractions {
            trie.insert(&format!("{contraction}{}", super::chars::EOW))?;
        }
        let mut tokens = FxHashSet::default();
        for hex in byte_tokens {
            tokens.insert(decode_hex(hex)?);
        }
        let mut buf = [0u8; 4];
        for &unit in &units {
            tokens.insert(unit.encode_utf8(&mut buf).as_bytes().to_vec());
        }
        let max_len = tokens.iter().map(Vec::len).max()?;
        Some(Self {
            trie,
            units,
            bytes: ByteFloor { tokens, max_len },
        })
    }

    /// One codepoint standing alone: 1 if it is itself a piece, else its byte-floor tiling.
    fn char_cost(&self, ch: char) -> u32 {
        let ch = unescape_marker(ch);
        if self.units.contains(&ch) {
            1
        } else {
            let mut buf = [0u8; 4];
            self.bytes.cost(ch.encode_utf8(&mut buf).as_bytes())
        }
    }

    /// Minimum token cost of tiling `text`; `unit_cost` prices a character no piece covers.
    fn tile_with(&self, text: &[char], unit_cost: impl Fn(char) -> u32) -> u32 {
        let mut best = vec![0u32; text.len() + 1];
        for end in 1..=text.len() {
            let first = self.trie.child(0, text[end - 1]);
            let single = match first {
                Some(node) if self.trie.is_terminal(node) => 1,
                _ => unit_cost(text[end - 1]),
            };
            best[end] = best[end - 1] + single;
            let Some(mut node) = first else {
                continue;
            };
            for start in (0..end - 1).rev() {
                let Some(next) = self.trie.child(node, text[start]) else {
                    break;
                };
                node = next;
                if self.trie.is_terminal(node) {
                    best[end] = best[end].min(best[start] + 1);
                }
            }
        }
        best[text.len()]
    }

    /// Token count of a marked stream: a marker no piece absorbs tiles as itself.
    pub(super) fn tile(&self, stream: &[char]) -> u32 {
        self.tile_with(
            stream,
            |ch| if is_marker(ch) { 1 } else { self.char_cost(ch) },
        )
    }

    /// Token count of a run of newlines, every newline being a unit.
    pub(super) fn tile_newlines(&self, len: usize) -> u32 {
        self.tile_with(&vec!['\n'; len], |_| 1)
    }
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}
