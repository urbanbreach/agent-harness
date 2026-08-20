//! Local inline image rendering for the transcript.
//!
//! Terminal renderers cannot display actual images inline. When the assistant
//! message contains image markdown (`![alt text](path)`), this module
//! provides a placeholder that preserves the alt text without exposing the
//! raw file path or markdown syntax.
//!
//! The placeholder is replay-safe: it does not attempt to read the file,
//! negotiate terminal protocols, or execute any side effects.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::theme::Theme;

/// The glyph used to mark an inline image placeholder.
pub(super) const IMAGE_PLACEHOLDER_GLYPH: &str = "\u{25C6}";

/// Check whether a URL is a local file path (as opposed to an http(s) URL).
pub(super) fn is_local_image_url(url: &str) -> bool {
    !url.starts_with("http://") && !url.starts_with("https://") && !url.is_empty()
}

/// Try to parse an image markdown span at the given position.
///
/// Returns `(alt_text, url, consumed_len)` if an image markdown span
/// (`![alt](url)`) is found at the start of `remaining`, or `None` otherwise.
pub(super) fn try_parse_image_markdown(remaining: &str) -> Option<(String, String, usize)> {
    let rest = remaining.strip_prefix('!')?;
    let after_bracket = rest.strip_prefix('[')?;
    let label_end = after_bracket.find("](")?;
    let alt_text = after_bracket[..label_end].to_string();
    let after_label = &after_bracket[label_end + 2..];
    let url_end = after_label.find(')')?;
    let url = after_label[..url_end].to_string();
    // Total consumed: ! [ alt ]( url )
    let consumed = 1 + 1 + label_end + 2 + url_end + 1;
    Some((alt_text, url, consumed))
}

/// Render an inline image placeholder line.
///
/// Returns a line with a styled placeholder that includes the alt text,
/// without exposing the raw file path or markdown syntax.
pub(super) fn render_image_placeholder_line(
    alt_text: &str,
    prefix: &str,
    theme: &Theme,
) -> Line<'static> {
    let display_text = if alt_text.is_empty() {
        "image".to_string()
    } else {
        alt_text.to_string()
    };

    let label = format!(
        "{glyph} image: {display_text}",
        glyph = IMAGE_PLACEHOLDER_GLYPH,
    );

    let placeholder_style = Style::default()
        .fg(theme.text.secondary)
        .add_modifier(Modifier::ITALIC);

    let prefix_span = Span::raw(prefix.to_string());
    let label_span = Span::styled(label, placeholder_style);

    Line::from(vec![prefix_span, label_span])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_image_markdown_into_media_reference() {
        // arrange
        // act
        let (alt, url, consumed) =
            try_parse_image_markdown("![screenshot](./img.png) rest").unwrap();
        // assert
        assert_eq!(alt, "screenshot");
        assert_eq!(url, "./img.png");
        assert_eq!(consumed, 24);
    }

    #[test]
    fn returns_none_for_link_not_image() {
        // arrange
        // act
        // assert
        assert!(try_parse_image_markdown("[label](url)").is_none());
    }

    #[test]
    fn returns_none_for_plain_text() {
        // arrange
        // act
        // assert
        assert!(try_parse_image_markdown("hello world").is_none());
    }

    #[test]
    fn detects_local_image_url() {
        // arrange
        // act
        // assert
        assert!(is_local_image_url("./screenshots/ui.png"));
        assert!(is_local_image_url("/absolute/path/to/image.png"));
        assert!(!is_local_image_url("https://example.com/image.png"));
        assert!(!is_local_image_url("http://example.com/image.png"));
        assert!(!is_local_image_url(""));
    }

    #[test]
    fn handles_empty_alt_text() {
        // arrange
        // act
        let (alt, _url, _consumed) = try_parse_image_markdown("![](./img.png)").unwrap();
        // assert
        assert_eq!(alt, "");
    }
}
