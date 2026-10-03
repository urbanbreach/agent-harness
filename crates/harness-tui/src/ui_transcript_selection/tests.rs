use super::*;

#[test]
fn fenced_selection_rows_preserve_logical_code_lines_through_rewrap() {
    // arrange
    // Given: a fenced assistant body whose code line wraps at a narrow width.
    let Some(rows) = selection_rows_for_rich_text_block(
        "```rust\nlet value = a_very_long_identifier;\n```",
        Color::White,
        "  ",
        &Theme::default(),
        16,
        false,
    ) else {
        panic!("ordinary code must remain selectable");
    };

    // When: the selection model records the rendered rows.
    let rendered = rows
        .iter()
        .map(|row| row.text.clone())
        .collect::<Vec<_>>()
        .join("\n");

    // act
    // Then: only painted code is selectable and wrapped rows retain one logical line.
    // assert
    assert!(!rendered.contains("```rust"));
    assert!(rendered.contains("let value"));
    assert!(rows.iter().any(|row| row.continues_previous));
}

#[test]
fn open_fence_selection_rows_match_streaming_code_body() {
    // arrange
    // act
    let Some(rows) = selection_rows_for_rich_text_block(
        "Before\n```rust\nlet value = 42;",
        Color::White,
        "  ",
        &Theme::default(),
        24,
        true,
    ) else {
        panic!("streaming code must remain selectable");
    };
    let rendered = rows
        .iter()
        .map(|row| row.text.clone())
        .collect::<Vec<_>>()
        .join("\n");

    // assert
    assert!(rendered.contains("Before"));
    assert!(rendered.contains("let value = 42;"));
    assert!(!rendered.contains("```rust"));
}

#[test]
fn markdown_selection_copy_retains_safe_destination_metadata() {
    // Given: a production markdown selection row containing a labeled link.
    let rows = selection_rows_for_markdownish_text_block(
        "Read [docs](https://example.com/docs)",
        Color::White,
        "  ",
        &Theme::default(),
        40,
    );
    let compact = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let mut row = row.clone();
            row.line_index = index;
            row
        })
        .collect::<Vec<_>>();
    let snapshot = TranscriptSelectionSnapshot {
        viewport: Rect::new(0, 0, 40, 1),
        visible_rows: vec![Some(0)],
        rows: compact,
        total_rows: 1,
        row_width: 40,
        resolved_selection: Some(TranscriptSelection {
            anchor: TranscriptSelectionCell { row: 0, column: 2 },
            focus: TranscriptSelectionCell { row: 0, column: 10 },
        }),
    };

    // When: selected cells are copied.
    let copied = snapshot
        .selection_text_with_destinations(TranscriptSelection {
            anchor: TranscriptSelectionCell { row: 0, column: 2 },
            focus: TranscriptSelectionCell { row: 0, column: 10 },
        })
        .expect("selected text");

    // Then: visible text and the safe destination survive together.
    assert_eq!(copied, "Read docs\n\nLinks:\nhttps://example.com/docs");
}

#[test]
fn destination_export_includes_only_half_open_runs_intersecting_selection() {
    // Given: one safe link occupying display cells [3, 7).
    let mut row = selection_rows_for_rendered_line(&Line::from("aa link zz"), 10).remove(0);
    row.links.push(TranscriptSelectionLink {
        continues_previous: false,
        start_cell: 3,
        end_cell: 7,
        destination: "https://example.com/link".into(),
    });
    let snapshot_for = |anchor, focus| TranscriptSelectionSnapshot {
        viewport: Rect::new(0, 0, 10, 1),
        visible_rows: vec![Some(0)],
        rows: vec![row.clone()],
        total_rows: 1,
        row_width: 10,
        resolved_selection: Some(TranscriptSelection {
            anchor: TranscriptSelectionCell {
                row: 0,
                column: anchor,
            },
            focus: TranscriptSelectionCell {
                row: 0,
                column: focus,
            },
        }),
    };

    // When: selections land before, after, and on the final linked cell.
    let before = snapshot_for(0, 1)
        .selection_text_with_destinations(TranscriptSelection {
            anchor: TranscriptSelectionCell { row: 0, column: 0 },
            focus: TranscriptSelectionCell { row: 0, column: 1 },
        })
        .expect("before text");
    let after = snapshot_for(8, 9)
        .selection_text_with_destinations(TranscriptSelection {
            anchor: TranscriptSelectionCell { row: 0, column: 8 },
            focus: TranscriptSelectionCell { row: 0, column: 9 },
        })
        .expect("after text");
    let boundary = snapshot_for(6, 7)
        .selection_text_with_destinations(TranscriptSelection {
            anchor: TranscriptSelectionCell { row: 0, column: 6 },
            focus: TranscriptSelectionCell { row: 0, column: 7 },
        })
        .expect("boundary text");

    // Then: only the exact half-open overlap exports the destination.
    assert!(!before.contains("Links:") && !after.contains("Links:"));
    assert!(boundary.ends_with("Links:\nhttps://example.com/link"));
}

