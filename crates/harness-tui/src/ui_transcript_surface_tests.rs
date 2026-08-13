use ratatui::{
    style::{Color, Style},
    text::Span,
};

use super::ui_transcript_surface::{
    append_user_surface_text_block_with_first_line_reserve, append_user_surface_wrapped_line,
    line_has_tool_rail, tool_finish_flash_brightness, wave_brightness,
};
use std::time::Duration;

#[test]
fn running_tool_wave_is_spatially_continuous_and_time_based() {
    let at_start = (0..6)
        .map(|row| wave_brightness(Duration::ZERO, row, 32))
        .collect::<Vec<_>>();
    let after_skipped_frames = (0..6)
        .map(|row| wave_brightness(Duration::from_millis(160), row, 32))
        .collect::<Vec<_>>();

    assert!(at_start
        .windows(2)
        .all(|pair| (pair[0] - pair[1]).abs() > f32::EPSILON));
    assert_ne!(at_start, after_skipped_frames);
    assert!(at_start
        .iter()
        .chain(&after_skipped_frames)
        .all(|brightness| (0.0..=1.0).contains(brightness)));
}

#[test]
fn finish_flash_decays_by_elapsed_time_and_ends_at_four_hundred_ms() {
    let start = tool_finish_flash_brightness(Duration::ZERO);
    let middle = tool_finish_flash_brightness(Duration::from_millis(200));
    let finished = tool_finish_flash_brightness(Duration::from_millis(400));

    assert!(start > middle);
    assert!(middle > finished);
    assert!(finished.abs() <= f32::EPSILON);
}

#[test]
fn tool_rail_motion_stops_before_group_spacers_and_assistant_footer() {
    let group_header = ratatui::text::Line::from(Span::raw("┃  ◇ Ran 2 commands"));
    let member = ratatui::text::Line::from(Span::raw("┃  ◆ Run cargo test"));
    let spacer = ratatui::text::Line::default();
    let footer = ratatui::text::Line::from(Span::raw("◇ model-tx"));

    assert!(line_has_tool_rail(&group_header, "┃"));
    assert!(line_has_tool_rail(&member, "┃"));
    assert!(!line_has_tool_rail(&spacer, "┃"));
    assert!(!line_has_tool_rail(&footer, "┃"));
}

#[test]
fn first_line_reserve_preserves_space_across_rewrapped_rows() {
    let mut lines = Vec::new();

    append_user_surface_text_block_with_first_line_reserve(
        &mut lines,
        "first 123456789012345 next",
        Color::White,
        "",
        20,
        Color::Black,
        5,
    );

    let rendered = lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["first ", "123456789012345 next"]);
}

#[test]
fn first_line_reserve_preserves_cjk_combining_and_emoji_graphemes() {
    let mut lines = Vec::new();

    append_user_surface_text_block_with_first_line_reserve(
        &mut lines,
        "first 한글e\u{301}👩‍💻 next",
        Color::White,
        "",
        14,
        Color::Black,
        8,
    );

    let rendered = lines
        .iter()
        .map(|line| {
            (
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>(),
                line.width(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rendered,
        vec![
            ("first ".to_string(), 6),
            ("한글e\u{301}👩‍💻 next".to_string(), 12),
        ]
    );
}

#[test]
fn first_line_reserve_preserves_styles_across_rewrapped_spans() {
    let mut lines = Vec::new();

    append_user_surface_wrapped_line(
        &mut lines,
        vec![
            Span::styled("first ", Style::default().fg(Color::Red)),
            Span::styled("123456789012345", Style::default().fg(Color::Green)),
            Span::styled(" next", Style::default().fg(Color::Blue)),
        ],
        "",
        Style::default(),
        20,
        Color::Black,
        5,
    );

    let rendered = lines[1]
        .spans
        .iter()
        .filter(|span| !span.content.is_empty())
        .map(|span| (span.content.to_string(), span.style.fg))
        .collect::<Vec<_>>();
    assert_eq!(
        rendered,
        vec![
            ("123456789012345".to_string(), Some(Color::Green)),
            (" ".to_string(), Some(Color::Blue)),
            ("next".to_string(), Some(Color::Blue)),
        ]
    );
}

#[test]
fn first_line_reserve_consumes_styled_leading_whitespace_once() {
    let mut lines = Vec::new();

    append_user_surface_wrapped_line(
        &mut lines,
        vec![
            Span::styled("  first", Style::default().fg(Color::Red)),
            Span::styled(" second", Style::default().fg(Color::Green)),
        ],
        "",
        Style::default(),
        10,
        Color::Black,
        5,
    );

    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["first", "second"]);
    assert_eq!(lines[1].spans[1].style.fg, Some(Color::Green));
}

#[test]
fn first_line_reserve_never_splits_a_zwj_grapheme() {
    let mut lines = Vec::new();

    append_user_surface_text_block_with_first_line_reserve(
        &mut lines,
        "👩‍💻x",
        Color::White,
        "",
        4,
        Color::Black,
        2,
    );

    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["👩‍💻", "x"]);
}
