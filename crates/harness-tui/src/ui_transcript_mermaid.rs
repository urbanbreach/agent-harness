//! Mermaid diagram block rendering for the transcript.
//!
//! When a fenced code block has the `mermaid` language, instead of rendering
//! the raw diagram source code (which is meaningless in a terminal), this
//! module renders a compact placeholder that identifies the diagram type and
//! size.
//!
//! Terminal renderers cannot draw actual Mermaid diagrams; the placeholder
//! preserves the diagram metadata (type, line count) without dumping raw
//! graph syntax into the scrollback.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::theme::Theme;

/// The glyph used to mark a Mermaid diagram placeholder.
pub(super) const MERMAID_PLACEHOLDER_GLYPH: &str = "\u{25C6}";

/// Detect the Mermaid diagram type from the first non-empty line of the body
/// and normalize it to a human-readable label.
///
/// Common types: `graph`, `flowchart`, `sequenceDiagram`, `classDiagram`,
/// `stateDiagram`, `erDiagram`, `gantt`, `pie`, `journey`, etc.
/// The raw keyword is normalized so the raw mermaid syntax does not appear
/// in the rendered placeholder.
fn detect_diagram_type(body: &str) -> Option<&'static str> {
    let raw = body.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.split_whitespace().next()
        }
    })?;
    Some(normalize_diagram_type(raw))
}

/// Normalize a raw mermaid diagram keyword to a human-readable label.
fn normalize_diagram_type(raw: &str) -> &'static str {
    match raw {
        "graph" | "flowchart" | "flow" => "graph",
        "sequenceDiagram" => "sequence",
        "classDiagram" | "classDiagram-v2" => "class",
        "stateDiagram" | "stateDiagram-v2" => "state",
        "erDiagram" => "ER",
        "gantt" => "gantt",
        "pie" => "pie",
        "journey" => "journey",
        "gitGraph" => "git",
        "C4Context" | "C4Container" | "C4Component" => "C4",
        "mindmap" => "mindmap",
        "timeline" => "timeline",
        "quadrantChart" => "quadrant",
        "xychart-beta" => "xychart",
        "requirementDiagram" => "requirement",
        "architecture-beta" => "architecture",
        _ => "diagram",
    }
}

/// Render a Mermaid diagram placeholder.
///
/// Returns a vector of lines that replace the raw diagram source in the
/// transcript. The placeholder shows the diagram type and line count
/// without exposing the raw graph syntax.
pub(super) fn render_mermaid_placeholder(
    body: &str,
    prefix: &str,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let line_count = body.lines().count();
    let diagram_type = detect_diagram_type(body).unwrap_or("diagram");

    let label = format!(
        "{glyph} Mermaid {diagram_type} ({line_count} line{plural})",
        glyph = MERMAID_PLACEHOLDER_GLYPH,
        diagram_type = diagram_type,
        line_count = line_count,
        plural = if line_count == 1 { "" } else { "s" },
    );

    let placeholder_style = Style::default()
        .fg(theme.text.secondary)
        .add_modifier(Modifier::ITALIC);

    let prefix_span = Span::raw(prefix.to_string());
    let label_span = Span::styled(label, placeholder_style);

    vec![Line::from(vec![prefix_span, label_span])]
}

/// Check whether a fenced code block language is Mermaid.
pub(super) fn is_mermaid_language(language: Option<&str>) -> bool {
    language.is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_graph_diagram_type() {
        assert_eq!(detect_diagram_type("graph TD\n  A --> B"), Some("graph"));
    }

    #[test]
    fn detects_sequence_diagram_type() {
        assert_eq!(
            detect_diagram_type("sequenceDiagram\n  Alice->>Bob: Hi"),
            Some("sequence")
        );
    }

    #[test]
    fn detects_flowchart_diagram_type() {
        assert_eq!(
            detect_diagram_type("flowchart LR\n  X --> Y"),
            Some("graph")
        );
    }

    #[test]
    fn returns_none_for_empty_body() {
        assert_eq!(detect_diagram_type(""), None);
    }

    #[test]
    fn is_mermaid_language_matches_case_insensitive() {
        assert!(is_mermaid_language(Some("mermaid")));
        assert!(is_mermaid_language(Some("Mermaid")));
        assert!(is_mermaid_language(Some("MERMAID")));
        assert!(!is_mermaid_language(Some("rust")));
        assert!(!is_mermaid_language(None));
    }
}
