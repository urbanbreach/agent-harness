use super::*;

#[test]
fn preferred_tool_bullet_stays_stable_across_lifecycle_states() {
    // arrange
    let theme = Theme::default();
    let cases = [
        (ToolCallPresentationStatus::Queued, "◆"),
        (ToolCallPresentationStatus::Running, "◆"),
        (ToolCallPresentationStatus::Waiting, "◆"),
        (ToolCallPresentationStatus::Succeeded, "◆"),
        (ToolCallPresentationStatus::Failed, "◆"),
        (ToolCallPresentationStatus::Cancelled, "◆"),
    ];

    // act
    let markers = cases.map(|(status, _)| completed_tool_marker(status, &theme));

    // assert
    assert_eq!(markers, cases.map(|(_, expected)| expected));
}

#[test]
fn completed_tool_marker_uses_ascii_lifecycle_fallbacks() {
    // arrange
    let theme = Theme::default().with_glyph_mode(crate::theme::GlyphMode::Ascii);
    let cases = [
        (ToolCallPresentationStatus::Queued, "."),
        (ToolCallPresentationStatus::Running, "o"),
        (ToolCallPresentationStatus::Waiting, "?"),
        (ToolCallPresentationStatus::Succeeded, "v"),
        (ToolCallPresentationStatus::Failed, "x"),
        (ToolCallPresentationStatus::Cancelled, "-"),
    ];

    // act
    let markers = cases.map(|(status, _)| completed_tool_marker(status, &theme));

    // assert
    assert_eq!(markers, cases.map(|(_, expected)| expected));
}

#[test]
fn generic_tool_header_omits_terminal_count_and_timing_metadata() {
    // arrange
    let header = TranscriptToolCallHeader {
        selected: false,
        tool_id: "edit".to_string(),
        title: "Edit".to_string(),
        subtitle: None,
        path_metadata: Some("src/main.rs".to_string()),
        icon: None,
        presentation: ToolCallPresentation {
            status: ToolCallPresentationStatus::Succeeded,
            duration_ms: Some(1_250),
            result_count: Some(7),
        },
        visual_style: TranscriptToolCallVisualStyle::Block,
        struck_out: false,
        disclosure_state: Some(TranscriptToolCallDisclosureState::Expanded),
    };

    // act
    let spans = build_tool_header_spans(
        &header,
        &Theme::default(),
        Style::default(),
        Style::default(),
        80,
    );
    let rendered = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    // assert
    assert!(rendered.contains("Edit src/main.rs"));
    assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(spans[3].style.fg, Some(Color::Rgb(255, 158, 100)));
    assert!(!rendered.contains("7 results"), "{rendered:?}");
    assert!(!rendered.contains("1.2s"), "{rendered:?}");
}