#[test]
fn inline_link_ranges_survive_repeated_labels_wrapping_and_wide_graphemes() {
    // Given: duplicate labels, whitespace, CJK, joined emoji and emoji presentation.
    let rows = selection_rows_for_markdownish_text_block(
        "same [same](https://example.com/one) 👩‍💻中#\u{fe0f} [same](https://example.com/two) [two words](https://example.com/words)",
        Color::White,
        "",
        &Theme::default(),
        7,
    );

    // When: rendered row-local link runs are inspected.
    let links = rows
        .iter()
        .flat_map(|row| row.links.iter().map(move |link| (row, link)))
        .collect::<Vec<_>>();

    // Then: each URL has its own exact non-empty run, including both words when wrapped.
    assert_eq!(
        links
            .iter()
            .map(|(_, link)| link.destination.as_str())
            .collect::<Vec<_>>(),
        vec![
            "https://example.com/one",
            "https://example.com/two",
            "https://example.com/words",
            "https://example.com/words",
        ]
    );
    for (row, link) in links {
        assert!(link.start_cell < link.end_cell);
        assert!(link.end_cell <= row.width);
        let label = extract_text_by_display_columns(&row.text, link.start_cell, link.end_cell - 1);
        assert!(matches!(label.as_str(), "same" | "two " | "words"));
    }
}

#[test]
fn streaming_and_settled_rows_preserve_link_metadata_before_open_fence() {
    // Given: visible linked prose before an unfinished code fence.
    let theme = Theme::default();
    let streaming = selection_rows_for_rich_text_block(
        "See [docs](https://example.com/docs)\n```rust\nfn main() {}",
        Color::White,
        "  ",
        &theme,
        40,
        true,
    )
    .expect("streaming rows");

    // When: the closing fence settles the same document.
    let settled = selection_rows_for_rich_text_block(
        "See [docs](https://example.com/docs)\n```rust\nfn main() {}\n```",
        Color::White,
        "  ",
        &theme,
        40,
        false,
    )
    .expect("settled rows");

    // Then: already-visible link geometry and destination stay stable.
    assert_eq!(streaming[0].links, settled[0].links);
    assert_eq!(
        streaming[0].links[0].destination,
        "https://example.com/docs"
    );
}

#[test]
fn transformed_fences_fail_closed_for_semantic_selection() {
    // arrange
    // act
    for source in [
        "```mermaid\ngraph TD\nA --> B\n```",
        "```diff\n-old\n+new\n```",
    ] {
        // assert
        assert!(selection_rows_for_rich_text_block(
            source,
            Color::White,
            "  ",
            &Theme::default(),
            40,
            false,
        )
        .is_none());
    }
}

#[test]
fn unresolved_semantic_selection_does_not_fall_back_to_stale_cells() {
    // arrange
    // Given: a snapshot whose anchored surface disappeared during reflow.
    let snapshot = TranscriptSelectionSnapshot {
        viewport: Rect::new(0, 0, 5, 1),
        visible_rows: vec![Some(0)],
        rows: vec![SelectionRow {
            line_index: 0,
            text: "stale".into(),
            width: 5,
            continues_previous: false,
            copy_joiner: None,
            start_cell: 0,
            end_cell: 4,
            links: Vec::new(),
        }],
        total_rows: 1,
        row_width: 5,
        resolved_selection: None,
    };
    let stale_selection = TranscriptSelection {
        anchor: TranscriptSelectionCell { row: 0, column: 0 },
        focus: TranscriptSelectionCell { row: 0, column: 4 },
    };

    // When: copy resolves selection text from the reflowed snapshot.
    let text = snapshot.selection_text(stale_selection);

    // act
    // Then: unresolved semantic endpoints fail closed instead of selecting new content.
    // assert
    assert_eq!(text, None);
}
