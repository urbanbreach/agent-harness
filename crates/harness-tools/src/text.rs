pub(crate) fn trimmed_non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(crate) fn has_trimmed_content(value: &str) -> bool {
    trimmed_non_empty(value).is_some()
}
