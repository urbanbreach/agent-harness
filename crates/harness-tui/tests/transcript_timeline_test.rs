use harness_tui::theme::{ColorLevel, Theme};
use harness_tui::theme_tokens::LifecycleState;
use harness_tui::transcript_identity::ReplayTurn;
use harness_tui::transcript_timeline::{MarkerInteraction, TimelineMarker, TimelineStatus};
use ratatui::style::Color;

#[allow(
    dead_code,
    reason = "shared journey fixture also covers other workflows"
)]
#[path = "support/rewrite_journey.rs"]
mod journey;

#[test]
fn timeline_markers_follow_active_terminal_color_level() {
    // arrange
    let marker = TimelineMarker::from_replay(
        ReplayTurn::event(11, 0, 1),
        TimelineStatus::Streaming,
        LifecycleState::Streaming,
    );

    // act
    for level in [
        ColorLevel::TrueColor,
        ColorLevel::Ansi256,
        ColorLevel::Basic,
        ColorLevel::None,
    ] {
        let theme = Theme::harness_dark().for_color_level(level);
        let style = marker.style(MarkerInteraction::Normal, &theme);
        match level {
            ColorLevel::TrueColor => {
                // assert
                assert!(matches!(style.foreground, Color::Rgb(..)));
                assert!(matches!(style.background, Color::Rgb(..)));
            }
            ColorLevel::Ansi256 => {
                assert!(matches!(style.foreground, Color::Indexed(_)));
                assert!(matches!(style.background, Color::Indexed(_)));
            }
            ColorLevel::Basic => {
                assert!(!matches!(
                    style.foreground,
                    Color::Rgb(..) | Color::Indexed(_)
                ));
                assert!(!matches!(
                    style.background,
                    Color::Rgb(..) | Color::Indexed(_)
                ));
            }
            ColorLevel::None => {
                assert_eq!(style.foreground, Color::Reset);
                assert_eq!(style.background, Color::Reset);
            }
        }
    }
}

#[test]
fn timeline_keys_select_failed_streaming_and_adjacent_messages_after_resize() -> journey::Result {
    let ascii = navigation_journey("ab")?;
    let joined_emoji = navigation_journey("👩‍💻")?;
    assert_eq!(
        ascii, joined_emoji,
        "equal display-cell widths must produce equal jump positions"
    );
    Ok(())
}

fn navigation_journey(cell_pair: &str) -> journey::Result<Vec<(u16, usize, usize)>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::layout::Rect;
    use serde_json::json;

    let mut scene = journey::Journey::new(false);
    let mut positions = Vec::new();
    let answer = format!(
        "```text\n{}{}\n```",
        "Recorded response\n".repeat(20),
        cell_pair.repeat(21)
    );
    for index in 0..3 {
        let request = format!("turn-{index}");
        scene.start(&request, &format!("Prompt {index}"))?;
        scene.finish(&request, &answer)?;
    }
    scene.start("failed", "Fail this request")?;
    scene.event("run_failed", json!({"error":"Synthetic failure"}))?;
    scene.start("tail", "Continue")?;
    scene.live(
        "provider_text_delta",
        json!({"request_id":"tail", "delta":"short"}),
    )?;
    scene.app.focus = harness_tui::app::Focus::Details;
    for area in [
        Rect::new(0, 0, 80, 24),
        Rect::new(0, 0, 40, 24),
        Rect::new(0, 0, 120, 40),
    ] {
        scene.app.set_frame_area(area);
        let _ = harness_tui::render_test::render_to_string(&scene.app, area, |app, frame, _| {
            harness_tui::ui::render_app(frame, app);
        });
        for (key, expected) in [(KeyCode::Char('f'), 3), (KeyCode::Char('s'), 4)] {
            scene.key(key, KeyModifiers::NONE);
            let buffer =
                harness_tui::render_test::render_to_buffer(&scene.app, area, |app, frame, _| {
                    harness_tui::ui::render_app(frame, app);
                });
            if let Some(path) = std::env::var_os("HARNESS_TUI_OUTLINE_FRAMES") {
                let path = std::path::PathBuf::from(path);
                std::fs::create_dir_all(&path)?;
                let case = if cell_pair.is_ascii() {
                    "ascii"
                } else {
                    "joined"
                };
                std::fs::write(
                    path.join(format!("{case}-{}-{expected}.json", area.width)),
                    serde_json::to_vec(&json!({"width":area.width, "height":area.height,
                        "text":harness_tui::render_test::buffer_to_string(&buffer, area.width),
                        "cells":buffer.content.iter().map(|cell| format!("{cell:?}")).collect::<Vec<_>>()
                    }))?,
                )?;
            }
            let state = scene.app.transcript_interaction_snapshot();
            assert_eq!(state.selected_activity_index, expected);
            assert!(!state.follow_mode);
            positions.push((area.width, expected, state.scroll));
        }
        assert!(scene.app.select_transcript_turn_at(0));
        for (key, expected) in [(KeyCode::Right, 1), (KeyCode::Left, 0)] {
            scene.key(key, KeyModifiers::SHIFT);
            assert_eq!(
                scene
                    .app
                    .transcript_interaction_snapshot()
                    .selected_activity_index,
                expected
            );
            let screen =
                harness_tui::render_test::render_to_string(&scene.app, area, |app, frame, _| {
                    harness_tui::ui::render_app(frame, app)
                });
            assert!(
                screen.contains(&format!("Harness {}/3", expected + 1)),
                "{screen}"
            );
        }
    }
    // A terminal can briefly have no transcript rows while output still arrives.
    scene.app.set_frame_area(Rect::new(0, 0, 80, 24));
    scene.app.set_frame_area(Rect::new(0, 0, 80, 0));
    scene.event("run_failed", json!({"error":"Synthetic tail failure"}))?;
    scene.app.set_frame_area(Rect::new(0, 0, 80, 24));
    scene.key(KeyCode::Char('f'), KeyModifiers::NONE);
    assert_eq!(
        scene
            .app
            .transcript_interaction_snapshot()
            .selected_activity_index,
        4,
        "the tail must retain its new failure status while the terminal has no transcript rows"
    );
    Ok(positions)
}
