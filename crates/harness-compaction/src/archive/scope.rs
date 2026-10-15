//! One dim-aware scanner for archive scope labels: full reasoning removal, known guide
//! legends and the scope boundary of a legacy text edge all share it.
//!
//! A label counts only at the start of the text or right after a line feed or FULL BLOCK,
//! and never while a dim-output span is open: output text can quote labels verbatim.

const DIM_ON: char = '\u{e}';
const DIM_OFF: char = '\u{f}';
const FULL_BLOCK: char = '█';

pub(super) const USER: &str = "¶user:";
pub(super) const THINK: &str = "¶think:";
pub(super) const AI: &str = "¶ai:";
pub(super) const CALL: &str = "¶call:";
const LABELS: [&str; 4] = [USER, THINK, AI, CALL];

/// Legend entries that name the reasoning scope, as the current and earlier guides wrote
/// them.
const LEGENDS: [&str; 7] = [
    "¶think: assistant reasoning.\n",
    "¶think: assistant reasoning.█",
    "¶think: assistant reasoning; ",
    "¶think: assistant reasoning, ",
    "; ¶think: assistant reasoning",
    ", ¶think: assistant reasoning",
    "¶think: assistant reasoning ",
];

/// A scope label found outside dim output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Boundary {
    pub index: usize,
    pub label: &'static str,
}

/// Whether the text starts inside a dim span whose opening control was cut away: its first
/// dim control closes a span.
fn starts_dim(text: &str) -> bool {
    text.chars()
        .find(|c| matches!(*c, DIM_ON | DIM_OFF))
        .is_some_and(|c| c == DIM_OFF)
}

/// Scope boundaries outside dim output, in order. `inherit_dim` treats a text whose first
/// dim control is a closer as starting inside a span (a legacy text edge).
pub(super) fn boundaries(text: &str, inherit_dim: bool) -> Vec<Boundary> {
    let mut found = Vec::new();
    let mut dim = inherit_dim && starts_dim(text);
    let mut at_start = true;
    for (index, ch) in text.char_indices() {
        if !dim
            && at_start
            && let Some(label) = LABELS.iter().find(|l| text[index..].starts_with(**l))
        {
            found.push(Boundary { index, label });
        }
        match ch {
            DIM_ON => dim = true,
            DIM_OFF => dim = false,
            _ => {}
        }
        at_start = matches!(ch, '\n' | FULL_BLOCK);
    }
    found
}

/// Whether any real reasoning scope is present in the source text.
pub(super) fn has_reasoning_scope(text: &str) -> bool {
    boundaries(text, false).iter().any(|b| b.label == THINK)
}

/// Whether a summary guide names the reasoning scope.
pub fn has_reasoning_legend(summary: &str) -> bool {
    summary.contains(THINK)
}

/// The text from its first recognized scope boundary on; an opening unlabeled fragment may
/// belong to reasoning and is discarded. Empty when no boundary exists.
pub fn scoped_suffix(text: &str) -> &str {
    boundaries(text, true)
        .first()
        .map_or("", |boundary| &text[boundary.index..])
}

/// Remove known reasoning legend entries that sit outside dim output.
fn remove_legend(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut dim = false;
    let mut skip_until = 0;
    for (index, ch) in text.char_indices() {
        if index < skip_until {
            continue;
        }
        if !dim && let Some(legend) = LEGENDS.iter().find(|l| text[index..].starts_with(**l)) {
            skip_until = index + legend.len();
            continue;
        }
        match ch {
            DIM_ON => dim = true,
            DIM_OFF => dim = false,
            _ => {}
        }
        output.push(ch);
    }
    output
}

/// Remove complete reasoning scopes (the label and every paragraph up to the next scope)
/// and the reasoning entry of known guide legends. Dim output is never structure.
pub fn strip_reasoning(text: &str) -> String {
    // Legends first: a guide folded into source text puts the reasoning entry at a line
    // start, where it would otherwise read as a scope.
    let text = remove_legend(text);
    let text = text.as_str();
    let marks = boundaries(text, false);
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for (position, mark) in marks.iter().enumerate() {
        if mark.label != THINK {
            continue;
        }
        output.push_str(&text[cursor..mark.index]);
        cursor = marks
            .get(position + 1)
            .map_or(text.len(), |next| next.index);
        if cursor == text.len() {
            // A removed final scope leaves no dangling separator behind.
            while output.ends_with(['\n', FULL_BLOCK]) {
                output.pop();
            }
        }
    }
    output.push_str(&text[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scopes_but_not_dim_output_or_inline_mentions() {
        for (text, expected) in [
            (
                "¶user:a\n\n¶think:hidden\n\nparagraph\n\n¶ai:b",
                "¶user:a\n\n¶ai:b",
            ),
            ("¶user:a█¶think:hidden█more█¶ai:b", "¶user:a█¶ai:b"),
            (
                "¶call:<out>\n\u{e}¶think:literal\u{f}\n</out>",
                "¶call:<out>\n\u{e}¶think:literal\u{f}\n</out>",
            ),
            ("¶ai:literal ¶think:here", "¶ai:literal ¶think:here"),
            ("¶ai:b\n\n¶think:hidden", "¶ai:b"),
            (
                "Scopes: ¶think: assistant reasoning.\n¶ai: assistant reply.",
                "Scopes: ¶ai: assistant reply.",
            ),
            (
                "¶call:\u{e}Scopes: ¶think: assistant reasoning.\n\u{f}",
                "¶call:\u{e}Scopes: ¶think: assistant reasoning.\n\u{f}",
            ),
            (
                "Scope labels: ¶user: input, ¶think: assistant reasoning, ¶ai: reply",
                "Scope labels: ¶user: input, ¶ai: reply",
            ),
        ] {
            assert_eq!(strip_reasoning(text), expected);
        }
    }

    #[test]
    fn legacy_edge_ignores_labels_inside_a_cut_dim_span() {
        for (edge, expected) in [
            ("tail of thought\n¶ai:reply", "¶ai:reply"),
            (
                "output tail\n¶think:literal\u{f}\n</out>\n\n¶ai:reply",
                "¶ai:reply",
            ),
            ("no labels at all", ""),
        ] {
            assert_eq!(scoped_suffix(edge), expected);
        }
    }

    #[test]
    fn reasoning_presence_ignores_dim_output_literals() {
        assert!(has_reasoning_scope("¶user:a\n\n¶think:b"));
        assert!(!has_reasoning_scope(
            "¶call:<out>\n\u{e}¶think:b\u{f}\n</out>"
        ));
    }
}
