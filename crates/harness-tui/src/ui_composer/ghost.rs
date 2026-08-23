use crate::composer_atoms::split_graphemes;

pub(super) fn truncate_to_width(text: &str, max_width: usize) -> String {
    let clusters = split_graphemes(text);
    let text_width = clusters.iter().fold(0_usize, |width, cluster| {
        width.saturating_add(usize::from(cluster.display_width()))
    });
    if text_width <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }

    let target_width = max_width.saturating_sub(1);
    let mut used_width = 0_usize;
    let mut output = String::new();
    for cluster in clusters {
        let width = usize::from(cluster.display_width());
        if used_width.saturating_add(width) > target_width {
            break;
        }
        output.push_str(cluster.as_str());
        used_width = used_width.saturating_add(width);
    }
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_preserves_cjk_cell_boundaries() {
        // arrange
        let suggestion = "界界界";

        // act
        let truncated = truncate_to_width(suggestion, 5);

        // assert
        assert_eq!(truncated, "界界…");
    }

    #[test]
    fn truncation_preserves_joined_emoji_and_combining_clusters() {
        // arrange
        let joined_emoji = "👩‍💻abc";
        let combining_mark = "e\u{301}fg";

        // act
        let joined_emoji_truncated = truncate_to_width(joined_emoji, 4);
        let combining_mark_truncated = truncate_to_width(combining_mark, 2);

        // assert
        assert_eq!(joined_emoji_truncated, "👩‍💻a…");
        assert_eq!(combining_mark_truncated, "e\u{301}…");
    }
}
