#[test]
fn reduced_motion_keeps_completed_tools_static() {
    // arrange
    // Given: a completed tool rendered with reduced motion.
    let mut app = tool_finished_app();
    app.set_reduced_motion_for_evidence(true);
    let before = rail_colors(&render_buffer(&app, 120, 40));

    // When: a deterministic animation tick advances.
    app.advance_animation_tick_for_evidence();
    let after = rail_colors(&render_buffer(&app, 120, 40));

    // act
    // Then: the completed tool remains rail-free and no timer is armed.
    // assert
    assert!(before.is_empty());
    assert_eq!(before, after);
    assert!(!app.has_active_animations_with_motion_for_evidence(false));
    assert_eq!(
        app.animation_tick_interval_with_motion_for_evidence(false),
        None
    );
}

#[test]
fn offscreen_running_tool_parks_and_wide_text_geometry_stays_stable() {
    // arrange
    // Given: an active tool row containing wide, emoji, and combining glyphs.
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_wide")), false, None);
    for event in tool_running_events("src/界🙂e\u{301}.rs") {
        app.ingest_event(event);
    }
    let before = render_buffer(&app, 120, 40);
    let before_text = buffer_to_string(&before, 120);
    assert!(
        before_text.contains("界 🙂 e\u{301}"),
        "wide fixture must be visible\n{before_text}"
    );

    // When: the rendered viewport reports that the running row is off-screen.
    app.record_visible_running_tool_motion_for_evidence(false);
    assert!(!app.has_active_animations_for_evidence());
    app.advance_animation_tick_for_evidence();
    let after = render_buffer(&app, 120, 40);
    let after_text = buffer_to_string(&after, 120);
    let before_wide_row = before_text
        .lines()
        .find(|line| line.contains("界 🙂 e\u{301}"))
        .expect("wide fixture row before tick");
    let after_wide_row = after_text
        .lines()
        .find(|line| line.contains("界 🙂 e\u{301}"))
        .expect("wide fixture row after tick");

    // act
    // Then: scheduling parks and the wide-text row keeps identical cell geometry.
    // assert
    assert_eq!(before_wide_row, after_wide_row);
}

#[test]
fn completed_tool_never_arms_motion_or_paints_a_rail() {
    // arrange
    // Given: a tool and provider that just completed successfully.
    let mut app = tool_finished_app();

    // When: the completion frame and a later frame are painted.
    let completed = rail_colors(&render_buffer(&app, 100, 24));
    app.advance_animation_tick_for_evidence();
    let later = rail_colors(&render_buffer(&app, 100, 24));

    // act
    // Then: completion is static, rail-free, and requests no idle redraws.
    // assert
    assert!(completed.is_empty());
    assert_eq!(completed, later);
    assert!(
        !app.has_active_animations_for_evidence(),
        "settled UI must request zero idle redraws"
    );
}

#[test]
fn completed_tool_stays_static_across_the_legacy_finish_window() {
    // arrange
    // Given: a completed tool with no active motion ownership.
    let mut app = tool_finished_app();
    let settled = rail_colors(&render_buffer(&app, 100, 24));

    // When: every former finish-flash frame is rendered.
    let later_frames = (0..finish_flash_frames())
        .map(|_| {
            app.advance_animation_tick_for_evidence();
            rail_colors(&render_buffer(&app, 100, 24))
        })
        .collect::<Vec<_>>();

    // act
    // Then: no legacy frame introduces a settled rail or visual transition.
    // assert
    assert!(settled.is_empty());
    assert!(later_frames.iter().all(|frame| frame == &settled));
}

#[test]
fn grouped_last_finisher_keeps_a_static_error_rail_without_motion() {
    // arrange
    // Given: a successful command followed later by a failed command in one group.
    let mut grouped = grouped_mixed_app();
    let flash = rail_colors(&render_buffer(&grouped, 120, 40));

    // When: the former finish-transition window elapses.
    for _ in 0..finish_flash_frames().saturating_add(2) {
        grouped.advance_animation_tick_for_evidence();
    }
    let settled = rail_colors(&render_buffer(&grouped, 120, 40));

    // act
    // Then: the failed group keeps its state rail without animating it.
    // assert
    assert!(!flash.is_empty());
    assert!(flash
        .iter()
        .all(|color| *color == Theme::default().status.error));
    assert_eq!(flash, settled);
}

#[test]
fn startup_welcome_expands_once_across_fixed_ticks() {
    // arrange
    // Given: a startup shell whose welcome panel expands once after its resting frame.
    let plan = FixedTickPlan::new("startup-logo", 100, 30, 5).with_tick_ms(100);

    // When: two independent fixed-tick captures advance the scheduler.
    let (sequence_a, sequence_b, clock_a) = capture_pair(&plan, startup_idle_app);

    // Then: the identity material stays static while the panel settles after one expansion.
    assert_eq!(sequence_a.schema_version, ANIMATION_FRAME_SEQUENCE_SCHEMA);
    assert_eq!(sequence_a.surface_id, "startup-logo");
    assert_eq!(sequence_a.frames.len(), 5);
    assert_eq!(sequence_a.frames[0].animation_phase, 0);
    assert_eq!(sequence_a.frames[4].animation_phase, 4);
    assert_eq!(sequence_a.frames[4].mono_ms, 400);
    assert_eq!(clock_a.mono_ms(), 400);

    // act
    for frame in &sequence_a.frames {
        // assert
        assert!(
            spinner_glyphs_in_cells(&frame.cells).is_empty(),
            "startup logo must not paint braille spinner motion\n{}",
            frame.cells
        );
        assert!(frame.cells.contains('⣿'));
    }
    assert_ne!(sequence_a.frames[2].cells, sequence_a.frames[3].cells);
    assert!(sequence_a
        .frames
        .windows(2)
        .skip(3)
        .all(|frames| frames[0].cells == frames[1].cells));

    assert_sequences_equal(&sequence_a, &sequence_b)
        .expect("independent startup-logo captures must be byte-stable");
}

