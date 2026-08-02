//! Owner/contract tests for the clean-room terminal lifecycle shard (task 10).
//!
//! Locks deterministic leaf contracts for the terminal lifecycle, cursor,
//! synchronized-output writer, and frame clock. These are pure value-object
//! contracts — no terminal I/O is performed; the writer targets an in-memory
//! sink so the emitted escape bytes are inspectable.
//!
//! Proof dimensions covered: P1 (contract), P3 (terminal), P5 (motion — the
//! deterministic frame clock), P7 (lifecycle state transitions).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration owner tests use fail-fast asserts"
)]

use std::io;
use std::sync::Arc;

use harness_core::clock::{Clock, FakeClock};
use harness_tui::terminal::{
    AltScreenMode, CursorPosition, CursorShape, CursorState, FrameClock, ScreenBuffer,
    SynchronizedWriter, TeardownPlan, TerminalCapabilities, TerminalLifecycle,
    TerminalLifecycleError, DEFAULT_FRAME_TICK_MS, END_SYNCHRONIZED_UPDATE,
};

// ---------------------------------------------------------------------------
// Frame clock (P5 motion / deterministic timing)
// ---------------------------------------------------------------------------

#[test]
fn frame_clock_starts_at_zero_phase_and_mono_ms() {
    // arrange
    // act
    let clock = FrameClock::new();

    // assert
    assert_eq!(clock.mono_ms(), 0);
    assert_eq!(clock.phase().get(), 0);
    assert_eq!(clock.tick_ms(), DEFAULT_FRAME_TICK_MS);
    assert_eq!(DEFAULT_FRAME_TICK_MS, 1_000 / 30);
}

#[test]
fn frame_clock_single_tick_advances_one_phase_and_one_step() {
    // arrange
    let mut clock = FrameClock::new();

    // act
    clock.tick();

    // assert
    assert_eq!(clock.mono_ms(), 1_000 / 30);
    assert_eq!(clock.phase().get(), 1);
}

#[test]
fn frame_clock_fixed_tick_plan_advances_deterministically() {
    // arrange — mirror the animation fixed-tick cadence (100ms per frame)
    let mut clock = FrameClock::new().with_tick_ms(100);

    // act
    clock.tick_n(6);

    // assert — strictly monotonic: 6 frames * 100ms == 600ms, phase == 6
    assert_eq!(clock.mono_ms(), 600);
    assert_eq!(clock.phase().get(), 6);
}

#[test]
fn frame_clock_explicit_advance_records_one_phase_per_call() {
    // arrange
    let mut clock = FrameClock::new();

    // act — arbitrary millisecond steps, not aligned to the tick step
    clock.advance(250);
    clock.advance(125);

    // assert — mono_ms sums the steps; phase counts advance calls
    assert_eq!(clock.mono_ms(), 375);
    assert_eq!(clock.phase().get(), 2);
}

#[test]
fn frame_clock_shares_its_backing_clock_with_consumers() {
    // arrange — a shared clock handed to another consumer (e.g. animation evidence)
    let shared = Arc::new(FakeClock::new());
    let mut clock = FrameClock::from_clock(Arc::clone(&shared));

    // act
    clock.tick_n(3);

    // assert — both the frame clock and the shared handle observe the same reading
    assert_eq!(clock.mono_ms(), 3 * (1_000 / 30));
    assert_eq!(shared.mono_ms(), 3 * (1_000 / 30));
    assert_eq!(clock.clock().mono_ms(), 3 * (1_000 / 30));
}

#[test]
fn two_independent_frame_clocks_advance_identically() {
    // arrange
    let mut clock_a = FrameClock::new().with_tick_ms(100);
    let mut clock_b = FrameClock::new().with_tick_ms(100);

    // act — identical tick programs on independent clocks
    clock_a.tick_n(8);
    clock_b.tick_n(8);

    // assert — byte-stable, deterministic across runs
    assert_eq!(clock_a.mono_ms(), clock_b.mono_ms());
    assert_eq!(clock_a.mono_ms(), 800);
    assert_eq!(clock_a.phase(), clock_b.phase());
}

