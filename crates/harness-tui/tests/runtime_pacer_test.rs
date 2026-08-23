use harness_tui::scheduling::{
    DualClock, RuntimePacer, WheelDirection, WheelSample, ANIMATION_PERIOD_MS, FLUSH_DEADLINE_MS,
    MAX_WHEEL_STEPS_PER_FLUSH,
};
use harness_tui::terminal::brand::TerminalName;
use harness_tui::terminal::multiplexer::TerminalMultiplexer;

#[test]
fn changed_work_is_coalesced_until_the_sixteen_millisecond_flush_deadline() {
    // arrange
    // Given: changed UI work requested at the start of a flush window.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    pacer.request_flush();

    // When: more changed work arrives before the original deadline.
    let armed = pacer.poll(clock.snapshot(), false);
    clock.advance_flush(10);
    pacer.request_flush();
    let pending = pacer.poll(clock.snapshot(), false);
    clock.advance_flush(FLUSH_DEADLINE_MS - 10);
    let due = pacer.poll(clock.snapshot(), false);

    // act
    // Then: neither request moves the deadline, and exactly the due poll paints.
    // assert
    assert_eq!(
        (armed.paint, pending.paint, due.paint),
        (false, false, true)
    );
}

#[test]
fn changed_work_can_be_paced_at_one_hundred_twenty_hertz() {
    // arrange — Given the runtime pacer uses an 8 ms minimum draw interval.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::with_reduced_motion_and_flush_interval_ms(false, 8);
    pacer.request_flush();

    // act — When the high-refresh deadline elapses.
    let armed = pacer.poll(clock.snapshot(), false);
    clock.advance_flush(8);
    let due = pacer.poll(clock.snapshot(), false);

    // assert — Then changed work paints at 120 Hz without creating an idle redraw loop.
    assert_eq!(
        (armed.next_wait_ms, armed.paint, due.paint, due.next_wait_ms),
        (Some(8), false, true, None)
    );
}

#[test]
fn arbiter_readiness_arms_then_waits_for_the_flush_deadline() {
    // arrange
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    pacer.request_flush();

    assert!(pacer.needs_poll(clock.snapshot(), false));
    let armed = pacer.poll(clock.snapshot(), false);
    assert!(!armed.paint);
    assert!(!pacer.needs_poll(clock.snapshot(), false));

    // act
    clock.advance_flush(FLUSH_DEADLINE_MS);
    // assert
    assert!(pacer.needs_poll(clock.snapshot(), false));
    assert!(pacer.poll(clock.snapshot(), false).paint);
    assert!(!pacer.needs_poll(clock.snapshot(), false));
}

#[test]
fn animation_ticks_remain_independent_from_the_flush_clock() {
    // arrange
    // Given: an active animation with no pending flush work.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    let armed = pacer.poll(clock.snapshot(), true);

    // When: the flush clock advances first, then the animation clock reaches 30 Hz.
    clock.advance_flush(FLUSH_DEADLINE_MS);
    let flush_clock_only = pacer.poll(clock.snapshot(), true);
    clock.advance_animation(ANIMATION_PERIOD_MS);
    let animation_due = pacer.poll(clock.snapshot(), true);

    // act
    // Then: only the independent animation deadline advances and paints a frame.
    // assert
    assert_eq!(
        (
            armed.advance_animation,
            flush_clock_only.advance_animation,
            animation_due.advance_animation,
            animation_due.paint,
        ),
        (false, false, true, true)
    );
}

#[test]
fn reduced_motion_settles_flush_work_without_arming_a_deadline() {
    // arrange
    // Given: reduced motion is enabled and changed UI work is pending.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::with_reduced_motion(true);
    pacer.request_flush();

    // When: the runtime pacer is polled without advancing either clock.
    let settled = pacer.poll(clock.snapshot(), false);

    // act
    // Then: existing reduced-motion scheduling settles immediately and parks.
    // assert
    assert_eq!((settled.paint, settled.next_wait_ms), (true, None));
}

