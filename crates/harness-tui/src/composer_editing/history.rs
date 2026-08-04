#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRecall {
    pub text: String,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptHistory {
    entries: Vec<String>,
    index: Option<usize>,
    scratch: Option<HistoryRecall>,
}

impl PromptHistory {
    pub fn new(entries: Vec<String>) -> Self {
        Self {
            entries,
            index: None,
            scratch: None,
        }
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn index(&self) -> Option<usize> {
        self.index
    }

    pub fn set_entries(&mut self, entries: Vec<String>) {
        self.entries = entries;
        self.index = None;
        self.scratch = None;
    }

    pub fn previous(&mut self, text: &str, cursor: usize) -> Option<HistoryRecall> {
        let next = match self.index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.scratch = Some(HistoryRecall {
                    text: text.to_owned(),
                    cursor,
                });
                self.entries.len().saturating_sub(1)
            }
        };
        if self.entries.is_empty() {
            return None;
        }
        self.index = Some(next);
        Some(self.recall_at(next))
    }

    pub fn next_prompt(&mut self) -> Option<HistoryRecall> {
        let index = self.index?;
        if index + 1 < self.entries.len() {
            let next = index + 1;
            self.index = Some(next);
            return Some(self.recall_at(next));
        }
        self.index = None;
        Some(self.scratch.take().unwrap_or(HistoryRecall {
            text: String::new(),
            cursor: 0,
        }))
    }

    fn recall_at(&self, index: usize) -> HistoryRecall {
        HistoryRecall {
            text: self.entries[index].clone(),
            cursor: self.entries[index].chars().count(),
        }
    }
}

impl ComposerEditor {
    pub fn set_history(&mut self, entries: Vec<String>) {
        self.history.set_entries(entries);
    }

    pub fn history_previous(&mut self) -> bool {
        let before = self.snapshot();
        let Some(recall) = self
            .history
            .previous(&self.text(), self.cursor.insertion_index())
        else {
            return false;
        };
        self.buffer = crate::composer_atoms::AtomBuffer::from_text(&recall.text);
        self.cursor = AtomCursor::before(recall.cursor.min(self.buffer.atoms().len()));
        self.selection = None;
        self.record(before, EditGroup::History);
        true
    }

    pub fn history_next(&mut self) -> bool {
        let before = self.snapshot();
        let Some(recall) = self.history.next_prompt() else {
            return false;
        };
        self.buffer = crate::composer_atoms::AtomBuffer::from_text(&recall.text);
        self.cursor = AtomCursor::before(recall.cursor.min(self.buffer.atoms().len()));
        self.selection = None;
        self.record(before, EditGroup::History);
        true
    }

    pub fn history_entries(&self) -> &[String] {
        self.history.entries()
    }
}
use crate::composer_atoms::AtomCursor;

use super::{ComposerEditor, EditGroup};
