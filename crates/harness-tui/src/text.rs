pub(crate) fn has_trimmed_content(value: &str) -> bool {
    non_empty_trimmed(value).is_some()
}

pub(crate) fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(crate) fn non_empty_preserved_string(value: &str) -> Option<String> {
    has_trimmed_content(value).then(|| value.to_string())
}

pub(crate) fn replace_control_chars_except_tabs(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() && character != '\t' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub(crate) fn trimmed_json_string_field(
    value: Option<&serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    let object = value?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(non_empty_trimmed)
            .map(str::to_string)
    })
}

pub(crate) fn trimmed_json_nested_string_field(
    value: Option<&serde_json::Value>,
    path: &[&str],
) -> Option<String> {
    let mut current = value?;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}
