use super::*;
fn row_json(row: &SelectionRow) -> serde_json::Value {
    serde_json::json!({"index":row.line_index,"text":row.text,"continued":row.continues_previous,"joiner":row.copy_joiner,"start":row.start_cell,"end":row.end_cell,"links":row.links.iter().map(|link|serde_json::json!([link.start_cell,link.end_cell,link.destination])).collect::<Vec<_>>()})
}
#[test]
fn diagnostic_compact_rows() {
    let sources = [
        "",
        " ",
        "  x  ",
        "界",
        "a界b",
        "界  b",
        "e\u{301}👩‍💻x",
        "#\u{fe0f} x",
        "\u{301}a",
        " \u{301} x",
        "a\t\r\nb",
        "a\u{200d}b",
        "👩\u{200d}💻",
        "👍🏽 x",
        "a\u{a0}b",
        "  α β  ",
    ];
    let mut records = Vec::new();
    for source in sources {
        let lines = [
            Line::from(source),
            Line::from(
                source
                    .chars()
                    .map(|ch| Span::raw(ch.to_string()))
                    .collect::<Vec<_>>(),
            ),
        ];
        for (split, line) in lines.iter().enumerate() {
            for width in (0..=12).chain([40]) {
                for rail in [None, Some(" "), Some("│"), Some("")] {
                    let rows = surface_selection_rows(
                        std::slice::from_ref(line),
                        width,
                        rail.is_some(),
                        rail.unwrap_or(" "),
                    );
                    records.push(serde_json::json!({"source":source,"split":split,"width":width,"rail":rail,"rows":rows.iter().map(row_json).collect::<Vec<_>>()}));
                }
                for alignment in [Alignment::Left, Alignment::Center, Alignment::Right] {
                    let snapshot = lifecycle_selection_snapshot(LifecycleSelectionSurface {
                        viewport: Rect::new(0, 0, width, 20),
                        text_rows: vec![crate::ui::ui_lifecycle::LifecycleSelectableText {
                            row: 0,
                            max_height: 20,
                            line: line.clone(),
                            alignment,
                        }],
                    })
                    .expect("positive height");
                    records.push(serde_json::json!({"source":source,"split":split,"width":width,"alignment":format!("{alignment:?}"),"rows":snapshot.rows.iter().map(row_json).collect::<Vec<_>>()}));
                }
            }
        }
    }
    std::fs::write(
        std::env::var("TUI_ROW_PARITY_OUT").expect("diagnostic output"),
        serde_json::to_vec(&records).expect("json"),
    )
    .expect("write");
}
