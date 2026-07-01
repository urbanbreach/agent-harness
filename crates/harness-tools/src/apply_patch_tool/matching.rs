pub(super) fn seek(
    lines: &[String],
    pattern: &[String],
    start: usize,
    end_of_file: bool,
) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > lines.len() {
        return None;
    }
    if end_of_file {
        let offset = lines.len().saturating_sub(pattern.len());
        if offset >= start && matches_pattern(lines, pattern, offset) {
            return Some(offset);
        }
    }
    (start..=lines.len() - pattern.len()).find(|offset| matches_pattern(lines, pattern, *offset))
}

fn matches_pattern(lines: &[String], pattern: &[String], offset: usize) -> bool {
    pattern.iter().enumerate().all(|(index, expected)| {
        let actual = &lines[offset + index];
        actual == expected
            || actual.trim_end() == expected.trim_end()
            || actual.trim() == expected.trim()
            || normalize(actual.trim()) == normalize(expected.trim())
    })
}

fn normalize(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '‘' | '’' | '‚' | '‛' => normalized.push('\''),
            '“' | '”' | '„' | '‟' => normalized.push('"'),
            '‐' | '‑' | '‒' | '–' | '—' | '―' => normalized.push('-'),
            '…' => normalized.push_str("..."),
            '\u{a0}' => normalized.push(' '),
            _ => normalized.push(ch),
        }
    }
    normalized
}
