#[path = "support/manual_live_turn_visual_capture_events.rs"]
mod capture_events;
#[path = "support/manual_live_turn_visual_capture_runtime.rs"]
mod capture_runtime;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn capture_live_turn_state_from_environment() -> TestResult {
    // arrange
    // act
    let Ok(name) = std::env::var("HARNESS_TUI_MANUAL_SCENARIO") else {
        return Ok(());
    };

    // assert
    capture_runtime::run_capture(capture_events::scenario(&name)?)
}

#[test]
fn unknown_manual_capture_scenario_is_rejected() {
    // arrange
    // act
    // assert
    assert!(capture_events::scenario("unknown").is_err());
}
