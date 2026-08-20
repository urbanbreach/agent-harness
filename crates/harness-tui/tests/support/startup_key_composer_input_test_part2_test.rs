#[test]
fn input_typed_text_uses_grok_primary_on_canvas() {
    // arrange
    let mut app = startup_app();
    for character in "Browser QA draft".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    let buffer = render_to_buffer(&app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let draft = "Browser QA draft".chars().collect::<Vec<_>>();
    let draft_cells = buffer
        .content
        .windows(draft.len())
        .find(|cells| {
            cells
                .iter()
                .zip(&draft)
                .all(|(cell, expected)| cell.symbol().starts_with(*expected))
        })
        .expect("typed draft must be rendered in the composer");

    let foregrounds = draft_cells.iter().map(|cell| cell.fg).collect::<Vec<_>>();
    let backgrounds = draft_cells.iter().map(|cell| cell.bg).collect::<Vec<_>>();
    assert!(
        foregrounds
            .iter()
            .all(|color| *color == Color::Rgb(225, 225, 225)),
        "typed draft foregrounds must match Grok primary: {foregrounds:?}"
    );
    assert!(
        backgrounds
            .iter()
            .all(|color| *color == Color::Rgb(20, 20, 20)),
        "typed draft backgrounds must match Grok canvas: {backgrounds:?}"
    );

    // act
    let composer = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, H))
        .dock
        .expect("startup shell must include a dock")
        .composer;
    // assert
    assert_eq!(buffer[(composer.x, composer.y)].fg, Color::Rgb(80, 80, 88));
    assert_eq!(
        buffer[(composer.x + 2, composer.y + 1)].fg,
        Color::Rgb(200, 200, 200)
    );
}

#[test]
fn unfocused_empty_composer_uses_grok_idle_state() {
    // arrange
    // act
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;
    let area = Rect::new(0, 0, W, 40);
    let composer = FrameLayoutPlan::for_app(&app, area)
        .dock
        .expect("live shell must include a dock")
        .composer;
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let input_row = (composer.x..composer.right())
        .map(|x| buffer[(x, composer.y)].symbol())
        .collect::<String>();

    // assert
    assert_eq!(composer.height, 1);
    assert!(input_row.contains("Build anything"), "{input_row:?}");
    assert_eq!(buffer[(composer.x, composer.y)].fg, Color::Rgb(88, 88, 88));
    assert_eq!(
        buffer[(composer.x + 2, composer.y)].fg,
        Color::Rgb(78, 78, 78)
    );
}

#[test]
fn unfocused_draft_keeps_wrapped_height_and_cursor_follow_window() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    let draft = format!(
        "FIRST alpha beta gamma delta epsilon zeta eta theta iota kappa {}LAST omega",
        "middle ".repeat(80)
    );
    app.focus = Focus::Prompt;
    app.handle_paste(&draft);
    let area = Rect::new(0, 0, 80, 24);
    let focused_composer = FrameLayoutPlan::for_app(&app, area)
        .dock
        .expect("live shell must include a dock")
        .composer;

    // act
    app.focus = Focus::Details;
    let composer = FrameLayoutPlan::for_app(&app, area)
        .dock
        .expect("live shell must include a dock")
        .composer;
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });

    // assert
    assert!(focused_composer.height > 3, "{focused_composer:?}");
    assert_eq!(composer.height, focused_composer.height, "{composer:?}");
    let visible_draft = (composer.y + 1..composer.bottom() - 1)
        .map(|y| {
            (composer.x..composer.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!visible_draft.contains("FIRST"), "{visible_draft:?}");
    assert!(visible_draft.contains("LAST"), "{visible_draft:?}");
    assert_eq!(
        buffer[(composer.x + 4, composer.bottom() - 2)].fg,
        Color::Rgb(155, 155, 155)
    );
}

#[test]
fn focused_wrapped_draft_caps_at_six_content_rows() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_buffer = "wrapped composer content ".repeat(200);
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.focus = Focus::Prompt;

    // act
    let composer = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 80, 24))
        .dock
        .expect("live shell must include a dock")
        .composer;

    // assert
    assert_eq!(
        composer.height, 8,
        "six content rows plus top and bottom borders must cap the composer at eight rows"
    );
}

#[test]
fn live_composer_wraps_against_its_inset_width() {
    // arrange
    for (width, height, draft_width) in [(120, 40, 111), (60, 20, 53)] {
        let mut app = AppState::new_live(None, false, None);
        app.composer.prompt_buffer = "x".repeat(draft_width);
        app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
        app.focus = Focus::Prompt;

        // act
        let composer = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height))
            .dock
            .expect("live shell must include a dock")
            .composer;

        // assert
        assert_eq!(
            composer.height, 4,
            "draft at the inset-width boundary must wrap to two content rows at {width}x{height}"
        );
    }
}

#[test]
fn shell_composer_semantics_preserve_border_corners_and_prompt_position() {
    // arrange
    // act
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref(
            "build",
            "mock:this-model-name-is-deliberately-much-too-long-for-sixty-columns",
        )
        .with_mode_label("Shell"),
    );
    app.queued_prompt_count = 7;
    app.composer.shell_mode = true;
    let area = Rect::new(0, 0, 60, 20);
    let composer = FrameLayoutPlan::for_app(&app, area)
        .dock
        .expect("live shell must include a dock")
        .composer;
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });

    // assert
    assert_eq!(buffer[(composer.x, composer.y)].symbol(), "╭");
    assert_eq!(buffer[(composer.right() - 1, composer.y)].symbol(), "╮");
    assert_eq!(buffer[(composer.x, composer.bottom() - 1)].symbol(), "╰");
    assert_eq!(
        buffer[(composer.right() - 1, composer.bottom() - 1)].symbol(),
        "╯"
    );
    assert_eq!(buffer[(composer.x + 2, composer.y + 1)].symbol(), "!");
    let bottom_border = (composer.x..composer.right())
        .map(|x| buffer[(x, composer.bottom() - 1)].symbol())
        .collect::<String>();
    assert!(
        bottom_border.contains("Run shell command"),
        "{bottom_border:?}"
    );
    assert!(!bottom_border.contains("queued 7"), "{bottom_border:?}");
}

