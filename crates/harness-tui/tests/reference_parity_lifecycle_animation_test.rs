//! Reference-parity lifecycle and animation timing tests.
//!
//! The lifecycle clock is deliberately tested as a value object: the semantic
//! 30 Hz root and the independent 12 Hz startup shimmer never need wall-clock
//! sleeps.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast assertions"
)]

use std::sync::Arc;

use harness_core::clock::{Clock, FakeClock};
use harness_tui::app::AppState;
use harness_tui::render_test::render_to_buffer;
use harness_tui::terminal::{FrameClock, DEFAULT_FRAME_TICK_MS};
use harness_tui::ui;
use ratatui::layout::Rect;

#[test]
fn frame_clock_starts_at_zero_phase_and_mono_ms() {
    // Given/When: a new lifecycle frame clock.
    let clock = FrameClock::new();

    // Then: it starts at the reference origin and 30 Hz step.
    assert_eq!(clock.mono_ms(), 0);
    assert_eq!(clock.phase().get(), 0);
    assert_eq!(clock.tick_ms(), DEFAULT_FRAME_TICK_MS);
    assert_eq!(DEFAULT_FRAME_TICK_MS, 1_000 / 30);
}

#[test]
fn frame_clock_single_tick_advances_one_phase_and_one_step() {
    // Given
    let mut clock = FrameClock::new();

    // When
    clock.tick();

    // Then
    assert_eq!(clock.mono_ms(), 1_000 / 30);
    assert_eq!(clock.phase().get(), 1);
}

#[test]
fn frame_clock_fixed_tick_plan_advances_deterministically() {
    // Given: six fixed root ticks at the authoritative 30 Hz cadence.
    let mut clock = FrameClock::new();

    // When
    clock.tick_n(6);

    // Then
    assert_eq!(clock.mono_ms(), 6 * (1_000 / 30));
    assert_eq!(clock.phase().get(), 6);
}

#[test]
fn frame_clock_explicit_advance_records_one_phase_per_call() {
    // Given
    let mut clock = FrameClock::new();

    // When: explicit millisecond advances are used by lifecycle adapters.
    clock.advance(250);
    clock.advance(125);

    // Then: elapsed time sums while phase counts calls.
    assert_eq!(clock.mono_ms(), 375);
    assert_eq!(clock.phase().get(), 2);
}

#[test]
fn frame_clock_shares_its_backing_clock_with_consumers() {
    // Given: an evidence consumer sharing the lifecycle clock.
    let shared = Arc::new(FakeClock::new());
    let mut clock = FrameClock::from_clock(Arc::clone(&shared));

    // When
    clock.tick_n(3);

    // Then: both consumers observe the same 30 Hz reading.
    assert_eq!(clock.mono_ms(), 3 * (1_000 / 30));
    assert_eq!(shared.mono_ms(), 3 * (1_000 / 30));
    assert_eq!(clock.clock().mono_ms(), 3 * (1_000 / 30));
}

#[test]
fn two_independent_frame_clocks_advance_identically() {
    // Given
    let mut clock_a = FrameClock::new();
    let mut clock_b = FrameClock::new();

    // When
    clock_a.tick_n(8);
    clock_b.tick_n(8);

    // Then: independent lifecycle captures remain byte-stable.
    assert_eq!(clock_a.mono_ms(), clock_b.mono_ms());
    assert_eq!(clock_a.mono_ms(), 8 * (1_000 / 30));
    assert_eq!(clock_a.phase(), clock_b.phase());
}

#[test]
fn startup_logo_shimmer_advances_on_each_twelve_fps_slow_tick() {
    // Given: an empty startup shell with its welcome-logo animation active.
    let mut app = AppState::new_startup(Vec::new(), None);
    assert!(app.has_active_animations_for_evidence());

    let render = |app: &AppState| {
        render_to_buffer(app, Rect::new(0, 0, 120, 32), |app, frame, _area| {
            ui::render_app(frame, app)
        })
    };
    let phase_zero = render(&app);

    // When: the startup scheduler delivers one Slow (12 Hz) tick.
    app.advance_animation_tick_for_evidence();
    let phase_one = render(&app);

    // Then: its phase advances the logo immediately; it is not bucketed by a
    // separate 30 Hz root clock.
    assert_ne!(phase_zero, phase_one);

    // And: each subsequent Slow tick produces the next deterministic frame.
    app.advance_animation_tick_for_evidence();
    let phase_two = render(&app);
    assert_ne!(phase_one, phase_two);
    assert_eq!(app.animation_phase_for_evidence(), 2);
}
