use harness_tui::scheduling::{DualClock, FrameInputs, FrameScheduler};
use harness_tui::transcript_identity::{BlockId, ReplayTurn, TranscriptIdentity};
use harness_tui::transcript_scroll::{
    DragAutoscroll, DragViewport, EasingKind, FollowState, FractionalScroll, LogicalAnchor,
    MotionPreference, ScrollTransition, ScrollbarDrag, ScrollbarGeometry, TranscriptLayout,
    TransitionRequest,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9);
}

fn stable_block_ids() -> Result<Vec<BlockId>, Box<dyn std::error::Error>> {
    let identity = TranscriptIdentity::from_replay([
        ReplayTurn::event(10, 0, 1),
        ReplayTurn::event(20, 1, 1),
        ReplayTurn::event(30, 2, 1),
        ReplayTurn::event(40, 3, 1),
        ReplayTurn::event(50, 4, 1),
        ReplayTurn::event(60, 5, 1),
    ])?;
    Ok(identity
        .turns()
        .iter()
        .map(|turn| turn.blocks()[0].id())
        .collect())
}

#[test]
fn fractional_scroll_conserves_signed_distance_for_property_inputs() {
    // Given: many same-direction fractional gesture streams.
    for run in 1..=128_u32 {
        let direction = if run % 2 == 0 { 1.0 } else { -1.0 };
        let deltas = (1..=run).map(|step| direction * f64::from(step) / 7.0);
        let total = deltas.clone().sum::<f64>();
        let mut scroll = FractionalScroll::new();

        // When: gesture deltas are reduced to integral scroll steps.
        let emitted = deltas.map(|delta| scroll.push(delta)).sum::<i32>();

        // Then: integral output plus the retained carry equals the input distance.
        assert_close(f64::from(emitted) + scroll.fractional_carry(), total);
    }
}

#[test]
fn logical_anchor_survives_concurrent_append_fold_and_resize() -> TestResult {
    // Given: replay-derived IDs and a viewport whose visible point is inside block three.
    let ids = stable_block_ids()?;
    for scenario in 1..=64_u32 {
        let before = TranscriptLayout::from_heights(
            vec![
                (ids[0], 4.0 + f64::from(scenario % 3)),
                (ids[1], 5.0),
                (ids[2], 3.0 + f64::from(scenario % 2)),
                (ids[3], 8.0),
                (ids[4], 6.0),
            ],
            10.0,
        )?;
        let point = before.top_of(ids[3])? + 1.25;
        let anchor = LogicalAnchor::capture(&before, point)?;

        // When: an earlier block folds, the anchored block resizes, a block appends,
        // and the viewport is resized in the same projection update.
        let after = TranscriptLayout::from_heights(
            vec![
                (ids[0], 1.0),
                (ids[1], 2.0 + f64::from(scenario % 2)),
                (ids[2], 1.0),
                (ids[3], 4.0 + f64::from(scenario % 3)),
                (ids[4], 20.0),
                (ids[5], 3.0),
            ],
            16.0 + f64::from(scenario % 5),
        )?;
        let restored = anchor.resolve(&after)?;

        // Then: the same logical block and intra-block distance are retained.
        assert_eq!(anchor.block_id(), ids[3]);
        assert_close(anchor.within_block(), 1.25);
        assert_close(restored, after.top_of(ids[3])? + 1.25);
    }
    Ok(())
}

#[test]
fn follow_detaches_on_user_scroll_up_and_reattaches_at_bottom_jump() -> TestResult {
    // Given: a following viewport at the content bottom.
    let mut follow = FollowState::new();
    assert!(follow.is_following());
    assert_close(follow.offset(), 0.0);

    // When: the user scrolls upward, then explicitly jumps to the bottom.
    follow.scroll_by(12.5, 100.0)?;
    assert!(!follow.is_following());
    follow.jump_to_bottom();

    // Then: follow mode is reattached at the exact bottom offset.
    assert!(follow.is_following());
    assert_close(follow.offset(), 0.0);
    Ok(())
}

