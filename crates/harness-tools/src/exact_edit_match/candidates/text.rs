pub(super) fn middle_line_similarity(
    content_lines: &[&str],
    search_lines: &[&str],
    start: usize,
    end: usize,
) -> f64 {
    let actual_middle = end.saturating_sub(start + 1);
    let search_middle = search_lines.len().saturating_sub(2);
    let lines_to_check = actual_middle.min(search_middle);
    if lines_to_check == 0 {
        return 1.0;
    }

    let total = (0..lines_to_check)
        .map(|offset| {
            line_similarity(
                content_lines[start + 1 + offset].trim(),
                search_lines[1 + offset].trim(),
            )
        })
        .sum::<f64>();
    total / f64::from(u32::try_from(lines_to_check).unwrap_or(u32::MAX))
}

pub(super) fn trimmed_middle_similarity(block_lines: &[&str], search_lines: &[&str]) -> f64 {
    let mut total = 0usize;
    let mut matched = 0usize;
    for index in 1..block_lines.len().saturating_sub(1) {
        let block = block_lines[index].trim();
        let search = search_lines[index].trim();
        if block.is_empty() && search.is_empty() {
            continue;
        }
        total += 1;
        if block == search {
            matched += 1;
        }
    }
    if total == 0 {
        1.0
    } else {
        f64::from(u32::try_from(matched).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(total).unwrap_or(u32::MAX))
    }
}

fn line_similarity(left: &str, right: &str) -> f64 {
    let max_len = left.chars().count().max(right.chars().count());
    if max_len == 0 {
        return 1.0;
    }
    1.0 - f64::from(u32::try_from(levenshtein(left, right)).unwrap_or(u32::MAX))
        / f64::from(u32::try_from(max_len).unwrap_or(u32::MAX))
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let cost = usize::from(left_char != *right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_chars.len()]
}

pub(super) fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn remove_shared_indentation(text: &str) -> String {
    let lines = text.split('\n').collect::<Vec<_>>();
    let Some(min_indent) = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
    else {
        return text.to_string();
    };
    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                (*line).to_string()
            } else {
                line.chars().skip(min_indent).collect()
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

pub(super) fn unescape_search_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            result.push(ch);
            break;
        };
        match next {
            'n' => result.push('\n'),
            't' => result.push('\t'),
            'r' => result.push('\r'),
            '\'' => result.push('\''),
            '"' => result.push('"'),
            '`' => result.push('`'),
            '\\' => result.push('\\'),
            '$' => result.push('$'),
            other => {
                result.push('\\');
                result.push(other);
            }
        }
    }
    result
}

pub(super) fn split_search_lines(text: &str) -> Vec<&str> {
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

pub(super) fn split_match_lines(text: &str) -> Vec<&str> {
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}
