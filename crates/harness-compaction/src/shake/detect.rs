//! Shake region detection: heavy tool results, fenced blocks and tagged blocks.
use super::{ShakeConfig, ShakeRegion, PLACEHOLDER_ESTIMATE};
use crate::{
    item::{Item, ItemKind, ItemPart},
    prune::{protects, Calls, ToolMatcher},
};
use harness_tokenizer::Counter;

fn tag(line: &str, closing: bool) -> Option<&str> {
    let body = line
        .strip_prefix(if closing { "</" } else { "<" })?
        .strip_suffix('>')?;
    let end = body.find(char::is_whitespace).unwrap_or(body.len());
    let name = &body[..end];
    if name.is_empty()
        || body.contains('>')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || b"_-".contains(&byte))
        || closing && end != body.len()
    {
        return None;
    }
    Some(name)
}
fn ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut fence = None;
    let mut stack = Vec::new();
    let mut outer = 0;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let line = raw
            .strip_suffix('\n')
            .unwrap_or(raw)
            .strip_suffix('\r')
            .unwrap_or(raw.strip_suffix('\n').unwrap_or(raw));
        let end = offset + line.len();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if let Some(start) = fence.take() {
                ranges.push((start, end));
            } else {
                fence = Some(offset);
            }
        } else if fence.is_none() {
            if let Some(name) = tag(line, false) {
                if stack.is_empty() {
                    outer = offset;
                }
                stack.push(name);
            } else if let Some(name) = tag(trimmed, true)
                && stack.last().is_some_and(|top| *top == name)
            {
                stack.pop();
                if stack.is_empty() {
                    ranges.push((outer, end));
                }
            }
        }
        offset += raw.len();
    }
    ranges.sort_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
    let mut last_end = 0;
    ranges.retain(|&(start, end)| {
        if start < last_end {
            false
        } else {
            last_end = end;
            true
        }
    });
    ranges
}
/// Regions to elide and the savings detection estimated for them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShakeDetection {
    pub regions: Vec<ShakeRegion>,
    /// Sum over regions of `tokens - PLACEHOLDER_ESTIMATE`, floored at zero.
    pub savings: u32,
}

fn estimated_savings(regions: &[ShakeRegion]) -> u32 {
    regions.iter().fold(0_u32, |sum, region| {
        sum.saturating_add(region.tokens().saturating_sub(PLACEHOLDER_ESTIMATE))
    })
}

/// Text blocks of an entry as `(offset, text)` slices of its flat content. Ordered parts are
/// scanned one block at a time when they reassemble the content, so no region spans blocks.
fn blocks<'a>(item: &'a Item<'a>) -> Vec<(usize, &'a str)> {
    let content = item.text();
    let mut offset = 0;
    let mut slices = Vec::new();
    for part in item.parts {
        if let ItemPart::Text(text) = part {
            if !content[offset..].starts_with(text.as_ref()) {
                return vec![(0, content)];
            }
            slices.push((offset, text.as_ref()));
            offset += text.len();
        }
    }
    if slices.is_empty() || offset != content.len() {
        return vec![(0, content)];
    }
    slices
}

fn label(item: &Item<'_>) -> String {
    if item.kind == ItemKind::Note {
        return item.note_kind.map_or_else(
            || "custom".into(),
            |kind| format!("{kind:?}").to_lowercase(),
        );
    }
    format!("{:?}", item.role()).to_lowercase()
}

/// Fenced and tagged blocks of one entry that are big enough to elide, in reverse document
/// order (the caller reverses the whole list).
fn block_regions(
    index: usize,
    item: &Item<'_>,
    cfg: &ShakeConfig,
    counter: &Counter,
) -> Vec<ShakeRegion> {
    let mut found = Vec::new();
    for (base, text) in blocks(item).into_iter().rev() {
        for (start, end) in ranges(text).into_iter().rev() {
            let original = &text[start..end];
            let tokens = counter.count_text(original);
            if tokens < cfg.fence_min_tokens {
                continue;
            }
            found.push(ShakeRegion::Block {
                index,
                start: base + start,
                end: base + end,
                tokens,
                original: original.to_owned(),
                label: label(item),
            });
        }
    }
    found
}

/// Finds elision candidates from `boundary` on.
///
/// Entries within `protect_tokens` of the end are skipped, except non-error useless results.
/// Tool results are eligible whole; other entries contribute fenced and tagged blocks of at
/// least `fence_min_tokens`. Nothing is returned unless the estimated savings reach
/// `min_savings`. Region sizes are priced with `counter`.
pub fn detect_regions(
    items: &[Item<'_>],
    boundary: usize,
    cfg: &ShakeConfig,
    protected: &[ToolMatcher],
    counter: &Counter,
) -> ShakeDetection {
    let calls = Calls::whole(items);
    let mut suffix = 0_u32;
    let mut regions = Vec::new();
    for index in (boundary.min(items.len())..items.len()).rev() {
        let item = &items[index];
        let bypass = item.is_tool_result() && item.tool.useless && !item.tool.is_error;
        let eligible = suffix >= cfg.protect_tokens || bypass;
        if !matches!(item.kind, ItemKind::Summary | ItemKind::BranchSummary) {
            suffix = suffix.saturating_add(item.tokens);
        }
        if !eligible {
            continue;
        }
        if item.is_tool_result() {
            if !item.tool.pruned
                && !protects(protected, item, calls.paired(item))
                && !item.text().is_empty()
            {
                regions.push(ShakeRegion::ToolResult {
                    index,
                    tokens: counter.count_text(item.text()),
                    original: item.text().to_owned(),
                    label: item.message.name.clone().unwrap_or_else(|| "tool".into()),
                });
            }
        } else if !matches!(item.kind, ItemKind::Summary | ItemKind::BranchSummary) {
            regions.extend(block_regions(index, item, cfg, counter));
        }
    }
    regions.reverse();
    let savings = estimated_savings(&regions);
    if savings < cfg.min_savings {
        return ShakeDetection::default();
    }
    ShakeDetection { regions, savings }
}
