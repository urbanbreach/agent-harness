// ===========================================================================
// Flow 5: Frame clock lifecycle → cursor state → writer pipeline (T10)
// ===========================================================================

/// The frame clock (T10) ticks deterministically, cursor state tracks
/// position/shape, and the synchronized writer emits correct escape bytes.
/// These compose into a render tick pipeline.

#[test]
fn frame_clock_cursor_and_writer_pipeline() {
    // arrange
    // --- T10: FrameClock ---
    let mut clock = FrameClock::new();
    assert_eq!(clock.mono_ms(), 0);
    assert_eq!(clock.phase().get(), 0);

    clock.tick();
    assert_eq!(
        clock.mono_ms(),
        33,
        "default tick follows the 30 Hz cadence"
    );
    assert_eq!(clock.phase().get(), 1);

    clock.tick_n(4);
    assert_eq!(clock.mono_ms(), 165, "5 total ticks × 33ms = 165ms");
    assert_eq!(clock.phase().get(), 5);

    // --- T10: CursorState ---
    let cursor = CursorState::new();
    assert!(cursor.visible, "cursor starts visible by default");
    assert_eq!(cursor.position.column, 0);
    assert_eq!(cursor.position.row, 0);
    assert_eq!(cursor.shape, CursorShape::Default);

    // Move and restyle via the builder methods
    let cursor = cursor
        .move_to(harness_tui::terminal::CursorPosition::new(10, 5))
        .with_shape(CursorShape::Line)
        .hide();
    assert_eq!(cursor.position.column, 10);
    assert_eq!(cursor.position.row, 5);
    assert_eq!(cursor.shape, CursorShape::Line);
    assert!(!cursor.is_visible(), "hide() makes cursor invisible");

    // Clamping prevents out-of-bounds
    let clamped =
        cursor.move_to_clamped(harness_tui::terminal::CursorPosition::new(200, 100), 80, 24);
    assert_eq!(
        clamped.position.column, 79,
        "column clamped to grid width - 1"
    );
    assert_eq!(clamped.position.row, 23, "row clamped to grid height - 1");

    // --- T10: SynchronizedWriter outputs BEGIN/END sync bytes ---
    let mut buffer: Vec<u8> = Vec::new();
    {
        let mut writer = harness_tui::terminal::SynchronizedWriter::new(&mut buffer);
        writer.begin_frame().unwrap();
        writer.write_payload(b"frame-data").unwrap();
        writer.end_frame().unwrap();
    }

    // act
    let begin_marker = String::from_utf8_lossy(harness_tui::terminal::BEGIN_SYNCHRONIZED_UPDATE);
    let end_marker = String::from_utf8_lossy(harness_tui::terminal::END_SYNCHRONIZED_UPDATE);
    let output = String::from_utf8_lossy(&buffer);
    // assert
    assert!(
        output.starts_with(begin_marker.as_ref()),
        "output must start with BEGIN sync escape, got: {output:?}"
    );
    assert!(
        output.ends_with(end_marker.as_ref()),
        "output must end with END sync escape, got: {output:?}"
    );
    assert!(
        output.contains("frame-data"),
        "frame data must be between sync markers"
    );
}

// ===========================================================================
// Flow 6: Responsive viewport → layout geometry → render (T14)
// ===========================================================================

