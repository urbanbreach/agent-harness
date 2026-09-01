pub(super) fn parse_table_row(row: &str) -> Option<Vec<String>> {
    let trimmed = row.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let cells = split_unescaped_pipes(trimmed.trim_matches('|'))
        .into_iter()
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();
    (cells.len() >= 2).then_some(cells)
}

pub(super) fn is_table_separator_row(row: &str, column_count: usize) -> bool {
    let Some(cells) = parse_table_row(row) else {
        return false;
    };
    cells.len() == column_count
        && cells.iter().all(|cell| {
            let marker = cell.trim();
            marker.len() >= 3
                && marker.chars().all(|ch| matches!(ch, '-' | ':'))
                && marker.contains('-')
        })
}

fn split_unescaped_pipes(content: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for ch in content.chars() {
        if escaped {
            cell.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '|' {
            cells.push(std::mem::take(&mut cell));
        } else {
            cell.push(ch);
        }
    }
    cells.push(cell);
    cells
}
