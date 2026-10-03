use crate::composer_editing::{ComposerEditor, DeleteKind, EditingError};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PaneQueryMode {
    #[default]
    Search,
    Filter,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct PaneQuery {
    pub(crate) editing: bool,
    pub(crate) mode: PaneQueryMode,
    pub(crate) editor: ComposerEditor,
    pub(crate) regex: Option<regex::Regex>,
    pub(crate) active: bool,
}

impl PaneQuery {
    pub(crate) fn has_bar(&self) -> bool {
        self.editing || self.active
    }

    pub(crate) fn matches(&self, text: &str) -> bool {
        self.regex
            .as_ref()
            .is_some_and(|regex| regex.is_match(text))
    }

    pub(crate) fn permits(&self, text: &str) -> bool {
        self.mode != PaneQueryMode::Filter || !self.active || self.matches(text)
    }

    pub(in crate::app) fn open(&mut self, mode: PaneQueryMode) {
        if !self.active || self.mode != mode {
            *self = Self::default();
        }
        self.mode = mode;
        self.editing = true;
    }

    fn update(&mut self) {
        let text = self.editor.text();
        self.active = !text.is_empty();
        // Match Grok's smart-case regular expressions. Invalid patterns match nothing.
        self.regex = self
            .active
            .then(|| {
                regex::RegexBuilder::new(&text)
                    .case_insensitive(!text.chars().any(char::is_uppercase))
                    .build()
                    .ok()
            })
            .flatten();
    }

    pub(in crate::app) fn close_unaccepted(&mut self) {
        if self.editing {
            *self = Self::default();
        }
    }

    pub(in crate::app) fn paste(&mut self, text: &str) -> Result<(), EditingError> {
        if self.editing {
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            self.editor.paste(&text)?;
            self.update();
        }
        Ok(())
    }

    pub(in crate::app) fn handle_key(&mut self, key: KeyEvent) -> Result<(), EditingError> {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => self.editing = false,
            (KeyCode::Esc, _) => *self = Self::default(),
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.editor.text().is_empty() {
                    *self = Self::default();
                } else {
                    self.editor = ComposerEditor::new();
                }
            }
            (KeyCode::Backspace, _) | (KeyCode::Char('w' | 'h'), KeyModifiers::CONTROL) => {
                if self.editor.text().is_empty() {
                    *self = Self::default();
                } else {
                    self.delete_backward(key)?;
                }
            }
            (KeyCode::Delete, modifiers)
                if modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.delete_word(true, false)?
            }
            (KeyCode::Delete, _) | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.editor.delete(DeleteKind::CharacterForward)?;
            }
            (KeyCode::Char('d'), KeyModifiers::ALT | KeyModifiers::SUPER) => {
                self.delete_word(true, false)?;
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.delete_to_edge(false)?,
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => self.delete_to_edge(true)?,
            (KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::ALT)
            | (KeyCode::Char('b'), KeyModifiers::ALT) => self.move_word(false),
            (KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::ALT)
            | (KeyCode::Char('f'), KeyModifiers::ALT) => self.move_word(true),
            (KeyCode::Left, _) | (KeyCode::Char('b'), KeyModifiers::CONTROL) => {
                self.editor.move_left()
            }
            (KeyCode::Right, _) | (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                self.editor.move_right()
            }
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.editor.move_line_start()
            }
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.editor.move_line_end()
            }
            (KeyCode::Char(c), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !c.is_control() =>
            {
                self.editor.insert_text(&c.to_string())?
            }
            _ => {}
        }
        self.update();
        Ok(())
    }

    fn delete_to_edge(&mut self, forward: bool) -> Result<(), EditingError> {
        let count = if forward {
            self.editor.buffer().atoms().len() - self.editor.cursor().insertion_index()
        } else {
            self.editor.cursor().insertion_index()
        };
        for _ in 0..count {
            if forward {
                self.editor.select_char_right();
            } else {
                self.editor.select_char_left();
            }
        }
        if count > 0 {
            self.editor.backspace()?;
        }
        Ok(())
    }

    fn word_steps(&self, forward: bool, whitespace_only: bool) -> usize {
        let text = self.editor.text();
        let cursor = self.editor.cursor().insertion_index();
        if forward {
            query_word_steps(text.graphemes(true).skip(cursor), whitespace_only)
        } else {
            let end = text
                .grapheme_indices(true)
                .nth(cursor)
                .map_or(text.len(), |(byte, _)| byte);
            query_word_steps(text[..end].graphemes(true).rev(), whitespace_only)
        }
    }

    fn move_word(&mut self, forward: bool) {
        for _ in 0..self.word_steps(forward, false) {
            if forward {
                self.editor.move_right();
            } else {
                self.editor.move_left();
            }
        }
    }

    fn delete_word(&mut self, forward: bool, whitespace_only: bool) -> Result<(), EditingError> {
        let steps = self.word_steps(forward, whitespace_only);
        for _ in 0..steps {
            if forward {
                self.editor.select_char_right();
            } else {
                self.editor.select_char_left();
            }
        }
        if steps > 0 {
            self.editor.backspace()?;
        }
        Ok(())
    }

    fn delete_backward(&mut self, key: KeyEvent) -> Result<(), EditingError> {
        if key.code == KeyCode::Char('w') {
            self.delete_word(false, true)?;
        } else if key.modifiers == KeyModifiers::SUPER {
            self.delete_to_edge(false)?;
        } else if key.code == KeyCode::Backspace
            && matches!(key.modifiers, KeyModifiers::ALT | KeyModifiers::CONTROL)
        {
            self.delete_word(false, false)?;
        } else {
            self.editor.backspace()?;
        }
        Ok(())
    }
}

fn query_word_steps<'a>(graphemes: impl Iterator<Item = &'a str>, whitespace_only: bool) -> usize {
    let mut classes = graphemes
        .map(|grapheme| {
            let c = grapheme.chars().next().unwrap_or(' ');
            (
                c.is_whitespace(),
                whitespace_only || c.is_alphanumeric() || c == '_',
            )
        })
        .peekable();
    let mut steps = 0;
    while classes.peek().is_some_and(|(space, _)| *space) {
        classes.next();
        steps += 1;
    }
    if let Some(class) = classes.next() {
        steps += 1 + classes.take_while(|next| *next == class).count();
    }
    steps
}
