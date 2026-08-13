use crate::composer_atoms::{AtomBuffer, AtomCursor, ComposerAtom, WrappedLine};
use crate::composer_editing::{ComposerEditor, Selection};

use super::ComposerPresentationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerSurface {
    Startup,
    Live,
    Shell,
    Plan,
    Permission,
    InlinePrompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerTone {
    Standard,
    Shell,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerChrome {
    Border,
    Metadata,
    Title,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerEditorModel {
    pub(super) text: String,
    pub(super) atoms: Vec<ComposerAtom>,
    pub(super) cursor: AtomCursor,
    pub(super) selection: Option<Selection>,
    pub(super) wrapped_lines: Vec<WrappedLine>,
    pub(super) viewport_rows: usize,
}

impl ComposerEditorModel {
    pub fn new(
        editor: &ComposerEditor,
        wrap_width: u16,
        max_viewport_lines: usize,
    ) -> Result<Self, ComposerPresentationError> {
        if wrap_width == 0 {
            return Err(ComposerPresentationError::ZeroWrapWidth);
        }
        if max_viewport_lines == 0 {
            return Err(ComposerPresentationError::ZeroViewportLines);
        }
        Ok(Self::from_editor(editor, wrap_width, max_viewport_lines))
    }

    pub(super) fn for_layout(
        editor: &ComposerEditor,
        wrap_width: u16,
        max_viewport_lines: usize,
    ) -> Self {
        Self::from_editor(editor, wrap_width.max(1), max_viewport_lines.max(1))
    }

    fn from_editor(editor: &ComposerEditor, wrap_width: u16, max_viewport_lines: usize) -> Self {
        let wrapped_lines = editor.buffer().wrap(wrap_width);
        let viewport_rows = wrapped_lines.len().min(max_viewport_lines).max(1);
        Self {
            text: editor.text(),
            atoms: editor.buffer().atoms().to_vec(),
            cursor: editor.cursor(),
            selection: editor.selection(),
            wrapped_lines,
            viewport_rows,
        }
    }

    pub fn reflow(
        &self,
        wrap_width: u16,
        max_viewport_lines: usize,
    ) -> Result<Self, ComposerPresentationError> {
        if wrap_width == 0 {
            return Err(ComposerPresentationError::ZeroWrapWidth);
        }
        if max_viewport_lines == 0 {
            return Err(ComposerPresentationError::ZeroViewportLines);
        }
        let buffer = AtomBuffer::from_atoms(self.atoms.clone())?;
        let wrapped_lines = buffer.wrap(wrap_width);
        Ok(Self {
            text: self.text.clone(),
            atoms: self.atoms.clone(),
            cursor: self.cursor,
            selection: self.selection,
            viewport_rows: wrapped_lines.len().min(max_viewport_lines).max(1),
            wrapped_lines,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn atoms(&self) -> &[ComposerAtom] {
        &self.atoms
    }

    pub const fn cursor(&self) -> AtomCursor {
        self.cursor
    }

    pub const fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn wrapped_lines(&self) -> &[WrappedLine] {
        &self.wrapped_lines
    }

    pub const fn viewport_rows(&self) -> usize {
        self.viewport_rows
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerPresentationConfig {
    pub surface: ComposerSurface,
    pub focused: bool,
    pub disabled: bool,
    pub available_rows: u16,
    pub placeholder: Option<String>,
}

impl ComposerPresentationConfig {
    pub const fn new(
        surface: ComposerSurface,
        focused: bool,
        disabled: bool,
        available_rows: u16,
    ) -> Self {
        Self {
            surface,
            focused,
            disabled,
            available_rows,
            placeholder: None,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ComposerPresentation<'a> {
    editor: &'a ComposerEditorModel,
    config: ComposerPresentationConfig,
    text_rows: u16,
    chrome: Vec<ComposerChrome>,
    collapsed: bool,
}

impl<'a> ComposerPresentation<'a> {
    pub fn resolve(
        editor: &'a ComposerEditorModel,
        config: ComposerPresentationConfig,
    ) -> Result<Self, ComposerPresentationError> {
        if config.available_rows == 0 {
            return Err(ComposerPresentationError::ZeroAvailableRows);
        }
        let collapsed = editor.text.is_empty()
            && !config.focused
            && matches!(
                config.surface,
                ComposerSurface::Live | ComposerSurface::Plan
            );
        let text_rows = if collapsed {
            1
        } else {
            u16::try_from(editor.viewport_rows)
                .unwrap_or(u16::MAX)
                .min(config.available_rows)
                .max(1)
        };
        let chrome = if collapsed {
            Vec::new()
        } else {
            visible_chrome(config.available_rows.saturating_sub(text_rows))
        };
        Ok(Self {
            editor,
            config,
            text_rows,
            chrome,
            collapsed,
        })
    }

    pub const fn editor(&self) -> &'a ComposerEditorModel {
        self.editor
    }

    pub fn body(&self) -> &str {
        if self.editor.text.is_empty() {
            self.config.placeholder.as_deref().unwrap_or_default()
        } else {
            &self.editor.text
        }
    }

    pub const fn text_rows(&self) -> u16 {
        self.text_rows
    }

    pub fn visible_chrome(&self) -> &[ComposerChrome] {
        &self.chrome
    }

    pub const fn collapsed(&self) -> bool {
        self.collapsed
    }

    pub const fn config(&self) -> &ComposerPresentationConfig {
        &self.config
    }

    pub const fn tone(&self) -> ComposerTone {
        match self.config.surface {
            ComposerSurface::Shell => ComposerTone::Shell,
            ComposerSurface::Plan => ComposerTone::Plan,
            ComposerSurface::Startup
            | ComposerSurface::Live
            | ComposerSurface::Permission
            | ComposerSurface::InlinePrompt => ComposerTone::Standard,
        }
    }

    pub fn shows(&self, chrome: ComposerChrome) -> bool {
        self.chrome.contains(&chrome)
    }
}

fn visible_chrome(mut rows: u16) -> Vec<ComposerChrome> {
    let mut visible = vec![ComposerChrome::Border];
    for (chrome, cost) in [(ComposerChrome::Metadata, 1), (ComposerChrome::Title, 1)] {
        if rows < cost {
            break;
        }
        rows = rows.saturating_sub(cost);
        visible.push(chrome);
    }
    visible
}
