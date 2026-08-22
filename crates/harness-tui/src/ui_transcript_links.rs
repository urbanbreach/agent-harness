use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use crate::theme::Theme;

/// A hyperlink extracted from markdown text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExtractedLink {
    pub label: String,
    pub url: String,
    pub is_image: bool,
}

/// Try to extract a markdown link at the given position.
///
/// Handles both regular links `[label](url)` and image links `![alt](url)`.
/// Returns the extracted link and the number of bytes consumed, or `None`.
pub(super) fn try_extract_link(remaining: &str) -> Option<(ExtractedLink, usize)> {
    let (is_image, after_marker) = if let Some(rest) = remaining.strip_prefix('!') {
        (true, rest)
    } else {
        (false, remaining)
    };

    let after_bracket = after_marker.strip_prefix('[')?;
    let label_end = after_bracket.find("](")?;
    let label = after_bracket[..label_end].to_string();
    let after_label = &after_bracket[label_end + 2..];
    let url_end = after_label.find(')')?;
    let url = after_label[..url_end].to_string();

    let prefix_len = if is_image { 2 } else { 1 }; // "!(" or "["
    let consumed = prefix_len + label_end + 2 + url_end + 1;

    Some((
        ExtractedLink {
            label,
            url,
            is_image,
        },
        consumed,
    ))
}

/// Check whether a string looks like a raw URL.
pub(super) fn is_raw_url(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://")
}

/// Compute the length of a raw URL starting at the beginning of `text`.
///
/// Returns `None` if `text` does not start with a URL scheme.
pub(super) fn raw_url_len(text: &str) -> Option<usize> {
    if !is_raw_url(text) {
        return None;
    }
    // URL ends at the first whitespace or end of string
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    // Also stop at common trailing punctuation
    let end = text[..end]
        .rfind(|c: char| !matches!(c, '.' | ',' | ';' | '!' | '?' | ')' | ']'))
        .map(|i| i + 1)
        .unwrap_or(end);
    Some(end)
}

/// Create a styled span for a link label.
pub(super) fn link_label_span(label: &str, base_style: Style, theme: &Theme) -> Span<'static> {
    Span::styled(
        label.to_string(),
        base_style
            .fg(theme.markdown.link_text)
            .add_modifier(Modifier::UNDERLINED),
    )
}

/// Create a styled span for a raw URL.
pub(super) fn raw_url_span(url: &str, base_style: Style, theme: &Theme) -> Span<'static> {
    Span::styled(
        url.to_string(),
        base_style
            .fg(theme.markdown.link)
            .add_modifier(Modifier::UNDERLINED),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_markdown_link_from_transcript_text() {
        // arrange
        // act
        let (link, consumed) = try_extract_link("[label](https://example.com)").unwrap();
        // assert
        assert_eq!(link.label, "label");
        assert_eq!(link.url, "https://example.com");
        assert!(!link.is_image);
        assert_eq!(consumed, "[label](https://example.com)".len());
    }

    #[test]
    fn extracts_image_link_from_transcript_text() {
        // arrange
        // act
        let (link, _consumed) = try_extract_link("![alt](./img.png)").unwrap();
        // assert
        assert_eq!(link.label, "alt");
        assert_eq!(link.url, "./img.png");
        assert!(link.is_image);
    }

    #[test]
    fn returns_none_for_plain_text() {
        // arrange
        // act
        // assert
        assert!(try_extract_link("hello world").is_none());
    }

    #[test]
    fn detects_raw_url_in_transcript_text() {
        // arrange
        // act
        // assert
        assert!(is_raw_url("https://example.com"));
        assert!(is_raw_url("http://example.com"));
        assert!(!is_raw_url("ftp://example.com"));
        assert!(!is_raw_url("example.com"));
    }

    #[test]
    fn computes_raw_url_length() {
        // arrange
        // act
        // assert
        assert_eq!(
            raw_url_len("https://example.com rest"),
            Some("https://example.com".len())
        );
        assert_eq!(
            raw_url_len("https://example.com."),
            Some("https://example.com".len())
        );
        assert_eq!(raw_url_len("not a url"), None);
    }
}
