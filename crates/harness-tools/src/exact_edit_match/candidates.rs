mod text;

use text::{
    middle_line_similarity, normalize_whitespace, remove_shared_indentation, split_match_lines,
    split_search_lines, trimmed_middle_similarity, unescape_search_text,
};

pub(super) fn replacement_candidates(content: &str, find: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_candidate(&mut candidates, find.to_string());
    append_line_trimmed_candidates(&mut candidates, content, find);
    append_block_anchor_candidates(&mut candidates, content, find);
    append_whitespace_normalized_candidates(&mut candidates, content, find);
    append_indentation_flexible_candidates(&mut candidates, content, find);
    append_escape_normalized_candidates(&mut candidates, content, find);
    append_trimmed_boundary_candidates(&mut candidates, content, find);
    append_context_aware_candidates(&mut candidates, content, find);
    candidates
}

fn append_line_trimmed_candidates(candidates: &mut Vec<String>, content: &str, find: &str) {
    let content_lines = split_match_lines(content);
    let search_lines = split_search_lines(find);
    if search_lines.is_empty() || content_lines.len() < search_lines.len() {
        return;
    }

    for start in 0..=content_lines.len() - search_lines.len() {
        if search_lines
            .iter()
            .enumerate()
            .all(|(offset, line)| content_lines[start + offset].trim() == line.trim())
        {
            push_candidate(
                candidates,
                content_lines[start..start + search_lines.len()].join("\n"),
            );
        }
    }
}

fn append_block_anchor_candidates(candidates: &mut Vec<String>, content: &str, find: &str) {
    let content_lines = split_match_lines(content);
    let search_lines = split_search_lines(find);
    if search_lines.len() < 3 {
        return;
    }

    let first = search_lines[0].trim();
    let last = search_lines[search_lines.len() - 1].trim();
    let max_line_delta = std::cmp::max(1, search_lines.len() / 4);
    let mut best: Option<(usize, usize, f64)> = None;

    for start in 0..content_lines.len() {
        if content_lines[start].trim() != first {
            continue;
        }
        for end in start + 2..content_lines.len() {
            if content_lines[end].trim() != last {
                continue;
            }
            let actual = end - start + 1;
            if actual.abs_diff(search_lines.len()) > max_line_delta {
                break;
            }
            let similarity = middle_line_similarity(&content_lines, &search_lines, start, end);
            if similarity >= 0.65 && best.is_none_or(|(_, _, best_score)| similarity > best_score) {
                best = Some((start, end, similarity));
            }
            break;
        }
    }

    if let Some((start, end, _)) = best {
        push_candidate(candidates, content_lines[start..=end].join("\n"));
    }
}

fn append_whitespace_normalized_candidates(
    candidates: &mut Vec<String>,
    content: &str,
    find: &str,
) {
    let normalized_find = normalize_whitespace(find);
    let words = find.split_whitespace().collect::<Vec<_>>();
    let content_lines = split_match_lines(content);

    for line in &content_lines {
        let normalized_line = normalize_whitespace(line);
        if normalized_line == normalized_find {
            push_candidate(candidates, (*line).to_string());
        }
        if normalized_line.contains(&normalized_find) && !words.is_empty() {
            append_whitespace_substring_candidate(candidates, line, &words);
        }
    }

    let search_lines = split_search_lines(find);
    if search_lines.len() <= 1 || content_lines.len() < search_lines.len() {
        return;
    }
    for start in 0..=content_lines.len() - search_lines.len() {
        let block = content_lines[start..start + search_lines.len()].join("\n");
        if normalize_whitespace(&block) == normalized_find {
            push_candidate(candidates, block);
        }
    }
}

fn append_indentation_flexible_candidates(candidates: &mut Vec<String>, content: &str, find: &str) {
    let content_lines = split_match_lines(content);
    let search_lines = split_search_lines(find);
    if search_lines.is_empty() || content_lines.len() < search_lines.len() {
        return;
    }
    let normalized_find = remove_shared_indentation(&search_lines.join("\n"));

    for start in 0..=content_lines.len() - search_lines.len() {
        let block = content_lines[start..start + search_lines.len()].join("\n");
        if remove_shared_indentation(&block) == normalized_find {
            push_candidate(candidates, block);
        }
    }
}

fn append_escape_normalized_candidates(candidates: &mut Vec<String>, content: &str, find: &str) {
    let unescaped_find = unescape_search_text(find);
    if content.contains(&unescaped_find) {
        push_candidate(candidates, unescaped_find.clone());
    }

    let content_lines = split_match_lines(content);
    let search_lines = split_search_lines(&unescaped_find);
    if search_lines.is_empty() || content_lines.len() < search_lines.len() {
        return;
    }
    for start in 0..=content_lines.len() - search_lines.len() {
        let block = content_lines[start..start + search_lines.len()].join("\n");
        if unescape_search_text(&block) == unescaped_find {
            push_candidate(candidates, block);
        }
    }
}

fn append_trimmed_boundary_candidates(candidates: &mut Vec<String>, content: &str, find: &str) {
    let trimmed = find.trim();
    if trimmed == find {
        return;
    }
    if content.contains(trimmed) {
        push_candidate(candidates, trimmed.to_string());
    }

    let content_lines = split_match_lines(content);
    let search_lines = split_search_lines(find);
    if search_lines.is_empty() || content_lines.len() < search_lines.len() {
        return;
    }
    for start in 0..=content_lines.len() - search_lines.len() {
        let block = content_lines[start..start + search_lines.len()].join("\n");
        if block.trim() == trimmed {
            push_candidate(candidates, block);
        }
    }
}

fn append_context_aware_candidates(candidates: &mut Vec<String>, content: &str, find: &str) {
    let content_lines = split_match_lines(content);
    let search_lines = split_search_lines(find);
    if search_lines.len() < 3 {
        return;
    }
    let first = search_lines[0].trim();
    let last = search_lines[search_lines.len() - 1].trim();

    for start in 0..content_lines.len() {
        if content_lines[start].trim() != first {
            continue;
        }
        for end in start + 2..content_lines.len() {
            if content_lines[end].trim() != last {
                continue;
            }
            let block_lines = &content_lines[start..=end];
            if block_lines.len() == search_lines.len()
                && trimmed_middle_similarity(block_lines, &search_lines) >= 0.5
            {
                push_candidate(candidates, block_lines.join("\n"));
            }
            break;
        }
    }
}

fn append_whitespace_substring_candidate(candidates: &mut Vec<String>, line: &str, words: &[&str]) {
    let Some(first_word) = words.first() else {
        return;
    };
    for (start, _) in line.match_indices(first_word) {
        let mut cursor = start + first_word.len();
        let mut end = cursor;
        let mut matched = true;
        for word in &words[1..] {
            let whitespace_len = line[cursor..]
                .chars()
                .take_while(|ch| ch.is_whitespace())
                .map(char::len_utf8)
                .sum::<usize>();
            if whitespace_len == 0 {
                matched = false;
                break;
            }
            cursor += whitespace_len;
            if !line[cursor..].starts_with(word) {
                matched = false;
                break;
            }
            cursor += word.len();
            end = cursor;
        }
        if matched {
            push_candidate(candidates, line[start..end].to_string());
            return;
        }
    }
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidate.is_empty() && !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}
