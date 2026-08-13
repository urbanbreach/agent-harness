use std::time::Duration;

use harness_tui::scheduling::{DualClock, MotionCadence, MotionDemand, MotionPlan, RuntimePacer};
use harness_tui::terminal::{FrameKind, FrameOutput, FrameSubmission};

#[test]
fn demand_aggregation_is_order_independent_and_keeps_earliest_expiry() {
    // Given: slow, fast, and semantic-expiry demands in different orders.
    let demands = [
        MotionDemand::slow(Duration::from_millis(133)),
        MotionDemand::until(Duration::from_millis(400)),
        MotionDemand::fast(Duration::from_millis(33)),
        MotionDemand::until(Duration::from_millis(250)),
    ];

    // When: both orders are aggregated.
    let forward = MotionPlan::from_demands(demands);
    let reverse = MotionPlan::from_demands(demands.into_iter().rev());

    // Then: aggregation is deterministic, fast wins, and the earliest expiry remains.
    assert_eq!(forward, reverse);
    assert_eq!(
        forward.cadence(),
        MotionCadence::Fast(Duration::from_millis(33))
    );
    assert_eq!(forward.until(), Some(Duration::from_millis(250)));
}

#[test]
fn runtime_pacer_respects_slow_deadline_without_intermediate_paints() {
    // Given: a slow spinner whose next visible frame changes after 133 ms.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    let plan = MotionPlan::from_demand(MotionDemand::slow(Duration::from_millis(133)));

    // When: the pacer is armed and sampled at 33, 99, and 133 ms.
    let armed = pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(33);
    let first = pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(66);
    let second = pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(34);
    let due = pacer.poll(clock.snapshot(), plan);

    // Then: only the actual visible-frame deadline paints.
    assert_eq!(
        (armed.paint, first.paint, second.paint, due.paint),
        (false, false, false, true)
    );
}

#[test]
fn until_demand_wakes_once_at_exact_wall_clock_deadline_then_parks() {
    // Given: a semantic completion deadline at 400 ms with no periodic motion.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    let plan = MotionPlan::from_demand(MotionDemand::until(Duration::from_millis(400)));
    pacer.poll(clock.snapshot(), plan);

    // When: time reaches the exact deadline and the completed plan becomes idle.
    clock.advance_animation(400);
    let due = pacer.poll(clock.snapshot(), plan);
    let parked = pacer.poll(clock.snapshot(), MotionPlan::none());

    // Then: completion paints once and leaves no timer armed.
    assert!(due.paint);
    assert_eq!(parked.next_wait_ms, None);
}

#[test]
fn reduced_motion_retains_semantic_expiry_without_periodic_redraw() {
    // Given: reduced motion with one wall-clock semantic expiry.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::with_reduced_motion(true);
    let plan = MotionPlan::from_demand(MotionDemand::until(Duration::from_millis(400)));

    // When: the expiry is armed, sampled early, and reached exactly.
    let armed = pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(399);
    let early = pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(1);
    let due = pacer.poll(clock.snapshot(), plan);

    // Then: no periodic frame occurs and the semantic completion paints once.
    assert_eq!((armed.paint, early.paint, due.paint), (false, false, true));
}

#[test]
fn unchanged_physical_submission_suppresses_identical_periodic_motion() {
    // Given: a due periodic motion frame whose physical output is unchanged.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    let plan = MotionPlan::from_demand(MotionDemand::fast(Duration::from_millis(33)))
        .with_revision(7)
        .with_visual_sample(1);
    pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(33);
    assert!(pacer.poll(clock.snapshot(), plan).paint);
    let (mut output, _writer, _receiver) = FrameOutput::bounded(1);
    output.begin_frame().expect("begin unchanged frame");
    let submission = output.finish_frame().expect("finish unchanged frame");
    assert_eq!(submission, FrameSubmission::Unchanged);

    // When: the production presentation outcome is fed back to the runtime pacer.
    pacer.record_submission(submission, plan);

    // Then: one unchanged natural probe parks static motion until a semantic revision.
    assert_eq!(pacer.next_wait_ms(clock.snapshot()), Some(33));
    assert!(!pacer.needs_poll(clock.snapshot(), plan));
    clock.advance_animation(33);
    assert!(pacer.needs_poll(clock.snapshot(), plan));
    assert!(!pacer.poll(clock.snapshot(), plan).paint);
    assert_eq!(pacer.next_wait_ms(clock.snapshot()), None);
    let revised = plan.with_revision(8);
    assert!(pacer.needs_poll(clock.snapshot(), revised));
    assert!(!pacer.poll(clock.snapshot(), revised).paint);
    assert_eq!(pacer.next_wait_ms(clock.snapshot()), Some(33));
}

#[test]
fn changed_physical_submission_keeps_periodic_motion_rearming() {
    // Given: a due periodic frame produced visible output.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    let plan = MotionPlan::from_demand(MotionDemand::fast(Duration::from_millis(33)))
        .with_revision(7)
        .with_visual_sample(1);
    pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(33);
    assert!(pacer.poll(clock.snapshot(), plan).paint);

    // When: the physical submission reports a changed differential frame.
    pacer.record_submission(FrameSubmission::Accepted(FrameKind::Differential), plan);

    // Then: the same semantic wave remains armed for its next visible sample.
    assert_eq!(pacer.next_wait_ms(clock.snapshot()), Some(33));
    assert!(!pacer.needs_poll(clock.snapshot(), plan));
    clock.advance_animation(33);
    assert!(pacer.poll(clock.snapshot(), plan).paint);
}

#[test]
fn unchanged_sample_probes_a_later_visual_boundary_without_duplicate_cadence() {
    // Given: a quantized wave produces unchanged cells at its first due sample.
    let clock = DualClock::new();
    let mut pacer = RuntimePacer::new();
    let plan =
        MotionPlan::from_demand(MotionDemand::fast(Duration::from_millis(33))).with_revision(7);
    pacer.poll(clock.snapshot(), plan);
    clock.advance_animation(33);
    assert!(pacer.poll(clock.snapshot(), plan).paint);
    pacer.record_submission(FrameSubmission::Unchanged, plan);

    // When: incidental work observes the same visual sample before its next boundary.
    assert!(!pacer.needs_poll(clock.snapshot(), plan));

    // Then: the next sample under the same semantic revision re-arms naturally.
    clock.advance_animation(33);
    let next_sample = plan.with_visual_sample(2);
    assert!(pacer.needs_poll(clock.snapshot(), next_sample));
    assert!(pacer.poll(clock.snapshot(), next_sample).paint);
    pacer.record_submission(
        FrameSubmission::Accepted(FrameKind::Differential),
        next_sample,
    );
    assert_eq!(pacer.next_wait_ms(clock.snapshot()), Some(33));
}
