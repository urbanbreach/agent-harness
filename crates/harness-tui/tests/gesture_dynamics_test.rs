use harness_tui::app::interaction_reducer::render_purity::RenderPurityProbe;
use harness_tui::app::AppState;
use harness_tui::gestures::{
    classify, route_hit_target, DragLifecycle, GestureDevice, GestureEvent, GestureHistory,
    GestureKind, HitSurface, HitTarget, Point, ScrollDirection, ScrollGesture,
};
use harness_tui::render_test::render_to_buffer;
use harness_tui::ui;
use ratatui::layout::Rect;

#[test]
fn signed_scroll_lines_are_conserved_across_fractional_deltas() {
    // arrange
    for run in 1..=128 {
        let direction = if run % 2 == 0 { 1.0 } else { -1.0 };
        let deltas = (1..=run).map(|step| direction * f64::from(step) / 7.0);
        let total: f64 = deltas.clone().sum();
        let mut gesture = ScrollGesture::new(GestureDevice::Trackpad);
        let emitted: i32 = deltas.map(|delta| gesture.push(delta)).sum();

        // act
        let conserved = f64::from(emitted) + gesture.fractional_carry();
        // assert
        assert!((conserved - total).abs() < 1e-9);
    }
}

#[test]
fn device_classification_is_deterministic_and_boundary_aware() {
    // arrange
    let mut history = GestureHistory::default();
    let trackpad = GestureEvent::scroll(0.25, 10);
    let wheel = GestureEvent::scroll(1.0, 20);

    assert_eq!(
        classify(&trackpad, &history),
        GestureKind::scroll(GestureDevice::Trackpad)
    );
    history.observe(&trackpad);
    assert_eq!(
        classify(&wheel, &history),
        GestureKind::scroll(GestureDevice::Trackpad)
    );

    // act
    let boundary = GestureEvent::scroll(1.0, 91);
    // assert
    assert_eq!(
        classify(&boundary, &history),
        GestureKind::scroll(GestureDevice::Wheel)
    );
    assert_eq!(classify(&boundary, &history), classify(&boundary, &history));
}

#[test]
fn timestamped_scroll_flushes_every_sixteen_milliseconds() {
    // arrange
    // act
    let mut gesture = ScrollGesture::new(GestureDevice::Trackpad);

    // assert
    assert!(!gesture.push_at(0.25, 0).flush_due);
    assert!(!gesture.push_at(0.25, 15).flush_due);
    assert!(gesture.push_at(0.25, 16).flush_due);
}

#[test]
fn direction_change_resets_fractional_carry_and_gesture_generation() {
    // arrange
    // act
    let mut gesture = ScrollGesture::new(GestureDevice::Trackpad);
    let _ = gesture.push(0.75);
    let first_generation = gesture.generation();

    // assert
    assert_eq!(gesture.push(-0.25), 0);
    assert_eq!(gesture.direction(), ScrollDirection::Down);
    assert!((gesture.fractional_carry() - (-0.25)).abs() < 1e-9);
    assert!(gesture.generation() > first_generation);
}

#[test]
fn hit_routing_does_not_pass_through_covered_or_inactive_surfaces() {
    // arrange
    let surfaces = [
        HitSurface::new(HitTarget::Overlay, Rect::new(0, 0, 10, 10), true, true),
        HitSurface::new(HitTarget::Transcript, Rect::new(0, 0, 10, 10), true, false),
    ];
    assert_eq!(
        route_hit_target(Point::new(2, 2), &surfaces),
        HitTarget::None
    );

    let inactive = [
        HitSurface::new(HitTarget::Overlay, Rect::new(0, 0, 10, 10), false, false),
        HitSurface::new(HitTarget::Transcript, Rect::new(0, 0, 10, 10), true, false),
    ];
    assert_eq!(
        route_hit_target(Point::new(2, 2), &inactive),
        HitTarget::None
    );

    // act
    let active = [HitSurface::new(
        HitTarget::Inspector,
        Rect::new(0, 0, 10, 10),
        true,
        false,
    )];
    // assert
    assert_eq!(
        route_hit_target(Point::new(2, 2), &active),
        HitTarget::Inspector
    );
}

#[test]
fn drag_lifecycle_requires_ordered_begin_update_end() {
    // arrange
    // act
    let mut drag = DragLifecycle::new();
    let start = Point::new(2, 3);
    let finish = Point::new(8, 9);

    // assert
    assert!(drag.begin(start).is_ok());
    assert!(drag.update(finish).is_ok());
    let ended = drag.end(finish);
    assert!(ended.is_ok());
    assert!(!drag.is_active());
}

#[test]
fn gesture_render_path_has_no_clock_reads_or_render_side_effects() {
    // arrange
    let sources = [
        include_str!("../src/gestures/mod.rs"),
        include_str!("../src/gestures/device.rs"),
        include_str!("../src/gestures/scroll.rs"),
        include_str!("../src/gestures/drag.rs"),
        include_str!("../src/gestures/classifier.rs"),
        include_str!("../src/gestures/routing.rs"),
    ];
    for source in sources {
        assert!(!source.contains("Instant::now"));
        assert!(!source.contains("SystemTime::now"));
    }

    // act
    let app = AppState::new_live(None, false, None);
    let mut probe = RenderPurityProbe::default();
    let result = probe.draw(|_| {
        let _ = render_to_buffer(&app, Rect::new(0, 0, 80, 24), |app, frame, _| {
            ui::render_app(frame, app);
        });
    });
    // assert
    assert!(result.is_ok());
    assert!(probe.side_effects().is_empty());
}
