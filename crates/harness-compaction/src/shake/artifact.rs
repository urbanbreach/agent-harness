//! Recovery artifact layout, placeholders and the edits that apply them.
use super::ShakeRegion;
use harness_tokenizer::Counter;

/// One document holding every region's original text, in region order.
pub fn artifact_text(regions: &[ShakeRegion]) -> String {
    regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            let (tokens, original, label) = region.data();
            format!(
                "### region {} ({label}, ~{tokens} tok)\n\n{original}\n",
                index + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn placeholder(tokens: u32, artifact: Option<&str>, region: usize) -> String {
    artifact.map_or_else(
        || format!("[shaken ~{tokens} tokens]"),
        |id| {
            format!("[shaken ~{tokens} tokens \u{2014} recover: artifact://{id} (region {region})]")
        },
    )
}

/// A tool result whose text becomes a placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolEdit {
    pub index: usize,
    pub replacement: String,
    /// Tokens the result's text held before.
    pub original_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    pub replacement: String,
    /// Tokens the edit removes from the entry's text.
    pub original_tokens: u32,
}

/// Tool-result replacements and text-range splices for `regions`. Splices are ordered so
/// applying them in order never shifts a later edit's offsets (highest start first per entry).
pub fn plan_edits(
    regions: &[ShakeRegion],
    artifact: Option<&str>,
) -> (Vec<ToolEdit>, Vec<TextEdit>) {
    let mut tools = Vec::new();
    let mut blocks = Vec::new();
    for (ordinal, region) in regions.iter().enumerate() {
        let replacement = placeholder(region.tokens(), artifact, ordinal + 1);
        match region {
            ShakeRegion::ToolResult { index, tokens, .. } => tools.push(ToolEdit {
                index: *index,
                replacement,
                original_tokens: *tokens,
            }),
            ShakeRegion::Block {
                index,
                start,
                end,
                tokens,
                ..
            } => blocks.push(TextEdit {
                index: *index,
                start: *start,
                end: *end,
                replacement,
                original_tokens: *tokens,
            }),
        }
    }
    blocks.sort_by_key(|edit| (edit.index, std::cmp::Reverse(edit.start)));
    (tools, blocks)
}

/// Tokens the regions save once replaced by their real placeholders: the exact figure a
/// tool-results-only pass gates on. `artifact` is the recovery id the placeholders cite.
pub fn exact_savings(regions: &[ShakeRegion], artifact: Option<&str>, counter: &Counter) -> u32 {
    regions
        .iter()
        .enumerate()
        .fold(0_u32, |sum, (ordinal, region)| {
            let replacement = placeholder(region.tokens(), artifact, ordinal + 1);
            sum.saturating_add(
                region
                    .tokens()
                    .saturating_sub(counter.count_text(&replacement)),
            )
        })
}