/// Responsive viewport classification (T14) determines layout mode and
/// geometry across all seven canonical viewports. The layout plan (T14)
/// and render output (T11) adapt to each viewport.
#[test]
fn responsive_viewport_to_layout_geometry_and_render() {
    // arrange
    let app = live_app();

    // --- T14: classify all seven viewports ---
    let plans = ViewportPlan::all_plans();
    assert_eq!(plans.len(), 7, "seven canonical viewports");

    for plan in &plans {
        let (cols, rows) = plan.id.dims();
        let classification = ViewportClassification::from_dims(cols, rows);
        assert_eq!(plan.classification, classification);
        assert!(
            plan.composer_bordered,
            "composer border preserved at all viewports"
        );
        assert!(
            plan.footer_hints_visible,
            "footer hints visible at all viewports"
        );
    }

    // --- T14: responsive mode transitions (shell_layout derived per viewport) ---
    let theme = app.theme();

    // 120x40 → Primary breakpoint exceeded → Primary or Split mode
    let shell_120 = theme.live_shell_layout(120, 40);
    let mode_120x40 = session_responsive_mode(Rect::new(0, 0, 120, 40), shell_120);
    assert!(
        matches!(
            mode_120x40,
            SessionResponsiveMode::StandardMinimum
                | SessionResponsiveMode::Split
                | SessionResponsiveMode::Primary
        ),
        "120x40 must be Standard or wider, got {mode_120x40:?}"
    );

    // 50x16 → within Dense limits (≤60 wide, ≤18 tall)
    let shell_50 = theme.live_shell_layout(50, 16);
    let mode_50x16 = session_responsive_mode(Rect::new(0, 0, 50, 16), shell_50);
    assert_eq!(
        mode_50x16,
        SessionResponsiveMode::Dense,
        "50x16 must be Dense"
    );

    // --- T14 + T11: render at two viewports produces different geometry ---
    let plan_small = plan_at(&app, 80, 24);
    let plan_large = plan_at(&app, 120, 40);
    assert!(
        plan_large.content.width >= plan_small.content.width,
        "wider viewport must produce wider content region"
    );

    // act
    // Both render successfully
    let rendered_small = render_at(&app, 80, 24);
    let rendered_large = render_at(&app, 120, 40);
    // assert
    assert!(!rendered_small.trim().is_empty());
    assert!(!rendered_large.trim().is_empty());
    // The large render has strictly more cells
    assert!(
        rendered_large.len() > rendered_small.len(),
        "larger viewport produces more rendered cells"
    );
}

// ===========================================================================
// Cross-flow: terminal decode → prompt editor → scrollback → overlay → render
// ===========================================================================

/// The full pipeline: decode terminal bytes (T9), type into the prompt (T13),
/// submit to create transcript content, scroll the transcript (T12), open an
/// overlay (T15), and render the final state (T11) with layout (T14).
#[test]
fn full_pipeline_decode_prompt_scrollback_overlay_render() {
    // arrange
    let mut app = live_app();

    // --- T9: decode a prompt from raw bytes ---
    let raw = b"status";
    let decoded = decode_all(raw);
    assert_eq!(decoded.len(), 6);

    // --- Bridge + T13: feed decoded keys to the app ---
    for event in &decoded {
        let key = terminal_key_to_crossterm(event).expect("char keys convert");
        app.handle_key(key);
    }
    assert_eq!(app.composer.prompt_buffer, "status");

    // --- T12: ingest events to create scrollable transcript content ---
    for event in tool_call_events() {
        app.ingest_event(event);
    }
    app.record_transcript_max_scroll(40);
    assert!(app.follow_mode_active());

    // Scroll up to inspect history
    app.scroll_page_up(8);
    assert!(!app.follow_mode_active());
    assert!(app.transcript_scroll_offset() > 0);

    // --- T15: open the command palette overlay ---
    app.palette_visible = true;
    let stack = app.overlay_stack();
    assert_eq!(stack.top(), Some(OverlayKind::CommandPalette));
    assert!(stack.blocks_pointer_interaction());

    // --- T14: layout plan accounts for overlay + scroll state ---
    let plan = plan_at(&app, VIEWPORT_W, VIEWPORT_H);
    assert!(plan.palette_overlay.is_some(), "palette overlay in layout");
    assert!(plan.composer.is_some(), "composer still in layout");

    // --- T11: render the full state ---
    let rendered = render_at(&app, VIEWPORT_W, VIEWPORT_H);
    assert!(rendered.contains("status"), "prompt text visible in render");
    assert!(!rendered.trim().is_empty(), "frame is not empty");

    // --- T15: close overlay, return to clean state ---
    app.palette_visible = false;
    let stack_clean = app.overlay_stack();
    assert!(!stack_clean.blocks_pointer_interaction());

    // --- T12: scroll back to bottom ---
    app.scroll_goto_bottom();
    assert!(app.follow_mode_active());
    assert_eq!(app.transcript_scroll_offset(), 0);

    // act
    // Final render without overlay
    let final_render = render_at(&app, VIEWPORT_W, VIEWPORT_H);
    // assert
    assert!(!final_render.trim().is_empty());
}
