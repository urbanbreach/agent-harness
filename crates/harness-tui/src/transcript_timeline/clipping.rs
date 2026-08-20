use crate::terminal::char_display_width;

/// Returns the terminal cell width of a marker label.
pub fn marker_display_width(label: &str) -> usize {
    label
        .chars()
        .map(|character| usize::from(char_display_width(character)))
        .sum()
}

/// Clips a label at a cell boundary without splitting a wide character.
pub fn clip_marker_label(label: &str, max_width: usize) -> String {
    let mut clipped = String::new();
    let mut used_width = 0usize;

    for character in label.chars() {
        let character_width = usize::from(char_display_width(character));
        if character_width > 0 && used_width.saturating_add(character_width) > max_width {
            break;
        }
        clipped.push(character);
        used_width = used_width.saturating_add(character_width);
    }

    clipped
}

/// Returns the width available to marker labels for a viewport.
pub const fn marker_column_width(viewport_width: u16) -> usize {
    if viewport_width == 0 {
        0
    } else if viewport_width < 80 {
        1
    } else if viewport_width < 132 {
        2
    } else if viewport_width < 160 {
        3
    } else {
        4
    }
}

/// Measures a label after applying the marker-column budget.
pub fn marker_label_width(label: &str, max_width: usize) -> u16 {
    let clipped = clip_marker_label(label, max_width);
    u16::try_from(marker_display_width(&clipped)).map_or(u16::MAX, |width| width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_keeps_cjk_codepoints_whole() {
        // arrange
        // act
        // assert
        assert_eq!(clip_marker_label("成功", 3), "成");
        assert_eq!(marker_display_width("成"), 2);
    }
}
