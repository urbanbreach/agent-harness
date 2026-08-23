use super::*;
use crate::ui::ui_transcript_style::blend_color;
use crate::UnwrapOrAbort;
use ratatui::widgets::BorderType;

#[path = "ui_composer/bordered.rs"]
mod bordered;
#[path = "ui_composer/collapsed.rs"]
mod collapsed;
#[path = "ui_composer/document.rs"]
mod document;
#[path = "ui_composer/file_tags.rs"]
mod file_tags;
#[path = "ui_composer/ghost.rs"]
mod ghost;
#[path = "ui_composer/identity.rs"]
mod identity;
#[path = "ui_composer/metadata.rs"]
mod metadata;
#[path = "ui_composer/presentation.rs"]
mod presentation;
#[path = "ui_composer/viewport.rs"]
mod viewport;

pub(super) use bordered::{connect_waiting_owns_input, render_bordered_composer};
pub(super) use document::render_document_composer_content;
pub(super) use file_tags::composer_line_with_file_tags;
pub(super) use identity::composer_model_badge;
pub(super) use metadata::{
    composer_metadata_candidates, composer_metadata_line, ComposerMetadataTone,
};
pub(super) use viewport::composer_viewport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposerViewport {
    pub(super) lines: Vec<String>,
    pub(super) line_starts: Vec<usize>,
    pub(super) cursor: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
struct ComposerVisualChar {
    index: usize,
    ch: char,
    width: usize,
}

type ComposerVisualLines = (Vec<(String, usize)>, Option<(usize, usize)>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComposerModeStyle {
    border: Color,
    accent: Color,
}

fn composer_mode_style(
    theme: &Theme,
    tone: crate::composer_integration::ComposerTone,
    focused: bool,
) -> ComposerModeStyle {
    use crate::composer_integration::ComposerTone;

    let standard_accent = bordered::live_composer_content_color(
        theme,
        if focused {
            theme.terminal_colors.prompt_accent
        } else {
            theme.terminal_colors.muted
        },
        focused,
    );
    match tone {
        ComposerTone::Standard => ComposerModeStyle {
            border: bordered::live_composer_border_color(theme, focused),
            accent: standard_accent,
        },
        ComposerTone::Shell => ComposerModeStyle {
            border: bordered::live_composer_border_color(theme, focused),
            accent: theme.status.warning,
        },
        ComposerTone::Plan => ComposerModeStyle {
            border: if focused {
                theme.terminal_colors.primary
            } else {
                bordered::live_composer_content_color(theme, theme.terminal_colors.primary, false)
            },
            accent: Color::LightYellow,
        },
    }
}
