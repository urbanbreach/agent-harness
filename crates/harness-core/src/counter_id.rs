pub(crate) fn parse_prefixed_counter(id: &str, expected_prefix: &str) -> Option<u64> {
    let tail = id.strip_prefix(expected_prefix)?;
    if tail.is_empty() {
        return None;
    }

    tail.parse::<u64>().ok()
}