#[test]
fn page_and_jump_easing_are_deterministic_with_a_fake_clock() -> TestResult {
    // Given: a full-motion page transition and a deterministic animation clock.
    let request = TransitionRequest::new(0.0, 100.0, 0, EasingKind::Page, MotionPreference::Full);
    let transition = ScrollTransition::start(request)?;
    let clock = DualClock::new();
    let first = transition.sample(clock.animation_now());
    let repeated = transition.sample(clock.animation_now());

    // When: the fake clock is sampled at the same point and after settling.
    let settled = transition.sample(200);

    // Then: equal clock inputs produce equal frames and the target is reached exactly.
    assert_close(first.value, repeated.value);
    assert!(!first.settled);
    assert!(settled.settled);
    assert_close(settled.value, 100.0);
    Ok(())
}

#[test]
fn scrollbar_drag_keeps_the_pointer_grab_anchor() -> TestResult {
    // Given: a scroll track and a fractional scroll offset.
    let geometry = ScrollbarGeometry::new(0.0, 100.0, 1_000.0, 100.0)?;
    let ids = stable_block_ids()?;
    let layout = TranscriptLayout::from_heights(vec![(ids[0], 8.0), (ids[1], 8.0)], 4.0)?;
    let logical_anchor = LogicalAnchor::capture(&layout, 3.0)?;
    let offset = 250.5;
    let thumb_start = geometry.thumb_start(offset)?;
    let pointer = thumb_start + geometry.thumb_extent() / 2.0;
    let mut drag =
        ScrollbarDrag::begin(pointer, offset, &geometry)?.with_logical_anchor(logical_anchor);

    // When: the pointer moves and then returns to its original position.
    let moved = drag.move_to(pointer + 10.0, &geometry)?;
    let returned = drag.move_to(pointer, &geometry)?;

    // Then: the thumb follows the pointer without a grab-point jump.
    assert!(moved > offset);
    assert_close(returned, offset);
    assert_close(drag.grab_offset(), geometry.thumb_extent() / 2.0);
    assert_eq!(drag.logical_anchor(), Some(logical_anchor));
    Ok(())
}

#[test]
fn reduced_motion_makes_every_transition_instant() -> TestResult {
    // Given: a reduced-motion jump request.
    let request = TransitionRequest::new(
        12.0,
        480.0,
        900,
        EasingKind::Jump,
        MotionPreference::ReducedMotion,
    );

    // When: the transition is sampled at its start time.
    let frame = ScrollTransition::start(request)?.sample(900);

    // Then: the target is already settled and no animation deadline exists.
    assert!(frame.settled);
    assert_close(frame.value, 480.0);
    assert!(!frame.needs_redraw);
    Ok(())
}

#[test]
fn settled_scroll_produces_zero_idle_scheduler_redraws() -> TestResult {
    // Given: a scheduled transition and the task-10 fake clock.
    let clock = DualClock::new();
    let transition = ScrollTransition::start(TransitionRequest::new(
        0.0,
        40.0,
        0,
        EasingKind::Page,
        MotionPreference::Full,
    ))?;
    let mut scheduler = FrameScheduler::new();
    let _ = scheduler.schedule(clock.snapshot(), FrameInputs::active());

    // When: the transition has settled and no input is pending.
    let settled = transition.sample(200);
    let idle = scheduler.schedule(
        clock.snapshot(),
        if settled.needs_redraw {
            FrameInputs::active()
        } else {
            FrameInputs::idle()
        },
    );

    // Then: the fake scheduler returns no idle frame.
    assert!(settled.settled);
    assert!(idle.is_none());
    Ok(())
}

#[test]
fn edge_drag_autoscroll_extends_selection_on_scheduler_deadlines() -> TestResult {
    // Given: an active drag and a viewport edge zone.
    let viewport = DragViewport::new(0.0, 20.0, 3.0)?;
    let mut drag = DragAutoscroll::new();
    let clock = DualClock::new();
    drag.start(10, clock.flush_now());
    drag.update_pointer(19.0, &viewport, clock.flush_now())?;
    assert!(drag.next_deadline_ms().is_some());

    // When: the task-10 flush deadline elapses at the lower edge.
    clock.tick_flush();
    let step = drag
        .tick(clock.flush_now())
        .ok_or("expected an autoscroll step")?;

    // Then: the scroll and selection extend forward, and leaving the edge idles.
    assert!(step.scroll_delta > 0.0);
    assert!(step.selection_delta > 0);
    assert_eq!(drag.selection_end(), 11);
    drag.update_pointer(10.0, &viewport, clock.flush_now())?;
    assert!(drag.next_deadline_ms().is_none());
    assert!(drag.tick(clock.flush_now() + 16).is_none());
    Ok(())
}
