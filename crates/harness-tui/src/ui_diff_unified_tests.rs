use super::*;
use crate::theme::ColorLevel;
use crate::UnwrapOrAbort;

fn unified_options() -> StructuredDiffRenderOptions {
    StructuredDiffRenderOptions {
        force_stacked: false,
        plain_numbered: false,
        highlight_intraline: false,
        highlight_syntax: false,
        show_file_header: false,
        show_hunk_header: false,
    }
}

fn render_plain_rows(diff: &str, width: u16, theme: &Theme) -> Vec<String> {
    render_structured_diff_lines_with_options(diff, None, "", width, unified_options(), theme)
        .unwrap_or_abort()
        .into_iter()
        .map(line_to_plain_text)
        .collect()
}

fn matching_rows<'a>(rows: &'a [String], needles: &[&str]) -> Vec<&'a str> {
    rows.iter()
        .filter(|row| needles.iter().any(|needle| row.contains(needle)))
        .map(String::as_str)
        .collect()
}

#[test]
fn renderer_keeps_delete_then_insert_as_separate_unified_rows_at_every_width() {
    // Given: one replacement rendered below and above the former split-pane threshold.
    let diff = "--- src/demo.rs\n+++ src/demo.rs\n@@ -7,1 +11,1 @@\n-let old_value = 1;\n+let new_value = 2;\n";

    for width in [72, 120] {
        // When: the same structured diff is rendered at this width.
        let rows = render_plain_rows(diff, width, &Theme::default());
        let changed = matching_rows(&rows, &["old_value", "new_value"]);

        // Then: delete and insert remain separate, ordered rows with shared geometry.
        assert_eq!(changed.len(), 2, "width {width} rendered {rows:#?}");
        assert!(changed[0].contains("old_value") && !changed[0].contains("new_value"));
        assert!(changed[1].contains("new_value") && !changed[1].contains("old_value"));
        assert_eq!(
            changed[0].find("let "),
            changed[1].find("let "),
            "width {width} must keep one content column: {changed:#?}"
        );
    }
}

#[test]
fn renderer_keeps_model_stats_and_hunk_offsets_width_independent() {
    // Given: two separated hunks with short rows that never need wrapping.
    let diff = "--- src/demo.rs\n+++ src/demo.rs\n@@ -1,2 +1,2 @@\n-old_one\n+new_one\n keep_one\n@@ -20,2 +20,2 @@\n-old_two\n+new_two\n keep_two\n";
    let render_offsets = |width| {
        render_structured_diff_lines_with_hunk_offsets(
            diff,
            None,
            "",
            width,
            unified_options(),
            &Theme::default(),
        )
        .unwrap_or_abort()
        .1
    };

    // When: model stats and offsets are projected at narrow and wide widths.
    let stats = structured_diff_stats(diff, None, false);
    let narrow_offsets = render_offsets(72);
    let wide_offsets = render_offsets(120);

    // Then: diff truth and hunk navigation do not depend on viewport width.
    assert_eq!(stats, Some((2, 2)));
    assert_eq!(narrow_offsets, wide_offsets);
}

#[test]
fn banded_rows_use_one_compact_number_gutter_without_redundant_markers() {
    // Given: a truecolor theme whose add/remove background bands are distinct.
    let diff = "--- src/demo.rs\n+++ src/demo.rs\n@@ -7,1 +11,1 @@\n-old_value\n+new_value\n";

    // When: the default diff variant renders the replacement.
    let rows = render_plain_rows(diff, 72, &Theme::default());
    let changed = matching_rows(&rows, &["old_value", "new_value"]);
    let removed_prefix = changed[0].split_once("old_value").unwrap_or_abort().0;
    let added_prefix = changed[1].split_once("new_value").unwrap_or_abort().0;

    // Then: each row has one natural-width number and no redundant +/- glyph.
    assert_eq!(
        removed_prefix
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>(),
        "7"
    );
    assert_eq!(
        added_prefix
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>(),
        "11"
    );
    assert!(!removed_prefix.contains('-'), "{removed_prefix:?}");
    assert!(!added_prefix.contains('+'), "{added_prefix:?}");
    assert!(removed_prefix.chars().count() <= 4, "{removed_prefix:?}");
    assert_eq!(removed_prefix.chars().count(), added_prefix.chars().count());
}

