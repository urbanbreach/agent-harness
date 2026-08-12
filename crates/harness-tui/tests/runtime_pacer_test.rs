use harness_tui::scheduling::{
    DualClock, RuntimePacer, WheelDirection, WheelSample, ANIMATION_PERIOD_MS, FLUSH_DEADLINE_MS,
    MAX_WHEEL_STEPS_PER_FLUSH,
};
use harness_tui::terminal::brand::TerminalName;
use harness_tui::terminal::multiplexer::TerminalMultiplexer;

#[test]
fn changed_work_is_coalesced_until_the_sixteen_millisecond_flush_deadline() {
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

    // Then: neither request moves the deadline, and exactly the due poll paints.
    assert_eq!(
        (armed.paint, pending.paint, due.paint),
        (false, false, true)
    );
}

#[test]
fn animation_ticks_remain_independent_from_the_flush_clock() {
    // Given: an active animation with no pending flush work.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    let armed = pacer.poll(clock.snapshot(), true);

    // When: the flush clock advances first, then the animation clock reaches 30 Hz.
    clock.advance_flush(FLUSH_DEADLINE_MS);
    let flush_clock_only = pacer.poll(clock.snapshot(), true);
    clock.advance_animation(ANIMATION_PERIOD_MS);
    let animation_due = pacer.poll(clock.snapshot(), true);

    // Then: only the independent animation deadline advances and paints a frame.
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
    // Given: reduced motion is enabled and changed UI work is pending.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::with_reduced_motion(true);
    pacer.request_flush();

    // When: the runtime pacer is polled without advancing either clock.
    let settled = pacer.poll(clock.snapshot(), false);

    // Then: existing reduced-motion scheduling settles immediately and parks.
    assert_eq!((settled.paint, settled.next_wait_ms), (true, None));
}

#[test]
fn wheel_flush_and_animation_both_progress_under_continuous_demand() {
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

    // Then: neither work stream starves the other.
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
    // Given: a settled runtime with no animations, input, or provider changes.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();

    // When: the runtime is polled repeatedly without advancing either clock.
    let paints = (0..1_000)
        .filter(|_| pacer.poll(clock.snapshot(), false).paint)
        .count();

    // Then: idle polling never schedules a paint.
    assert_eq!(paints, 0);
}

#[test]
fn wheel_flood_is_reduced_to_one_capped_batch() {
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

    // Then: the flood becomes one meaningful bounded dispatch and no tail work.
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

    // Then: the physical notch dispatches one logical scroll step.
    assert_eq!(due.wheel_batch.map(|batch| batch.steps()), Some(1));
}

#[test]
fn multiplexer_reencoded_wheel_events_remain_individual_steps() {
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

    // Then: all re-encoded events remain logical scroll steps.
    assert_eq!(due.wheel_batch.map(|batch| batch.steps()), Some(3));
}

#[test]
fn wheel_flush_paints_only_when_app_state_reports_movement() {
    // Given: a due wheel batch with no other dirty or animation work.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    pacer.queue_wheel(WheelSample::new(WheelDirection::Up, 20, 8));
    pacer.poll(clock.snapshot(), false);
    clock.advance_flush(FLUSH_DEADLINE_MS);

    // When: the batch is released to the AppState movement seam.
    let due = pacer.poll(clock.snapshot(), false);

    // Then: clamped/no-op movement suppresses paint while real movement enables it.
    assert_eq!(
        (due.should_paint(false), due.should_paint(true)),
        (false, true)
    );
}