#[test]
fn wheel_flush_and_animation_both_progress_under_continuous_demand() {
    // arrange
    // Given: animation and wheel input become active together.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    pacer.queue_wheel(WheelSample::new(WheelDirection::Down, 40, 12));
    pacer.poll(clock.snapshot(), true);

    // When: each independent deadline becomes due in turn.
    clock.advance_flush(FLUSH_DEADLINE_MS);
    let wheel_due = pacer.poll(clock.snapshot(), true);
    clock.advance_animation(ANIMATION_PERIOD_MS);
    let animation_due = pacer.poll(clock.snapshot(), true);

    // act
    // Then: neither work stream starves the other.
    // assert
    assert_eq!(
        (
            wheel_due.wheel_batch.map(|batch| batch.steps()),
            animation_due.advance_animation,
        ),
        (Some(1), true)
    );
}

#[test]
fn one_thousand_idle_polls_produce_zero_paints() {
    // arrange
    // Given: a settled runtime with no animations, input, or provider changes.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();

    // When: the runtime is polled repeatedly without advancing either clock.
    let paints = (0..1_000)
        .filter(|_| pacer.poll(clock.snapshot(), false).paint)
        .count();

    // act
    // Then: idle polling never schedules a paint.
    // assert
    assert_eq!(paints, 0);
}

#[test]
fn wheel_flood_is_reduced_to_one_capped_batch() {
    // arrange
    // Given: a thousand wheel ticks land inside one flush window.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    for _ in 0..1_000 {
        pacer.queue_wheel(WheelSample::new(WheelDirection::Down, 72, 18));
    }
    pacer.poll(clock.snapshot(), false);

    // When: the 16 ms wheel flush becomes due.
    clock.advance_flush(FLUSH_DEADLINE_MS);
    let due = pacer.poll(clock.snapshot(), false);
    let after = pacer.poll(clock.snapshot(), false);

    // act
    // Then: the flood becomes one meaningful bounded dispatch and no tail work.
    // assert
    assert_eq!(
        due.wheel_batch.map(|batch| (
            batch.direction(),
            batch.steps(),
            batch.column(),
            batch.row()
        )),
        Some((WheelDirection::Down, MAX_WHEEL_STEPS_PER_FLUSH, 72, 18,))
    );
    assert_eq!(after.wheel_batch, None);
}

#[test]
fn three_event_terminal_wheel_notch_normalizes_to_one_step() {
    // arrange
    // Given: a terminal that reports three wheel events for one physical notch.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::with_terminal_wheel_profile(
        false,
        TerminalName::Ghostty,
        TerminalMultiplexer::Undetected,
    );
    for _ in 0..3 {
        pacer.queue_wheel(WheelSample::new(WheelDirection::Down, 30, 12));
    }
    pacer.poll(clock.snapshot(), false);

    // When: the coalesced wheel flush becomes due.
    clock.advance_flush(FLUSH_DEADLINE_MS);
    let due = pacer.poll(clock.snapshot(), false);

    // act
    // Then: the physical notch dispatches one logical scroll step.
    // assert
    assert_eq!(due.wheel_batch.map(|batch| batch.steps()), Some(1));
}

#[test]
fn multiplexer_reencoded_wheel_events_remain_individual_steps() {
    // arrange
    // Given: the same terminal behind a multiplexer that re-encodes wheel input.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::with_terminal_wheel_profile(
        false,
        TerminalName::Ghostty,
        TerminalMultiplexer::Tmux,
    );
    for _ in 0..3 {
        pacer.queue_wheel(WheelSample::new(WheelDirection::Down, 30, 12));
    }
    pacer.poll(clock.snapshot(), false);

    // When: the coalesced wheel flush becomes due.
    clock.advance_flush(FLUSH_DEADLINE_MS);
    let due = pacer.poll(clock.snapshot(), false);

    // act
    // Then: all re-encoded events remain logical scroll steps.
    // assert
    assert_eq!(due.wheel_batch.map(|batch| batch.steps()), Some(3));
}

#[test]
fn wheel_flush_paints_only_when_app_state_reports_movement() {
    // arrange
    // Given: a due wheel batch with no other dirty or animation work.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    pacer.queue_wheel(WheelSample::new(WheelDirection::Up, 20, 8));
    pacer.poll(clock.snapshot(), false);
    clock.advance_flush(FLUSH_DEADLINE_MS);

    // When: the batch is released to the AppState movement seam.
    let due = pacer.poll(clock.snapshot(), false);

    // act
    // Then: clamped/no-op movement suppresses paint while real movement enables it.
    // assert
    assert_eq!(
        (due.should_paint(false), due.should_paint(true)),
        (false, true)
    );
}
