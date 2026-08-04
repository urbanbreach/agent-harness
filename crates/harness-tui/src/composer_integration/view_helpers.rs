use crate::attachment_lifecycle::Preview;
use crate::composer_atoms::{AtomKind, ComposerAtom};

pub(super) fn atom_char_count(atom: &ComposerAtom) -> usize {
    match &atom.kind {
        AtomKind::Text(cluster) => cluster.as_str().chars().count(),
        AtomKind::Newline => 1,
        AtomKind::FileMention(_) | AtomKind::Attachment(_) => 0,
    }
}

pub(super) fn preview_label(preview: &Preview) -> String {
    match preview {
        Preview::Image { mime, dimensions } => dimensions.map_or_else(
            || mime.as_str().to_owned(),
            |dimensions| {
                format!(
                    "{} {}x{}",
                    mime.as_str(),
                    dimensions.width,
                    dimensions.height
                )
            },
        ),
        Preview::Text { text, truncated } => {
            let suffix = if *truncated { "…" } else { "" };
            format!("{}{}", text.lines().next().unwrap_or(""), suffix)
        }
        Preview::Binary { bytes, .. } => format!("binary ({bytes} bytes)"),
    }
}
