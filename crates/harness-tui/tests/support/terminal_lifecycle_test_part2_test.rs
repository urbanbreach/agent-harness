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

    // act
    // act / assert — enable, then disable, then disable-again fails
    lifecycle.enable_synchronized_output(&full).expect("enable");
    // assert
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

    // act
    // act / assert — enable, disable, then disable-again fails
    lifecycle.enable_bracketed_paste(&full).expect("enable");
    // assert
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
