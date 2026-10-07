//! Ultrawork directive dedupe (senpi `prompt-directive-dedupe.ts`).
use super::*;

// ---- ultrawork directive dedupe ----

const ULTRAWORK_OPEN: &str = "<ultrawork-mode>";
const ULTRAWORK_CLOSE: &str = "</ultrawork-mode>";
pub(super) const SUPERSEDED_PLACEHOLDER: &str =
    "[ultrawork directive superseded; the latest ultrawork directive block below applies]";

fn has_nested_directive(blocks: &[Value]) -> bool {
    let mut depth = 0usize;
    for text in blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
    {
        let mut rest = text;
        loop {
            let open = rest.find(ULTRAWORK_OPEN);
            let close = rest.find(ULTRAWORK_CLOSE);
            match (open, close) {
                (Some(o), c) if c.is_none_or(|c| o < c) => {
                    depth += 1;
                    if depth > 1 {
                        return true;
                    }
                    rest = &rest[o + ULTRAWORK_OPEN.len()..];
                }
                (_, Some(c)) => {
                    depth = depth.saturating_sub(1);
                    rest = &rest[c + ULTRAWORK_CLOSE.len()..];
                }
                (None, None) | (Some(_), None) => break,
            }
        }
    }
    false
}

/// Non-overlapping `<ultrawork-mode>...</ultrawork-mode>` spans (non-greedy) in one block.
fn spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find(ULTRAWORK_OPEN) {
        let start = from + open;
        let Some(close) = text[start + ULTRAWORK_OPEN.len()..].find(ULTRAWORK_CLOSE) else {
            break;
        };
        let end = start + ULTRAWORK_OPEN.len() + close + ULTRAWORK_CLOSE.len();
        spans.push((start, end));
        from = end;
    }
    spans
}

pub struct DedupeResult {
    pub blocks: Vec<Value>,
    pub collapsed_directives: usize,
}

/// Keeps only the last ultrawork directive span; earlier ones become a placeholder.
pub fn dedupe_ultrawork_blocks(blocks: Vec<Value>) -> DedupeResult {
    if has_nested_directive(&blocks) {
        return DedupeResult {
            blocks,
            collapsed_directives: 0,
        };
    }
    let total: usize = blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .map(|t| spans(t).len())
        .sum();
    if total == 0 {
        return DedupeResult {
            blocks,
            collapsed_directives: 0,
        };
    }
    let mut remaining = total;
    let blocks = blocks
        .into_iter()
        .map(|block| {
            let Some(text) = block["text"].as_str().filter(|_| block["type"] == "text") else {
                return block;
            };
            let mut out = String::new();
            let mut last = 0;
            for (start, end) in spans(text) {
                remaining -= 1;
                out.push_str(&text[last..start]);
                out.push_str(if remaining == 0 {
                    &text[start..end]
                } else {
                    SUPERSEDED_PLACEHOLDER
                });
                last = end;
            }
            out.push_str(&text[last..]);
            text_block(out)
        })
        .collect();
    DedupeResult {
        blocks,
        collapsed_directives: total - 1,
    }
}

/// UTF-8 bytes of the serialized prompt's text blocks.
pub fn serialized_payload_bytes(blocks: &[Value]) -> usize {
    blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .map(str::len)
        .sum()
}
