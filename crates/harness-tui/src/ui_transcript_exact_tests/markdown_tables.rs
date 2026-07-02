use super::super::*;

#[cfg(test)]
pub(crate) fn exact_test_markdown_tables_match_reference_top_level_columns() {
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-markdown-table",
            ActivityStatus::Done,
            "| Name | Age |\n|---|---|\n| Alice | 30 |\n| Bob | 5 |",
        )]);

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let rendered = lines.join("\n");

    assert!(
        rendered.contains("Name   Age"),
        "table header should render as borderless content-width columns\n{rendered}"
    );
    assert!(
        rendered.contains("Alice  30"),
        "table body should align content-width columns\n{rendered}"
    );
    assert!(
        !rendered.contains("| Name | Age |"),
        "markdown table source rows should not render as raw pipe text\n{rendered}"
    );
    assert!(
        !rendered.contains('┌'),
        "top-level markdown tables should be borderless like the reference transcript\n{rendered}"
    );
    if std::env::var_os("HARNESS_TUI_MARKDOWN_TABLE_RENDER_CAPTURE").is_some() {
        println!("# Markdown table parity\n{rendered}");
    }
}

#[cfg(test)]
pub(crate) fn exact_test_markdown_table_selection_matches_rendered_rows() {
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-markdown-table-selection",
            ActivityStatus::Done,
            "| Name | Age |\n|---|---|\n| Alice | 30 |\n| Bob | 5 |",
        )]);

    let snapshot =
        transcript_selection_debug_snapshot(&app, ratatui::layout::Rect::new(0, 0, 96, 24))
            .expect("selection snapshot");
    let rendered = snapshot.rows.join("\n");

    assert!(
        rendered.contains("Name   Age"),
        "selection rows should mirror rendered table header\n{rendered}"
    );
    assert!(
        rendered.contains("Alice  30"),
        "selection rows should mirror rendered table body\n{rendered}"
    );
    assert!(
        rendered.contains("Bob    5"),
        "selection rows should mirror padded rendered table body\n{rendered}"
    );
    assert!(
        !rendered.contains("|---|---|"),
        "selection rows should not expose the raw markdown separator\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_markdown_tables_render_inline_links_code_alignment_and_cjk_width() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
        "request-rich-markdown-table",
        ActivityStatus::Done,
        "| 항목 | Link | Code | State |\n|:---|---:|:---:|---|\n| 모델 | [Docs](https://example.com/docs) | `spawn()` | 완료 |\n| A | **Bold** | `x` | 대기 |",
    )]);

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let rendered = lines.join("\n");

    assert!(
        rendered.contains("항목"),
        "CJK headers should render as table cells\n{rendered}"
    );
    assert!(
        rendered.contains("모델  Docs  spawn()  완료"),
        "inline links/code/emphasis should render concealed markdown text with CJK-width padding\n{rendered}"
    );
    assert!(
        rendered.contains("A     Bold  x        대기"),
        "ASCII rows should pad against preceding CJK column display width\n{rendered}"
    );
    assert!(
        !rendered.contains("https://example.com/docs"),
        "markdown links should render the label, not raw href text\n{rendered}"
    );
    assert!(
        !rendered.contains("|:---"),
        "alignment separator rows should not render as raw pipe text\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_markdown_table_rich_selection_matches_rendered_rows() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
        "request-rich-markdown-table-selection",
        ActivityStatus::Done,
        "| 항목 | Link | Code | State |\n|:---|---:|:---:|---|\n| 모델 | [Docs](https://example.com/docs) | `spawn()` | 완료 |\n| A | **Bold** | `x` | 대기 |",
    )]);

    let snapshot =
        transcript_selection_debug_snapshot(&app, ratatui::layout::Rect::new(0, 0, 96, 24))
            .expect("selection snapshot");
    let rendered = snapshot.rows.join("\n");

    assert!(
        rendered.contains("모델  Docs  spawn()  완료"),
        "selection rows should mirror rendered CJK table rows\n{rendered}"
    );
    assert!(
        rendered.contains("A     Bold  x        대기"),
        "selection rows should mirror rendered ASCII/CJK padding\n{rendered}"
    );
    assert!(
        !rendered.contains("https://example.com/docs"),
        "selection rows should not expose raw markdown href text\n{rendered}"
    );
}
