use super::{
    permissions::{PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage},
    Focus,
};
use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionPointerTarget {
    Decision(PermissionModalSelection),
    Confirm(PermissionConfirmSelection),
    QuestionChoice(usize),
    QuestionSubmit,
    QuestionScrollbar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionPointerDown {
    pub(crate) permission_id: String,
    pub(crate) target: PermissionPointerTarget,
    pub(crate) area: Rect,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PermissionPromptState {
    pub(crate) permission_id: Option<String>,
    pub(crate) stage: PermissionModalStage,
    pub(crate) selection: PermissionModalSelection,
    pub(crate) confirm_selection: PermissionConfirmSelection,
    pub(crate) detail_expanded: bool,
    pub(crate) pointer_down: Option<PermissionPointerDown>,
    pub(crate) focus_return: Option<Focus>,
    pub(crate) feedback: Option<PermissionFeedback>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PermissionFeedback {
    text: String,
    cursor: usize,
    pub(crate) editing: bool,
}

impl PermissionFeedback {
    pub(super) fn edit(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(character) if !character.is_control() => {
                self.text.insert(self.cursor, character);
                let inserted_end = self.cursor.saturating_add(character.len_utf8());
                // A typed scalar can join a combining or ZWJ cluster on either side.
                self.cursor = self
                    .text
                    .grapheme_indices(true)
                    .map(|(index, _)| index)
                    .find(|index| *index >= inserted_end)
                    .unwrap_or(self.text.len());
            }
            KeyCode::Left => self.cursor = self.previous_boundary(),
            KeyCode::Right => self.cursor = self.next_boundary(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            KeyCode::Backspace => {
                let start = self.previous_boundary();
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
            }
            KeyCode::Delete => {
                self.text
                    .replace_range(self.cursor..self.next_boundary(), "");
            }
            _ => {}
        }
    }

    fn previous_boundary(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self) -> usize {
        self.cursor.saturating_add(
            self.text[self.cursor..]
                .graphemes(true)
                .next()
                .map_or(0, str::len),
        )
    }

    pub(super) fn reason(&self) -> Option<String> {
        let text = self.text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    pub(crate) fn visible_parts(&self, width: usize) -> (&str, &str) {
        let mut remaining = width.saturating_sub(usize::from(self.editing));
        let mut start = self.cursor;
        for (index, grapheme) in self.text[..self.cursor].grapheme_indices(true).rev() {
            let cells = grapheme.width();
            if cells > remaining {
                break;
            }
            remaining = remaining.saturating_sub(cells);
            start = index;
        }
        let mut end = self.cursor;
        for grapheme in self.text[self.cursor..].graphemes(true) {
            let cells = grapheme.width();
            if cells > remaining {
                break;
            }
            remaining = remaining.saturating_sub(cells);
            end = end.saturating_add(grapheme.len());
        }
        (&self.text[start..self.cursor], &self.text[self.cursor..end])
    }
}
