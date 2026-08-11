#[cfg(test)]
use crate::UnwrapOrAbort;
use ratatui::style::Color;
use ratatui::text::Line;

use crate::theme::Theme;

#[path = "ui_diff_model.rs"]
mod ui_diff_model;
#[path = "ui_diff_render.rs"]
mod ui_diff_render;
#[path = "ui_diff_syntax.rs"]
pub(super) mod ui_diff_syntax;

use ui_diff_model::{structured_diff_model_from_patch, structured_diff_stats_from_patch};
use ui_diff_render::render_structured_diff_model;

#[cfg(test)]
use ui_diff_model::DiffSegmentKind;
#[cfg(test)]
use ui_diff_render::{
    diff_hunk_palette, diff_marker_style, diff_row_palette, diff_segment_style,
    reference_diff_added_bg, reference_diff_added_line_number_bg, reference_diff_highlight_added,
    reference_diff_highlight_removed, reference_diff_hunk_header, reference_diff_removed_bg,
    reference_diff_removed_line_number_bg, render_diff_hunk_header,
};
#[cfg(test)]
use ui_diff_syntax::{diff_path_is_plain_prose, highlight_diff_line_chunks};

#[derive(Debug, Clone, Copy)]
pub(super) struct StructuredDiffRenderOptions {
    pub force_stacked: bool,
    pub plain_numbered: bool,
    pub highlight_intraline: bool,
    pub highlight_syntax: bool,
    pub show_file_header: bool,
    pub show_hunk_header: bool,
}

pub(super) fn render_structured_diff_lines(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    force_stacked: bool,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    render_structured_diff_lines_with_options(
        diff_content,
        fallback_path,
        prefix,
        width,
        StructuredDiffRenderOptions {
            force_stacked,
            plain_numbered: false,
            highlight_intraline: true,
            highlight_syntax: false,
            show_file_header: true,
            show_hunk_header: true,
        },
        theme,
    )
}

pub(super) fn render_structured_diff_lines_with_options(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    options: StructuredDiffRenderOptions,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    render_structured_diff_lines_with_hunk_offsets(
        diff_content,
        fallback_path,
        prefix,
        width,
        options,
        theme,
    )
    .map(|(lines, _)| lines)
}

pub(super) fn render_structured_diff_lines_with_hunk_offsets(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    options: StructuredDiffRenderOptions,
    theme: &Theme,
) -> Option<(Vec<Line<'static>>, Vec<usize>)> {
    let model =
        structured_diff_model_from_patch(diff_content, fallback_path, options.highlight_intraline)?;
    Some(render_structured_diff_model(
        &model,
        prefix,
        width,
        options.force_stacked,
        options.plain_numbered,
        options.highlight_syntax,
        options.show_file_header,
        options.show_hunk_header,
        theme,
    ))
}

pub(crate) fn structured_diff_stats(
    diff_content: &str,
    fallback_path: Option<&str>,
    highlight_intraline: bool,
) -> Option<(usize, usize)> {
    structured_diff_stats_from_patch(diff_content, fallback_path, highlight_intraline)
}

