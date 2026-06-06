use crate::keybindings;

pub(super) fn slash_command_match_rank(
    command: &str,
    description: &str,
    query: &str,
) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }

    let display = format!("/{command}");
    let command = command.to_lowercase();
    let description = description.to_lowercase();
    let aliases = keybindings::slash_command_aliases(command.as_str());

    if command == query || display == query || aliases.contains(&query) {
        return Some((0, 0));
    }

    if command.starts_with(query)
        || display.starts_with(query)
        || aliases.iter().any(|alias| alias.starts_with(query))
    {
        return Some((0, command.len().saturating_sub(query.len())));
    }

    if let Some(index) = command.find(query).or_else(|| display.find(query)) {
        return Some((1, index));
    }

    if let Some(index) = aliases.iter().find_map(|alias| alias.find(query)) {
        return Some((1, index));
    }

    if let Some(score) = slash_subsequence_score(&command, query)
        .or_else(|| slash_subsequence_score(&display, query))
        .or_else(|| {
            aliases
                .iter()
                .filter_map(|alias| slash_subsequence_score(alias, query))
                .min()
        })
    {
        return Some((2, score));
    }

    description.find(query).map(|index| (3, index))
}

pub(super) fn slash_command_display_width(command: &str) -> usize {
    command.chars().count().saturating_add(1)
}

pub(super) fn auth_slash_args_from_prompt(prompt: &str) -> Vec<String> {
    let trimmed = prompt.trim().trim_start_matches('/');
    let mut parts = trimmed.split_whitespace();
    match parts.next() {
        Some("login") => std::iter::once("login".to_string())
            .chain(parts.map(str::to_string))
            .collect(),
        Some("auth") => {
            let args = parts.map(str::to_string).collect::<Vec<_>>();
            if args.is_empty() {
                vec!["list".to_string()]
            } else {
                args
            }
        }
        _ => vec!["list".to_string()],
    }
}

fn slash_subsequence_score(haystack: &str, needle: &str) -> Option<usize> {
    let mut total_gap = 0usize;
    let mut last_index = 0usize;

    for ch in needle.chars() {
        let next = haystack[last_index..].find(ch)?;
        total_gap = total_gap.saturating_add(next);
        last_index = last_index
            .saturating_add(next)
            .saturating_add(ch.len_utf8());
    }

    Some(total_gap)
}
