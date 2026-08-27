use serde::{Deserialize, Serialize};

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
    let chars: Vec<char> = text.chars().collect();
    let mut clusters = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let mut cluster = String::new();
        let first = chars[index];
        cluster.push(first);
        index += 1;

        if is_regional_indicator(first)
            && index < chars.len()
            && is_regional_indicator(chars[index])
        {
            cluster.push(chars[index]);
            index += 1;
        }

        while let Some(&next) = chars.get(index) {
            if next == '\u{200D}' {
                cluster.push(next);
                index += 1;
                if let Some(&joined) = chars.get(index) {
                    cluster.push(joined);
                    index += 1;
                }
            } else if is_grapheme_extend(next) {
                cluster.push(next);
                index += 1;
            } else {
                break;
            }
        }
        clusters.push(GraphemeCluster::new(&cluster));
    }
    clusters
}

fn cluster_display_width(text: &str) -> u16 {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    if chars.len() == 2
        && chars
            .iter()
            .all(|character| is_regional_indicator(*character))
    {
        return 2;
    }
    if chars.contains(&'\u{200D}') {
        return chars
            .iter()
            .filter(|character| !is_grapheme_extend(**character) && **character != '\u{200D}')
            .map(|character| display_width(*character))
            .max()
            .unwrap_or(0);
    }
    chars
        .iter()
        .filter(|character| !is_grapheme_extend(**character))
        .map(|character| display_width(*character))
        .sum()
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
