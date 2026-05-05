use crate::text::trimmed_non_empty;

pub(crate) fn first_env_value(keys: &[&str]) -> Option<String> {
    first_env_entry(keys).map(|(_, value)| value)
}

pub(crate) fn first_non_empty_env_value(keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|value| trimmed_non_empty(&value).map(str::to_string))
}

pub(crate) fn first_env_entry<'a>(keys: &'a [&str]) -> Option<(&'a str, String)> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok().map(|value| (*key, value)))
}