// ---------------------------------------------------------------------------
// Cursor control (P3 terminal)
// ---------------------------------------------------------------------------

#[test]
fn cursor_state_defaults_to_home_visible_default_shape() {
    // arrange
    // act
    let cursor = CursorState::new();

    // assert
    assert_eq!(cursor.position, CursorPosition::new(0, 0));
    assert!(cursor.is_visible());
    assert_eq!(cursor.shape, CursorShape::Default);
}

#[test]
fn cursor_move_to_sets_exact_position() {
    // arrange
    let cursor = CursorState::new();

    // act
    let moved = cursor.move_to(CursorPosition::new(5, 7));

    // assert
    assert_eq!(moved.position, CursorPosition::new(5, 7));
    assert!(moved.is_visible(), "moving must not change visibility");
}

#[test]
fn cursor_position_clamps_to_grid_bounds() {
    // arrange
    // act
    let inside = CursorPosition::new(3, 4).clamped(80, 24);
    let overflow = CursorPosition::new(500, 500).clamped(80, 24);
    let collapsed = CursorPosition::new(9, 9).clamped(0, 0);

    // assert
    assert_eq!(inside, CursorPosition::new(3, 4));
    assert_eq!(overflow, CursorPosition::new(79, 23));
    assert_eq!(collapsed, CursorPosition::new(0, 0));
}

#[test]
fn cursor_move_to_clamped_keeps_position_inside_grid() {
    // arrange
    let cursor = CursorState::new();

    // act
    let moved = cursor.move_to_clamped(CursorPosition::new(500, 500), 80, 24);

    // assert
    assert_eq!(moved.position, CursorPosition::new(79, 23));
}

#[test]
fn cursor_visibility_toggles_between_show_and_hide() {
    // arrange
    let cursor = CursorState::new();

    // act
    let hidden = cursor.hide();
    let shown = hidden.show();

    // assert
    assert!(cursor.is_visible(), "default cursor is visible");
    assert!(!hidden.is_visible());
    assert!(shown.is_visible());
}

#[test]
fn cursor_shape_changes_via_with_shape() {
    // arrange
    let cursor = CursorState::new();

    // act
    let block = cursor.with_shape(CursorShape::Block);
    let line = block.with_shape(CursorShape::Line);
    let underline = line.with_shape(CursorShape::Underline);

    // assert — shape changes preserve other fields
    assert_eq!(block.shape, CursorShape::Block);
    assert_eq!(line.shape, CursorShape::Line);
    assert_eq!(underline.shape, CursorShape::Underline);
    assert!(underline.is_visible());
    assert_eq!(underline.position, CursorPosition::new(0, 0));
}

// ---------------------------------------------------------------------------
// Synchronized-output writer (P3 terminal / DEC mode 2026)
// ---------------------------------------------------------------------------

#[test]
fn synchronized_writer_emits_begin_payload_end() {
    // arrange
    let mut writer = SynchronizedWriter::new(Vec::new());

    // act
    writer.begin_frame().expect("begin");
    writer.write_payload(b"hello").expect("write");
    writer.end_frame().expect("end");

    // assert
    assert_eq!(writer.into_inner(), b"\x1b[?2026hhello\x1b[?2026l".to_vec());
}

#[test]
fn synchronized_writer_elides_empty_frame() {
    // arrange
    let mut writer = SynchronizedWriter::new(Vec::new());

    // act — begin + end with no payload
    writer.begin_frame().expect("begin");
    writer.end_frame().expect("end");

    // assert — no markers emitted for a frame that drew nothing
    assert_eq!(writer.into_inner(), Vec::<u8>::new());
}

