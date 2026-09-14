use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use crate::terminal::char_display_width;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphemeCluster {
    text: String,
    display_width: u16,
}

impl GraphemeCluster {
    pub fn new(text: &str) -> Self {
        Self {
            display_width: cluster_display_width(text),
            text: text.to_owned(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub const fn display_width(&self) -> u16 {
        self.display_width
    }
}

pub(crate) fn split_graphemes(text: &str) -> Vec<GraphemeCluster> {
    text.graphemes(true).map(GraphemeCluster::new).collect()
}

fn cluster_display_width(text: &str) -> u16 {
    if text.chars().count() == 2 && text.chars().all(is_regional_indicator) {
        return 2;
    }
    let widths = text
        .chars()
        .filter(|character| !is_grapheme_extend(*character) && *character != '\u{200D}')
        .map(display_width);
    if text.contains('\u{200D}') {
        widths.max().unwrap_or(0)
    } else {
        widths.sum()
    }
}

fn display_width(character: char) -> u16 {
    let measured = char_display_width(character);
    if measured == 1 && is_extended_emoji(character) {
        2
    } else {
        measured
    }
}

fn is_regional_indicator(character: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&character)
}

fn is_extended_emoji(character: char) -> bool {
    ('\u{1F300}'..='\u{1FAFF}').contains(&character)
}

fn is_grapheme_extend(character: char) -> bool {
    matches!(
        character,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{1F3FB}'..='\u{1F3FF}'
    )
}