#[test]
fn startup_logo_fixed_ticks_park_after_the_settled_frame() {
    // arrange
    // act
    let plan = FixedTickPlan::new("startup-logo-settled", 100, 30, 21).with_tick_ms(100);
    let mut app = startup_idle_app();
    let clock = FakeClock::new();
    let sequence = capture_fixed_tick_sequence(&mut app, &clock, &plan)
        .expect("startup logo fixed-tick capture");

    // assert
    assert_eq!(sequence.frames[0].mono_ms, 0);
    assert_eq!(sequence.frames[6].mono_ms, 600);
    assert_eq!(sequence.frames[20].mono_ms, 2_000);
    assert!(sequence
        .frames
        .windows(2)
        .skip(3)
        .all(|frames| frames[0].cells == frames[1].cells));
    assert!(!app.has_active_animations_for_evidence());
}

#[test]
fn permission_wait_fixed_tick_sequence_is_deterministic() {
    // arrange
    // act
    // assert
    // Given: permission dock open during an active turn (spinner stays static while waiting).
    // Modal/waiting chrome is static under phase ticks; capture is still fail-closed deterministic.
    let plan = FixedTickPlan::new("permission-wait", 100, 28, 6).with_tick_ms(100);

    // When: two independent fixed-tick captures
    let (sequence_a, sequence_b, clock_a) = capture_pair(&plan, permission_wait_app);

    // Then: schema, clock, permission chrome, stable cells, equality
    assert_eq!(sequence_a.schema_version, ANIMATION_FRAME_SEQUENCE_SCHEMA);
    assert_eq!(sequence_a.surface_id, "permission-wait");
    assert_eq!(sequence_a.frames.len(), 6);
    assert_eq!(sequence_a.frames[0].animation_phase, 0);
    assert_eq!(sequence_a.frames[5].animation_phase, 5);
    assert_eq!(sequence_a.frames[5].mono_ms, 500);
    assert_eq!(clock_a.mono_ms(), 500);

    let first = &sequence_a.frames[0].cells;
    assert!(
        first.contains("Allow") || first.contains("Edit") || first.contains("demo.txt"),
        "permission-wait surface must paint permission dock chrome\n{first}"
    );

    for frame in &sequence_a.frames {
        assert!(
            spinner_glyphs_in_cells(&frame.cells).is_empty(),
            "permission-wait freezes braille spinner motion\n{}",
            frame.cells
        );
    }
    assert_eq!(
        sequence_a.frames[0].cells, sequence_a.frames[1].cells,
        "permission-wait cells stay stable across phase ticks (no dedicated wait animation yet)"
    );
    assert_eq!(
        sequence_a.frames[0].cells, sequence_a.frames[5].cells,
        "permission-wait must remain stable for the full fixed-tick plan"
    );

    assert_sequences_equal(&sequence_a, &sequence_b)
        .expect("independent permission-wait captures must be byte-stable");

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("permission-wait.frames.json");
    write_sequence_artifact(&path, &sequence_a).expect("write artifact");
    let loaded = read_sequence_artifact(&path).expect("read artifact");
    assert_eq!(loaded, sequence_a);
}

#[test]
fn empty_fixed_tick_plan_fails_closed() {
    // arrange
    // act
    // assert
    let mut app = streaming_wait_app();
    let clock = FakeClock::new();
    let plan = FixedTickPlan::new("empty", 40, 12, 0);
    let err = capture_fixed_tick_sequence(&mut app, &clock, &plan).expect_err("empty plan");
    assert_eq!(err, AnimationEvidenceError::EmptyPlan);
}

#[test]
fn fixed_tick_trace_frames_are_ordered_and_advance_at_spinner_cadence() {
    // arrange — fixed-tick plan for an animated spinner surface
    let plan = FixedTickPlan::new("trace-ordering", 100, 24, 8).with_tick_ms(100);

    // act
    let (sequence, _second, _clock) = capture_pair(&plan, streaming_wait_app);

    // assert — clock and phase are strictly increasing.
    assert_eq!(sequence.frames.len(), 8);
    for pair in sequence.frames.windows(2) {
        assert!(
            pair[1].mono_ms > pair[0].mono_ms,
            "trace mono_ms must strictly increase: {} then {}",
            pair[0].mono_ms,
            pair[1].mono_ms
        );
        assert!(
            pair[1].animation_phase > pair[0].animation_phase,
            "animation phase must strictly increase: {} then {}",
            pair[0].animation_phase,
            pair[1].animation_phase
        );
    }
    assert!(
        sequence
            .frames
            .windows(2)
            .any(|pair| pair[0].cells != pair[1].cells),
        "the trace must visibly advance at the spinner cadence"
    );
}