#[cfg(test)]
fn line_to_plain_text(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn structured_diff_rows_respect_display_width_for_wide_glyphs() {
        // arrange
        // act
        // assert
        let diff = "--- demo.txt\n+++ demo.txt\n@@ -1,2 +1,2 @@\n-漢字🙂漢字🙂漢字🙂\n+🙂漢字🙂漢字🙂漢字\n";
        let lines = render_structured_diff_lines(diff, None, "", 24, false, &Theme::default())
            .unwrap_or_abort();

        assert!(
            lines.iter().all(|line| line.width() <= 24),
            "rendered diff rows should honor visible width: {:#?}",
            lines
                .iter()
                .map(|line| line_to_plain_text(line.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn stacked_diff_text_spans_keep_row_backgrounds() {
        // arrange
        // act
        // assert
        let diff = "--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n";
        let theme = Theme::default();
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            80,
            StructuredDiffRenderOptions {
                force_stacked: true,
                plain_numbered: false,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &theme,
        )
        .unwrap_or_abort();

        let context_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("alpha"))
            .unwrap_or_abort();
        let removed_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("beta"))
            .unwrap_or_abort();
        let added_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("BETA"))
            .unwrap_or_abort();

        let context_span = context_line
            .spans
            .iter()
            .find(|span| span.content.contains("alpha"))
            .unwrap_or_abort();
        let removed_span = removed_line
            .spans
            .iter()
            .find(|span| span.content.contains("beta"))
            .unwrap_or_abort();
        let added_span = added_line
            .spans
            .iter()
            .find(|span| span.content.contains("BETA"))
            .unwrap_or_abort();

        assert_eq!(context_span.style.bg, Some(theme.surface.panel));
        assert_eq!(
            removed_span.style.bg,
            Some(diff_row_palette('-', &theme).content_bg)
        );
        assert_eq!(
            added_span.style.bg,
            Some(diff_row_palette('+', &theme).content_bg)
        );
    }

    #[test]
    fn structured_diff_palette_matches_reference_inline_diff_colors() {
        // arrange
        // act
        // assert
        let theme = Theme::default();

        assert_eq!(
            diff_row_palette('+', &theme).content_bg,
            reference_diff_added_bg(&theme)
        );
        assert_eq!(
            diff_row_palette('+', &theme).gutter_bg,
            reference_diff_added_line_number_bg(&theme)
        );
        assert_eq!(
            diff_row_palette('-', &theme).content_bg,
            reference_diff_removed_bg(&theme)
        );
        assert_eq!(
            diff_row_palette('-', &theme).gutter_bg,
            reference_diff_removed_line_number_bg(&theme)
        );
        assert_eq!(diff_hunk_palette(&theme).content_bg, theme.surface.panel);
        assert_eq!(
            diff_marker_style('+', None, &theme).fg,
            Some(reference_diff_highlight_added(&theme))
        );
        assert_eq!(
            diff_marker_style('-', None, &theme).fg,
            Some(reference_diff_highlight_removed(&theme))
        );
        assert_eq!(
            diff_segment_style(
                DiffSegmentKind::Added,
                DiffSegmentKind::Removed,
                None,
                &theme
            )
            .fg,
            Some(reference_diff_highlight_added(&theme))
        );
        assert_eq!(
            diff_segment_style(
                DiffSegmentKind::Removed,
                DiffSegmentKind::Added,
                None,
                &theme
            )
            .fg,
            Some(reference_diff_highlight_removed(&theme))
        );

        let hunk_header = render_diff_hunk_header("", "@@ -1,1 +1,1 @@", 48, 2, &theme);
        let hunk_span = hunk_header
            .spans
            .iter()
            .find(|span| span.content.contains("@@ -1,1 +1,1 @@"))
            .unwrap_or_abort();
        assert_eq!(hunk_span.style.fg, Some(reference_diff_hunk_header(&theme)));
        assert_eq!(hunk_span.style.bg, Some(theme.surface.panel));
    }

    #[test]
    fn structured_diff_syntax_highlighting_uses_reference_token_colors() {
        // arrange
        // act
        // assert
        let chunks = highlight_diff_line_chunks(
            Some("src/demo.rs"),
            "let value = \"hi\"; let total = 42; // note",
            Some(reference_diff_added_bg(&Theme::default())),
            crate::theme::ColorLevel::TrueColor,
        )
        .unwrap_or_abort();

        let find_chunk = |needle: &str| {
            chunks
                .iter()
                .find(|chunk| chunk.text.contains(needle))
                .unwrap_or_else(|| panic!("missing chunk containing {needle:?}: {chunks:#?}"))
        };

        assert_eq!(
            find_chunk("hi").style.fg,
            Some(Color::Rgb(0x7F, 0xD8, 0x8F))
        );
        assert_eq!(
            find_chunk("42").style.fg,
            Some(Color::Rgb(0xE5, 0xC0, 0x7B))
        );
        assert_eq!(
            find_chunk("note").style.fg,
            Some(Color::Rgb(0x80, 0x80, 0x80))
        );
        assert_eq!(
            find_chunk("note").style.bg,
            Some(reference_diff_added_bg(&Theme::default()))
        );
    }

    #[test]
    fn prose_diff_paths_skip_syntax_highlighting_fast_path() {
        // arrange
        let prose_paths = [
            "README.md",
            "docs/guide.markdown",
            "notes.MDOWN",
            "changelog.mkd",
            "manual.rst",
            "plain.text",
            "message.txt",
            "guide.adoc",
            "guide.asciidoc",
        ];
        let syntax_paths = ["src/lib.rs", "script.py", "Makefile"];

        // act
        let prose_results = prose_paths.map(|path| {
            (
                path,
                diff_path_is_plain_prose(path),
                highlight_diff_line_chunks(
                    Some(path),
                    "# heading",
                    Some(reference_diff_added_bg(&Theme::default())),
                    crate::theme::ColorLevel::TrueColor,
                )
                .is_none(),
            )
        });
        let syntax_results = syntax_paths.map(|path| (path, diff_path_is_plain_prose(path)));
        let extensionless_result = diff_path_is_plain_prose("README");

        // assert
        for (path, is_plain_prose, skips_highlighting) in prose_results {
            assert!(is_plain_prose, "{path} should be treated as prose");
            assert!(
                skips_highlighting,
                "{path} should not initialize syntect syntax highlighting"
            );
        }

        for (path, is_plain_prose) in syntax_results {
            assert!(!is_plain_prose, "{path} should keep normal syntax handling");
        }
        assert!(!extensionless_result);
    }

    #[test]
    fn structured_diff_syntax_respects_limited_color_levels() {
        let chunks = highlight_diff_line_chunks(
            Some("src/demo.rs"),
            "let value = 42;",
            Some(
                Theme::harness_chat()
                    .for_color_level(crate::theme::ColorLevel::Ansi256)
                    .surface
                    .panel,
            ),
            crate::theme::ColorLevel::Ansi256,
        )
        .unwrap_or_abort();

        assert!(chunks
            .iter()
            .all(|chunk| !matches!(chunk.style.fg, Some(Color::Rgb(_, _, _)))));
        assert!(chunks
            .iter()
            .all(|chunk| !matches!(chunk.style.bg, Some(Color::Rgb(_, _, _)))));
    }
    #[test]
    fn separated_hunks_render_truthful_unchanged_line_marker() {
        let diff = "--- src/demo.rs\n+++ src/demo.rs\n@@ -1,2 +1,2 @@\n-old_one\n+new_one\n keep_one\n@@ -20,2 +20,2 @@\n-old_two\n+new_two\n keep_two\n";
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            72,
            StructuredDiffRenderOptions {
                force_stacked: true,
                plain_numbered: false,
                highlight_intraline: true,
                highlight_syntax: true,
                show_file_header: true,
                show_hunk_header: false,
            },
            &Theme::default(),
        )
        .unwrap_or_abort();
        let text = lines
            .into_iter()
            .map(line_to_plain_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("… 17 unchanged lines"), "{text}");
    }

    #[test]
    fn highlight_upgrade_preserves_text_and_row_geometry() {
        let diff = "--- src/demo.rs\n+++ src/demo.rs\n@@ -1,2 +1,2 @@\n-fn old() { let value = 1; }\n+fn new() { let value = 2; }\n context();\n";
        let render = |highlight_syntax| {
            render_structured_diff_lines_with_options(
                diff,
                None,
                "",
                64,
                StructuredDiffRenderOptions {
                    force_stacked: true,
                    plain_numbered: false,
                    highlight_intraline: true,
                    highlight_syntax,
                    show_file_header: true,
                    show_hunk_header: false,
                },
                &Theme::default(),
            )
            .unwrap_or_abort()
        };
        let local = render(false);
        let upgraded = render(true);
        assert_eq!(local.len(), upgraded.len());
        assert_eq!(
            local
                .into_iter()
                .map(line_to_plain_text)
                .collect::<Vec<_>>(),
            upgraded
                .into_iter()
                .map(line_to_plain_text)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn structured_diff_headers_surface_rename_paths() {
        // arrange
        // act
        // assert
        let diff = "--- src/old_name.rs\n+++ src/new_name.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            96,
            StructuredDiffRenderOptions {
                force_stacked: true,
                plain_numbered: false,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &Theme::default(),
        )
        .unwrap_or_abort();
        let header = lines
            .iter()
            .map(|line| line_to_plain_text(line.clone()))
            .find(|line| line.contains("old_name.rs") && line.contains("new_name.rs"))
            .unwrap_or_abort();

        assert!(
            header.contains("→"),
            "header should surface rename arrow: {header}"
        );
    }

    #[test]
    fn stacked_diff_long_rows_wrap_instead_of_truncating() {
        // arrange
        // act
        // assert
        let diff = "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n-session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows\n+session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells\n";
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            84,
            StructuredDiffRenderOptions {
                force_stacked: true,
                plain_numbered: false,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &Theme::default(),
        )
        .unwrap_or_abort();
        let rendered = lines
            .iter()
            .map(|line| line_to_plain_text(line.clone()))
            .collect::<Vec<_>>();
        let collect_stacked_cell_text = |rows: &[String], marker: char| {
            let marker_token = format!("{marker} ");
            let start = rows
                .iter()
                .position(|line| line.contains(&marker_token))
                .unwrap_or_else(|| panic!("missing {marker} row marker: {rows:#?}"));
            let marker_column = rows[start]
                .find(&marker_token)
                .unwrap_or_else(|| panic!("missing {marker} marker column: {rows:#?}"));
            let text_column = marker_column + marker_token.len();
            let mut chunks = Vec::new();

            for line in &rows[start..] {
                let marker_cell = line.get(marker_column..text_column).unwrap_or("");
                let Some(text) = line.get(text_column..) else {
                    break;
                };
                let text = text.trim_end();

                if marker_cell == marker_token {
                    chunks.push(text.to_string());
                    continue;
                }

                if marker_cell == "  " && !text.is_empty() {
                    chunks.push(text.to_string());
                    continue;
                }

                break;
            }

            chunks.concat()
        };
        let removed_text = collect_stacked_cell_text(&rendered, '-');
        let added_text = collect_stacked_cell_text(&rendered, '+');

        assert!(
            removed_text
                == "session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows",
            "removed continuation should preserve the full text across wrapped rows: {rendered:#?}"
        );
        assert!(
            added_text
                == "session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells",
            "added continuation should preserve the full text across wrapped rows: {rendered:#?}"
        );
        assert!(
            rendered.iter().all(|line| !line.contains('…')),
            "stacked renderer should keep the full text without ellipsis: {rendered:#?}"
        );
    }
}
