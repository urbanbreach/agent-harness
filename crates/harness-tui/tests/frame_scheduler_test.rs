use harness_tui::scheduling::{DualClock, FrameInputs, FrameReason, FrameScheduler};

#[test]
fn frame_clock_advances_at_fixed_cadence() {
    // arrange
    // Given: two independent fake clock streams and an active scheduler.
    let clock = DualClock::new();
    let mut scheduler = FrameScheduler::new();

    // When: the active cadence is armed, then one animation tick elapses.
    let armed = scheduler.schedule(clock.snapshot(), FrameInputs::active());
    clock.tick_animation();
    let frame = scheduler.schedule(clock.snapshot(), FrameInputs::active());

    // act
    // Then: the design-contract cadence is 30 Hz (33 ms integer deadline).
    // assert
    assert_eq!(clock.animation_now(), 33);
    assert_eq!(clock.flush_now(), 0);
    assert_eq!(
        armed.as_ref().and_then(|decision| decision.deadline_ms),
        Some(33)
    );
    assert_eq!(frame.as_ref().map(|decision| decision.render), Some(true));
    assert_eq!(
        frame.as_ref().and_then(|decision| decision.deadline_ms),
        Some(66)
    );
    assert_eq!(
        frame.as_ref().map(|decision| decision.reason),
        Some(FrameReason::Animation)
    );
}

#[test]
fn animation_and_flush_deadlines_are_serviced_in_exact_order() {
    // arrange
    // Given: animation and one coalesced input flush are armed together.
    let clock = DualClock::new();
    let mut scheduler = FrameScheduler::new();
    let armed = scheduler.schedule(clock.snapshot(), FrameInputs::active_and_flush());
    assert_eq!(
        armed.as_ref().and_then(|decision| decision.deadline_ms),
        Some(16)
    );

    // When: only the shorter flush deadline elapses.
    clock.tick_flush();
    let flush = scheduler.schedule(clock.snapshot(), FrameInputs::active());

    // Then: flush renders first and leaves animation at its original 33 ms deadline.
    assert_eq!(
        flush.as_ref().map(|decision| decision.reason),
        Some(FrameReason::Flush)
    );
    assert_eq!(
        flush.as_ref().and_then(|decision| decision.deadline_ms),
        Some(33)
    );

    // When: the independent animation clock reaches 33 ms.
    clock.tick_animation();
    let animation = scheduler.schedule(clock.snapshot(), FrameInputs::active());

    // act
    // Then: animation renders second.
    // assert
    assert_eq!(
        animation.as_ref().map(|decision| decision.reason),
        Some(FrameReason::Animation)
    );
}

#[test]
fn flush_is_not_starved_by_continuous_animation() {
    // arrange
    // Given: a continuously active animation cadence.
    let clock = DualClock::new();
    let mut scheduler = FrameScheduler::new();
    scheduler.schedule(clock.snapshot(), FrameInputs::active());

    // When: a flush arrives while animation is still progressing.
    clock.tick_animation();
    scheduler.schedule(clock.snapshot(), FrameInputs::active_and_flush());
    clock.tick_flush();
    let flush = scheduler.schedule(clock.snapshot(), FrameInputs::active());

    // act
    // Then: the flush is rendered even though animation remains active.
    // assert
    assert_eq!(flush.as_ref().map(|decision| decision.render), Some(true));
    assert_eq!(
        flush.as_ref().map(|decision| decision.reason),
        Some(FrameReason::Flush)
    );
}

#[test]
fn settled_scheduler_has_zero_idle_redraw() {
    // arrange
    // Given: an animation that has rendered its final active frame.
    let clock = DualClock::new();
    let mut scheduler = FrameScheduler::new();
    scheduler.schedule(clock.snapshot(), FrameInputs::active());
    clock.tick_animation();
    scheduler.schedule(clock.snapshot(), FrameInputs::active());

    // When: animation settles and no input is pending.
    let settled = scheduler.schedule(clock.snapshot(), FrameInputs::idle());
    let idle = scheduler.schedule(clock.snapshot(), FrameInputs::idle());

    // act
    // Then: stale animation deadlines are disarmed and idle produces no redraw.
    // assert
    assert_eq!(settled, None);
    assert_eq!(idle, None);
}

#[test]
fn reduced_motion_settles_transitions_without_scheduled_frames() {
    // arrange
    // Given: reduced motion is enabled before an active transition.
    let clock = DualClock::new();
    let mut scheduler = FrameScheduler::with_reduced_motion(true);

    // When: an animated transition is submitted.
    let settled = scheduler.schedule(clock.snapshot(), FrameInputs::active());
    let idle = scheduler.schedule(clock.snapshot(), FrameInputs::idle());

    // act
    // Then: it settles immediately and never arms a frame deadline.
    // assert
    assert_eq!(settled.as_ref().map(|decision| decision.render), Some(true));
    assert_eq!(
        settled.as_ref().and_then(|decision| decision.deadline_ms),
        None
    );
    assert_eq!(
        settled.as_ref().map(|decision| decision.reason),
        Some(FrameReason::ReducedMotion)
    );
    assert_eq!(idle, None);
}

#[test]
fn input_burst_is_coalesced_into_one_flush_render() {
    // arrange
    // Given: several input events arrive before the 16 ms flush boundary.
    let clock = DualClock::new();
    let mut scheduler = FrameScheduler::new();
    scheduler.schedule(clock.snapshot(), FrameInputs::flush());
    clock.advance_flush(5);
    scheduler.schedule(clock.snapshot(), FrameInputs::flush());
    clock.advance_flush(5);
    scheduler.schedule(clock.snapshot(), FrameInputs::flush());

    // When: the original flush deadline is reached.
    clock.advance_flush(6);
    let rendered = scheduler.schedule(clock.snapshot(), FrameInputs::idle());

    // act
    // Then: all three inputs produce one render, not one render per input.
    // assert
    assert_eq!(
        rendered.as_ref().map(|decision| decision.render),
        Some(true)
    );
    assert_eq!(
        rendered.as_ref().map(|decision| decision.reason),
        Some(FrameReason::Flush)
    );
    assert_eq!(
        scheduler.schedule(clock.snapshot(), FrameInputs::idle()),
        None
    );
}

#[test]
fn custom_flush_cadence_supports_one_hundred_twenty_hertz_input_pacing() {
    let clock = DualClock::new();
    let mut scheduler = FrameScheduler::with_flush_interval_ms(8);

    // When: the scheduler reaches the configured flush boundary.
    let armed = scheduler.schedule(clock.snapshot(), FrameInputs::flush());
    clock.advance_flush(8);
    let rendered = scheduler.schedule(clock.snapshot(), FrameInputs::idle());

    // Then: the work is presented at the 120 Hz cadence instead of the 16 ms default.
    assert_eq!(
        (
            armed.and_then(|decision| decision.deadline_ms),
            rendered.map(|decision| (decision.render, decision.reason)),
        ),
        (Some(8), Some((true, FrameReason::Flush)))
    );
}
