/// Estimates text tokens with the canonical compaction heuristic.
///
/// This matches the established UTF-8 byte-length ceiling behavior.
pub fn estimate_text_tokens(text: &str) -> u32 {
    let byte_len = u32::try_from(text.len()).unwrap_or(u32::MAX);
    byte_len.div_ceil(4)
}