#[test]
fn synchronized_writer_elides_frame_with_only_empty_writes() {
    // arrange
    let mut writer = SynchronizedWriter::new(Vec::new());

    // act
    writer.begin_frame().expect("begin");
    writer.write_payload(b"").expect("empty write");
    writer.end_frame().expect("end");

    // assert — a zero-length write does not count as visible content
    assert_eq!(writer.into_inner(), Vec::<u8>::new());
}

#[test]
fn synchronized_writer_nesting_emits_a_single_marker_pair() {
    // arrange
    let mut writer = SynchronizedWriter::new(Vec::new());

    // act — nested begin/end frames
    writer.begin_frame().expect("outer begin");
    writer.begin_frame().expect("inner begin");
    assert_eq!(writer.depth(), 2);
    writer.write_payload(b"x").expect("write");
    writer.end_frame().expect("inner end");
    writer.end_frame().expect("outer end");

    // assert — exactly one synchronized pair around the single payload
    assert_eq!(writer.depth(), 0);
    assert!(!writer.begin_emitted());
    assert_eq!(writer.into_inner(), b"\x1b[?2026hx\x1b[?2026l".to_vec());
}

#[test]
fn synchronized_writer_end_without_begin_errors() {
    // arrange
    let mut writer = SynchronizedWriter::new(Vec::new());

    // act
    let err = writer.end_frame().expect_err("no open frame must fail");

    // assert
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(writer.depth(), 0);
}

#[test]
fn synchronized_writer_raii_guard_emits_a_balanced_frame() {
    // arrange
    let mut writer = SynchronizedWriter::new(Vec::new());

    // act — the guard closes the frame on drop
    {
        let mut guard = writer.frame().expect("begin via guard");
        guard.write_payload(b"ok").expect("write");
    }

    // assert
    assert_eq!(writer.depth(), 0);
    assert!(!writer.begin_emitted());
    assert_eq!(writer.into_inner(), b"\x1b[?2026hok\x1b[?2026l".to_vec());
}

#[test]
fn synchronized_writer_passes_through_writes_outside_a_frame() {
    // arrange
    let mut writer = SynchronizedWriter::new(Vec::new());

    // act — payload written with no open frame is raw pass-through
    let written = writer.write_payload(b"raw").expect("write");

    // assert — no synchronized markers, payload untouched
    assert_eq!(written, 3);
    assert_eq!(writer.into_inner(), b"raw".to_vec());
}

#[test]
fn synchronized_writer_begin_bytes_match_dec_private_mode_2026() {
    // arrange
    // act
    // assert — the constants must be the exact DEC mode 2026 set/reset sequences
    assert_eq!(
        harness_tui::terminal::BEGIN_SYNCHRONIZED_UPDATE,
        b"\x1b[?2026h".as_ref()
    );
    assert_eq!(END_SYNCHRONIZED_UPDATE, b"\x1b[?2026l".as_ref());
}

// ---------------------------------------------------------------------------
// Terminal lifecycle state machine (P7 lifecycle / P1 contract)
// ---------------------------------------------------------------------------

#[test]
fn raw_mode_enter_then_exit_succeeds() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();
    let caps = TerminalCapabilities::full();

    // act
    lifecycle.enter_raw_mode(&caps).expect("enter raw");
    let active = lifecycle.is_raw_mode_active();
    lifecycle.exit_raw_mode().expect("exit raw");

    // assert
    assert!(active);
    assert!(!lifecycle.is_raw_mode_active());
}

#[test]
fn raw_mode_enter_fails_closed_without_capability() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();

    // act
    let err = lifecycle
        .enter_raw_mode(&TerminalCapabilities::none())
        .expect_err("raw mode must fail closed when unsupported");

    // assert
    assert_eq!(err, TerminalLifecycleError::RawModeUnsupported);
    assert!(!lifecycle.is_raw_mode_active());
}

#[test]
fn raw_mode_exit_without_enter_errors() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();

    // act
    let err = lifecycle.exit_raw_mode().expect_err("exit when not active");

    // assert
    assert_eq!(err, TerminalLifecycleError::RawModeNotActive);
}

