use std::time::Duration;

use super::key::NormalizedKey;
use crate::terminal::{KeyCode, KeyModifiers};

pub const PASTE_START_WINDOW: Duration = Duration::from_millis(2);
pub const PASTE_BURST_WINDOW: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteKind {
    Bracketed,
    Heuristic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedPaste {
    pub text: String,
    pub kind: PasteKind,
}

impl NormalizedPaste {
    pub fn new(text: impl Into<String>, kind: PasteKind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteOutput {
    Key(NormalizedKey),
    Paste(NormalizedPaste),
}

#[derive(Debug, Default)]
pub struct PasteDetector {
    pending: String,
    first_at: Option<Duration>,
    last_at: Option<Duration>,
    candidate: bool,
}

impl PasteDetector {
    pub fn ingest_key(&mut self, at: Duration, key: NormalizedKey) -> Vec<PasteOutput> {
        let Some(character) = self.printable_character(key) else {
            return self.flush_all_with_boundary();
        };
        let mut output = self.flush_due(at);
        match self.last_at {
            None => self.start(character, at),
            Some(last_at) if self.candidate => {
                if at.saturating_sub(last_at) <= PASTE_BURST_WINDOW {
                    self.append(character, at);
                } else {
                    output.extend(self.flush_all_with_boundary());
                    self.start(character, at);
                }
            }
            Some(first_at) => {
                if at.saturating_sub(first_at) <= PASTE_START_WINDOW {
                    self.append(character, at);
                    self.candidate = true;
                } else {
                    output.extend(self.flush_all_with_boundary());
                    self.start(character, at);
                }
            }
        }
        output
    }

    pub fn flush_due(&mut self, at: Duration) -> Vec<PasteOutput> {
        let Some(last_at) = self.last_at else {
            return Vec::new();
        };
        let window = if self.candidate {
            PASTE_BURST_WINDOW
        } else {
            PASTE_START_WINDOW
        };
        if at.saturating_sub(last_at) > window && !self.holds_grapheme_tail() {
            self.flush_all_with_boundary()
        } else {
            Vec::new()
        }
    }

    pub fn flush_all(&mut self) -> Vec<PasteOutput> {
        self.flush_all_with_boundary()
    }

    fn printable_character(&self, key: NormalizedKey) -> Option<char> {
        match key.code {
            KeyCode::Char(character)
                if key.modifiers == KeyModifiers::NONE && !character.is_control() =>
            {
                Some(character)
            }
            _ => None,
        }
    }

    fn start(&mut self, character: char, at: Duration) {
        self.pending.clear();
        self.pending.push(character);
        self.first_at = Some(at);
        self.last_at = Some(at);
        self.candidate = false;
    }

    fn append(&mut self, character: char, at: Duration) {
        self.pending.push(character);
        self.last_at = Some(at);
    }

    fn flush_all_with_boundary(&mut self) -> Vec<PasteOutput> {
        let pending = std::mem::take(&mut self.pending);
        self.first_at = None;
        self.last_at = None;
        let candidate = std::mem::take(&mut self.candidate);
        if pending.is_empty() {
            return Vec::new();
        }
        if candidate {
            return vec![PasteOutput::Paste(NormalizedPaste::new(
                pending,
                PasteKind::Heuristic,
            ))];
        }
        pending
            .chars()
            .map(|character| {
                PasteOutput::Key(NormalizedKey::new(
                    KeyCode::Char(character),
                    KeyModifiers::NONE,
                ))
            })
            .collect()
    }

    fn holds_grapheme_tail(&self) -> bool {
        self.pending
            .chars()
            .last()
            .is_some_and(is_grapheme_continuation)
    }
}

fn is_grapheme_continuation(character: char) -> bool {
    matches!(
        character,
        '\u{200D}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{1F3FB}'..='\u{1F3FF}'
            | '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}