#[test]
fn bandless_rows_keep_foreground_and_glyph_fallbacks_legible() {
    // Given: ANSI16 and monochrome themes whose add/remove bands collapse together.
    let diff = "--- src/demo.rs\n+++ src/demo.rs\n@@ -1,1 +1,1 @@\n-old_value\n+new_value\n";
    let ansi_theme = Theme::harness_chat().for_color_level(ColorLevel::Basic);
    assert_eq!(
        diff_row_palette('-', &ansi_theme).content_bg,
        diff_row_palette('+', &ansi_theme).content_bg
    );

    // When: the foreground-only variant renders semantic rows.
    let ansi_lines = render_structured_diff_lines_with_options(
        diff,
        None,
        "",
        72,
        unified_options(),
        &ansi_theme,
    )
    .unwrap_or_abort();
    let removed = ansi_lines
        .iter()
        .find(|line| line_to_plain_text((*line).clone()).contains("old_value"))
        .unwrap_or_abort();
    let added = ansi_lines
        .iter()
        .find(|line| line_to_plain_text((*line).clone()).contains("new_value"))
        .unwrap_or_abort();
    let removed_marker = removed
        .spans
        .iter()
        .find(|span| span.content.trim() == "-")
        .unwrap_or_abort();
    let added_marker = added
        .spans
        .iter()
        .find(|span| span.content.trim() == "+")
        .unwrap_or_abort();

    // Then: foreground accents remain, with glyphs surviving monochrome mode too.
    assert_eq!(
        removed_marker.style.fg,
        Some(reference_diff_highlight_removed(&ansi_theme))
    );
    assert_eq!(
        added_marker.style.fg,
        Some(reference_diff_highlight_added(&ansi_theme))
    );
    let monochrome = render_plain_rows(
        diff,
        72,
        &Theme::harness_chat().for_color_level(ColorLevel::None),
    );
    let monochrome_changed = matching_rows(&monochrome, &["old_value", "new_value"]);
    assert!(monochrome_changed[0].contains('-'));
    assert!(monochrome_changed[1].contains('+'));
}

#[test]
fn long_rows_wrap_without_ellipsis_at_wide_and_narrow_widths() {
    // Given: replacement rows longer than either viewport's content column.
    let diff = concat!(
        "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n",
        "-removed content crosses every historical pane boundary and remains fully visible to the operator all the way through removed-narrow-shell-tail\n",
        "+added content crosses every historical pane boundary and remains fully visible to the operator all the way through added-narrow-shell-tail\n",
    );

    for width in [48, 120] {
        // When: the long replacement is rendered at this width.
        let theme = Theme::default();
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            width,
            unified_options(),
            &theme,
        )
        .unwrap_or_abort();
        let logical_text = |marker| {
            let background = diff_row_palette(marker, &theme).content_bg;
            lines
                .iter()
                .filter(|line| {
                    line.spans
                        .iter()
                        .any(|span| span.style.bg == Some(background))
                })
                .flat_map(|line| line.spans.iter().skip(3))
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        let removed = logical_text('-');
        let added = logical_text('+');
        let rows = lines
            .into_iter()
            .map(line_to_plain_text)
            .collect::<Vec<_>>();

        // Then: continuation rows retain the tails and never substitute an ellipsis.
        assert!(
            removed.ends_with("removed-narrow-shell-tail"),
            "{removed:?}"
        );
        assert!(added.ends_with("added-narrow-shell-tail"), "{added:?}");
        assert!(
            rows.iter().all(|row| !row.contains('…')),
            "width {width} rendered {rows:#?}"
        );
    }
}

#[test]
fn unchanged_gap_label_uses_the_same_compact_unified_content_column() {
    // Given: separated hunks that produce an unchanged-line label.
    let diff = "--- src/demo.rs\n+++ src/demo.rs\n@@ -1,1 +1,1 @@\n-old_one\n+new_one\n@@ -20,1 +20,1 @@\n-old_two\n+new_two\n";

    // When: the diff renders at a formerly side-by-side width.
    let rows = render_plain_rows(diff, 120, &Theme::default());
    let gap = rows
        .iter()
        .find(|row| row.contains("unchanged lines"))
        .unwrap_or_abort();
    let prefix = gap.split_once('…').unwrap_or_abort().0;

    // Then: the label aligns after one compact, decorative number gutter.
    assert!(prefix.chars().count() <= 4, "{gap:?}");
    assert!(!prefix.chars().any(|ch| ch.is_ascii_digit()));
    assert!(!prefix.contains(['-', '+']));
}

#[test]
fn each_hunk_sizes_its_number_gutter_from_its_own_line_range() {
    // Given: a one-digit hunk followed by a separate three-digit hunk.
    let diff = concat!(
        "--- src/demo.rs\n+++ src/demo.rs\n",
        "@@ -7,1 +7,1 @@\n-old_seven\n+new_seven\n",
        "@@ -100,1 +100,1 @@\n-old_hundred\n+new_hundred\n",
    );

    // When: both hunks render through the same file-level model.
    let rows = render_plain_rows(diff, 72, &Theme::default());
    let first = rows
        .iter()
        .find(|row| row.contains("old_seven"))
        .unwrap_or_abort();
    let second = rows
        .iter()
        .find(|row| row.contains("old_hundred"))
        .unwrap_or_abort();

    // Then: each hunk uses the natural digit width of its own line range.
    assert_eq!(first.find("old_seven"), Some(3), "{rows:#?}");
    assert_eq!(second.find("old_hundred"), Some(5), "{rows:#?}");
}
