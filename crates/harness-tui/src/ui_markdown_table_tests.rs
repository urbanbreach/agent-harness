use ratatui::style::Modifier;

use super::try_render_markdown_table_block;
use crate::composer_atoms::split_graphemes;
use crate::theme::Theme;
use crate::ui::ui_chrome::display_width;

fn rendered_text(lines: &[ratatui::text::Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

#[test]
fn table_preserves_nested_emphasis_and_link_label_styles() {
    // Given: table cells containing nested emphasis inside a link and ordinary emphasis.
    let theme = Theme::default();
    let rows = [
        "| Package | Status |",
        "| --- | --- |",
        "| [**harness**](https://example.com/docs) | *ready* |",
    ];

    // When: the table is rendered.
    let (lines, consumed, links) =
        try_render_markdown_table_block(&rows, theme.text.primary, "", &theme, 80)
            .expect("markdown table");

    // Then: markdown syntax and destinations are not painted, while nested styles survive.
    let text = rendered_text(&lines).join("\n");
    assert_eq!(consumed, 3);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].destination, "https://example.com/docs");
    assert!(text.contains("harness"));
    assert!(!text.contains("https://") && !text.contains("**"));
    assert!(lines.iter().flat_map(|line| &line.spans).any(|span| {
        span.content.contains("harness")
            && span.style.add_modifier.contains(Modifier::BOLD)
            && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));
    assert!(lines.iter().flat_map(|line| &line.spans).any(|span| {
        span.content.contains("ready") && span.style.add_modifier.contains(Modifier::ITALIC)
    }));
}

#[test]
fn boxed_table_aligns_cjk_and_zwj_emoji_within_normal_width() {
    // Given: wide CJK and ZWJ emoji cells.
    let theme = Theme::default();
    let rows = [
        "| Name | Value |",
        "| --- | --- |",
        "| 東京 | 👩‍💻 |",
        "| A | ok |",
    ];

    // When: the table is rendered at a normal width.
    let (lines, _, _) = try_render_markdown_table_block(&rows, theme.text.primary, "", &theme, 32)
        .expect("markdown table");
    let text = rendered_text(&lines);

    // Then: every box row has identical terminal geometry.
    assert!(text.first().is_some_and(|line| line.starts_with('┌')));
    assert!(text.last().is_some_and(|line| line.starts_with('└')));
    let widths = text
        .iter()
        .map(|line| display_width(line))
        .collect::<Vec<_>>();
    assert!(widths.iter().all(|width| *width == widths[0]));
    assert!(widths[0] <= 32);
}

#[test]
fn boxed_table_projects_complete_repeated_and_whitespace_link_ranges_through_wrapping() {
    // Given: repeated labels, plain duplicate text, whitespace, CJK, and ZWJ graphemes in cells.
    let theme = Theme::default();
    let rows = [
        "| Name | Value |",
        "| --- | --- |",
        "| same [same](https://example.com/one) | 👩‍💻中 [same](https://example.com/two) [two words](https://example.com/words) |",
    ];

    // When: the table is wrapped narrowly.
    let (lines, _, links) =
        try_render_markdown_table_block(&rows, theme.text.primary, "", &theme, 16)
            .expect("markdown table");
    let text = rendered_text(&lines);

    // Then: every complete label cell, including its internal space, is linked exactly once.
    assert_eq!(
        links
            .iter()
            .filter(|link| link.destination == "https://example.com/one")
            .count(),
        1
    );
    assert_eq!(
        links
            .iter()
            .filter(|link| link.destination == "https://example.com/two")
            .count(),
        1
    );
    let linked_words = links
        .iter()
        .filter(|link| link.destination == "https://example.com/words")
        .flat_map(|link| {
            let mut cell = 0usize;
            split_graphemes(&text[link.row])
                .into_iter()
                .filter_map(move |cluster| {
                    let start = cell;
                    cell = cell.saturating_add(usize::from(cluster.display_width()));
                    (start < link.end_cell && cell > link.start_cell)
                        .then(|| cluster.as_str().to_string())
                })
        })
        .collect::<String>();
    assert_eq!(linked_words, "two words");
    assert!(links.iter().all(|link| {
        link.start_cell > 0
            && link.start_cell < link.end_cell
            && link.end_cell < display_width(&text[link.row])
    }));
}

#[test]
fn boxed_table_wraps_cells_without_exceeding_narrow_width() {
    // Given: content wider than a narrow transcript surface.
    let theme = Theme::default();
    let rows = [
        "| Name | Value |",
        "| --- | --- |",
        "| 東京支社 | [👩‍💻 builds](https://example.com/builds) releases |",
    ];

    // When: the table is rendered in twelve cells.
    let (lines, _, links) =
        try_render_markdown_table_block(&rows, theme.text.primary, "", &theme, 12)
            .expect("markdown table");
    let text = rendered_text(&lines);

    // Then: the box remains bounded and wrapped rows retain both vertical edges.
    assert!(text.len() > 5, "narrow cells should wrap: {text:?}");
    assert!(text.iter().all(|line| display_width(line) <= 12));
    assert!(text.iter().all(|line| {
        (line.starts_with('┌') && line.ends_with('┐'))
            || (line.starts_with('├') && line.ends_with('┤'))
            || (line.starts_with('└') && line.ends_with('┘'))
            || (line.starts_with('│') && line.ends_with('│'))
    }));
    assert!(links.iter().all(|link| {
        link.start_cell > 0
            && link.end_cell < display_width(&text[link.row])
            && link.start_cell < link.end_cell
    }));
}