#[test]
fn raw_mode_enter_is_idempotent() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();
    let caps = TerminalCapabilities::full();

    // act — second enter is a no-op success (matches real raw-mode semantics)
    lifecycle.enter_raw_mode(&caps).expect("first enter");
    lifecycle.enter_raw_mode(&caps).expect("second enter");

    // assert
    assert!(lifecycle.is_raw_mode_active());
}

#[test]
fn alternate_screen_auto_enters_when_supported() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();

    // act
    lifecycle
        .enter_alternate_screen(&TerminalCapabilities::full(), AltScreenMode::Auto)
        .expect("auto enter");

    // assert
    assert_eq!(lifecycle.screen_buffer(), ScreenBuffer::Alternate);
}

#[test]
fn alternate_screen_auto_stays_main_when_unsupported() {
    // arrange — graceful degradation under Auto
    let mut lifecycle = TerminalLifecycle::new();

    // act
    lifecycle
        .enter_alternate_screen(&TerminalCapabilities::none(), AltScreenMode::Auto)
        .expect("auto degrades to main");

    // assert
    assert_eq!(lifecycle.screen_buffer(), ScreenBuffer::Main);
}

#[test]
fn alternate_screen_always_fails_closed_when_unsupported() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();

    // act — Always on an unsupported terminal must not silently stay on main
    let err = lifecycle
        .enter_alternate_screen(&TerminalCapabilities::none(), AltScreenMode::Always)
        .expect_err("always + unsupported must fail closed");

    // assert
    assert_eq!(err, TerminalLifecycleError::AlternateScreenUnsupported);
    assert_eq!(lifecycle.screen_buffer(), ScreenBuffer::Main);
}

#[test]
fn alternate_screen_never_stays_main_even_when_supported() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();

    // act
    lifecycle
        .enter_alternate_screen(&TerminalCapabilities::full(), AltScreenMode::Never)
        .expect("never honored");

    // assert
    assert_eq!(lifecycle.screen_buffer(), ScreenBuffer::Main);
}

#[test]
fn leave_alternate_screen_returns_to_main_idempotently() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();
    lifecycle
        .enter_alternate_screen(&TerminalCapabilities::full(), AltScreenMode::Auto)
        .expect("enter");

    // act
    lifecycle.leave_alternate_screen();
    lifecycle.leave_alternate_screen();

    // assert — leaving when already on main is a harmless no-op
    assert_eq!(lifecycle.screen_buffer(), ScreenBuffer::Main);
}

#[test]
fn synchronized_output_enable_disable_lifecycle() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();
    let full = TerminalCapabilities::full();

    // act / assert — enable requires capability
    let unsupported = lifecycle
        .enable_synchronized_output(&TerminalCapabilities::none())
        .expect_err("sync unsupported");
    assert_eq!(
        unsupported,
        TerminalLifecycleError::SynchronizedOutputUnsupported
    );

    // act / assert — enable, then disable, then disable-again fails
    lifecycle.enable_synchronized_output(&full).expect("enable");
    assert!(lifecycle.is_synchronized_active());
    lifecycle.disable_synchronized_output().expect("disable");
    assert!(!lifecycle.is_synchronized_active());
    let not_active = lifecycle
        .disable_synchronized_output()
        .expect_err("disable when inactive");
    assert_eq!(
        not_active,
        TerminalLifecycleError::SynchronizedOutputNotActive
    );
}

#[test]
fn bracketed_paste_enable_disable_lifecycle() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();
    let full = TerminalCapabilities::full();

    // act / assert — enable requires capability
    let unsupported = lifecycle
        .enable_bracketed_paste(&TerminalCapabilities::none())
        .expect_err("paste unsupported");
    assert_eq!(
        unsupported,
        TerminalLifecycleError::BracketedPasteUnsupported
    );

    // act / assert — enable, disable, then disable-again fails
    lifecycle.enable_bracketed_paste(&full).expect("enable");
    assert!(lifecycle.is_bracketed_paste_active());
    lifecycle.disable_bracketed_paste().expect("disable");
    assert!(!lifecycle.is_bracketed_paste_active());
    let not_active = lifecycle
        .disable_bracketed_paste()
        .expect_err("disable when inactive");
    assert_eq!(not_active, TerminalLifecycleError::BracketedPasteNotActive);
}