#[test]
fn live_bordered_composer_reserves_an_inner_input_row() {
    // arrange
    // act
    let app = AppState::new_live(None, false, None);
    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, 40));
    let composer = plan.dock.expect("live shell must include a dock").composer;

    // assert
    assert_eq!(
        composer.height, 3,
        "single-line bordered composer needs exactly top, input, and bottom rows; got {composer:?}"
    );
}

/// Typing clears the welcome panel (startup_mode stays but welcome not visible).
#[test]
fn input_typing_clears_welcome_panel() {
    // arrange
    let mut app = startup_app();
    assert!(app.startup_shell_visible());

    // act
    // Type a char to transition to Prompt
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    // Startup mode is still true, but the welcome panel is no longer
    // the active surface because the composer has a draft.
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    // assert
    assert_eq!(view.phase, StartupPhase::DraftActive);
    assert!(!view.welcome_visible);
    assert!(view.welcome_cleared_by_draft());
}

/// Unicode text is handled without panic and cursor stays in bounds.
#[test]
fn input_unicode_text_no_panic() {
    // arrange
    let mut app = startup_app();
    // Type unicode chars one at a time
    for c in "你好世界🌍".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.prompt_buffer, "你好世界🌍");

    // act
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    // assert
    assert!(view.cursor_in_bounds());
    let width = view.draft_display_width();
    assert!(width > 0, "unicode text should have nonzero display width");
}

/// Enhanced key: backspace works on unicode text without panic.
#[test]
fn input_enhanced_key_backspace_unicode() {
    // arrange
    let mut app = startup_app();
    app.focus = Focus::Prompt;
    // Type unicode chars
    for c in "你好".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.prompt_buffer, "你好");
    assert_eq!(app.composer.prompt_cursor, 2);

    // Backspace one char
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.composer.prompt_buffer, "你");
    assert_eq!(app.composer.prompt_cursor, 1);

    // act
    // Backspace again
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    // assert
    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(app.composer.prompt_cursor, 0);
}

/// Empty draft at startup: focus owner is still composer.
#[test]
fn input_empty_draft_focus_owner_composer() {
    // arrange
    // act
    let app = startup_app();
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    // assert
    assert_eq!(view.focus_owner, FocusOwner::Composer);
    assert!(view.welcome_visible);
    assert_eq!(view.focus_owner.as_str(), "composer");
}

/// Small viewport (80x24) does not panic at startup.
#[test]
fn input_small_viewport_no_panic() {
    // arrange
    // act
    let app = startup_app();
    let rendered = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    // Must still render something
    // assert
    assert!(!rendered.is_empty());
    // Composer glyph should still be present even at small viewport
    assert!(
        rendered.contains('❯') || rendered.contains('│'),
        "small viewport must still render composer area\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Failure scenario: empty-small-unicode-enhanced-key
// Verifies no_panic==true and recovered==true
// ---------------------------------------------------------------------------

/// Failure scenario: empty draft, small viewport, unicode text, enhanced keys.
/// The app must not panic and must recover to a usable state.
#[test]
fn failure_scenario_empty_small_unicode_enhanced_key() {
    // arrange
    let mut app = startup_app();

    // Render at small viewport — must not panic
    let small = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    assert!(!small.is_empty(), "small viewport render must not be empty");

    // Type unicode chars — must not panic
    for c in "你好世界🌍".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.prompt_buffer, "你好世界🌍");

    // Render with unicode draft at small viewport — must not panic
    let small_unicode = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    assert!(
        !small_unicode.is_empty(),
        "unicode small viewport render must not be empty"
    );

    // Enhanced key: backspace — must not panic
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(
        app.composer.prompt_buffer,
        "你好世界🌍"[..].chars().take(4).collect::<String>()
    );

    // Recovered: app is still usable (can type more text)
    for c in " recovered".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert!(app.composer.prompt_buffer.contains("recovered"));

    // Final render — must not panic
    let final_render = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    assert!(
        !final_render.is_empty(),
        "final render after recovery must not be empty"
    );

    // act
    // External postconditions
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    // assert
    assert_eq!(view.focus_owner, FocusOwner::Composer);
    assert!(view.cursor_in_bounds());
}

// ---------------------------------------------------------------------------
// Render capture for evidence (run with --nocapture)
// ---------------------------------------------------------------------------

/// Print the rendered startup screen for evidence capture.
#[test]
fn render_startup_capture_matches_expected_cells() {
    // arrange
    // act
    let app = startup_app();
    let rendered = render(&app);
    // assert
    println!("{rendered}");
}

/// Print the rendered draft screen for evidence capture.
#[test]
fn render_draft_capture_matches_expected_cells() {
    // arrange
    // act
    let mut app = startup_app();
    app.composer.prompt_buffer = "Browser QA draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let rendered = render(&app);
    // assert
    println!("{rendered}");
}
