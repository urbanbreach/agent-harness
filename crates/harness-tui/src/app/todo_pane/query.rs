use crate::composer_editing::{ComposerEditor, DeleteKind, EditingError};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TodoQueryMode {
    #[default]
    Search,
    Filter,
}

#[derive(Debug, Default)]
pub(crate) struct TodoQuery {
    pub(crate) editing: bool,
    pub(crate) mode: TodoQueryMode,
    pub(crate) editor: ComposerEditor,
    pub(crate) regex: Option<regex::Regex>,
    pub(crate) active: bool,
}

impl TodoQuery {
    pub(crate) fn has_bar(&self) -> bool {
        self.editing || self.active
    }

    pub(crate) fn matches(&self, text: &str) -> bool {
        self.regex
            .as_ref()
            .is_some_and(|regex| regex.is_match(text))
    }

    pub(crate) fn permits(&self, text: &str) -> bool {
        self.mode != TodoQueryMode::Filter || !self.active || self.matches(text)
    }

    pub(super) fn open(&mut self, mode: TodoQueryMode) {
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

    pub(super) fn close_unaccepted(&mut self) {
        if self.editing {
            *self = Self::default();
        }
    }

    pub(super) fn paste(&mut self, text: &str) -> Result<(), EditingError> {
        if self.editing {
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            self.editor.paste(&text)?;
            self.update();
        }
        Ok(())
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Result<(), EditingError> {
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
            (KeyCode::Backspace, _) | (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                if self.editor.text().is_empty() {
                    *self = Self::default();
                } else if key.code == KeyCode::Backspace {
                    self.editor.backspace()?;
                } else {
                    self.editor.delete(DeleteKind::WordBackward)?;
                }
            }
            (KeyCode::Delete, _) => self.editor.delete(DeleteKind::CharacterForward)?,
            (KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.editor.move_word_left()
            }
            (KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.editor.move_word_right()
            }
            (KeyCode::Left, _) => self.editor.move_left(),
            (KeyCode::Right, _) => self.editor.move_right(),
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
}