#[test]
fn teardown_plan_reflects_active_modes_and_clears_on_exit() {
    // arrange — fully activated terminal
    let mut lifecycle = TerminalLifecycle::new();
    let caps = TerminalCapabilities::full();
    lifecycle.enter_raw_mode(&caps).expect("raw");
    lifecycle
        .enter_alternate_screen(&caps, AltScreenMode::Always)
        .expect("alt");
    lifecycle.enable_synchronized_output(&caps).expect("sync");
    lifecycle.enable_bracketed_paste(&caps).expect("paste");

    // act
    let active_plan = lifecycle.teardown_plan();

    // assert — every active mode requires reversal
    assert_eq!(
        active_plan,
        TeardownPlan {
            disable_raw_mode: true,
            leave_alternate_screen: true,
            disable_synchronized_output: true,
            disable_bracketed_paste: true,
        }
    );

    // act — tear each mode down
    lifecycle.exit_raw_mode().expect("exit raw");
    lifecycle.leave_alternate_screen();
    lifecycle
        .disable_synchronized_output()
        .expect("disable sync");
    lifecycle.disable_bracketed_paste().expect("disable paste");

    // assert — nothing left to reverse
    assert_eq!(lifecycle.teardown_plan(), TeardownPlan::default());
}

#[test]
fn lifecycle_starts_in_the_terminal_default_state() {
    // arrange
    // act
    let lifecycle = TerminalLifecycle::new();

    // assert — cooked mode, main screen, no optional modes active
    assert!(!lifecycle.is_raw_mode_active());
    assert_eq!(lifecycle.screen_buffer(), ScreenBuffer::Main);
    assert!(!lifecycle.is_synchronized_active());
    assert!(!lifecycle.is_bracketed_paste_active());
    assert_eq!(lifecycle.teardown_plan(), TeardownPlan::default());
}

/// Capstone (P1 contract + P3 terminal + P7 lifecycle): a full-capability enter
/// sequence reaches a fully active state and the teardown plan round-trips it
/// back to the terminal default.
#[test]
fn full_capability_enter_sequence_round_trips_through_teardown() {
    // arrange
    let mut lifecycle = TerminalLifecycle::new();
    let caps = TerminalCapabilities::full();

    // act — enter the complete interactive session
    lifecycle.enter_raw_mode(&caps).expect("raw");
    lifecycle
        .enter_alternate_screen(&caps, AltScreenMode::Always)
        .expect("alt");
    lifecycle.enable_synchronized_output(&caps).expect("sync");
    lifecycle.enable_bracketed_paste(&caps).expect("paste");

    // assert — fully active
    assert!(lifecycle.is_raw_mode_active());
    assert_eq!(lifecycle.screen_buffer(), ScreenBuffer::Alternate);
    assert!(lifecycle.is_synchronized_active());
    assert!(lifecycle.is_bracketed_paste_active());

    // act — restore via the teardown plan decisions
    if lifecycle.teardown_plan().disable_raw_mode {
        lifecycle.exit_raw_mode().expect("exit raw");
    }
    if lifecycle.teardown_plan().leave_alternate_screen {
        lifecycle.leave_alternate_screen();
    }
    if lifecycle.teardown_plan().disable_synchronized_output {
        lifecycle
            .disable_synchronized_output()
            .expect("disable sync");
    }
    if lifecycle.teardown_plan().disable_bracketed_paste {
        lifecycle.disable_bracketed_paste().expect("disable paste");
    }

    // assert — round-tripped back to the default state
    assert_eq!(lifecycle, TerminalLifecycle::new());
    assert_eq!(lifecycle.teardown_plan(), TeardownPlan::default());
}
