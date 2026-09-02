#[path = "support/p1_03_artifact_guard_test.rs"]
mod artifact_guard_test;
#[path = "support/p1_03_pty.rs"]
mod scenario;
#[path = "support/p1_03_recorded_owner.rs"]
mod support;

#[test]
fn p1_03_pty_helper() {
    // arrange: direct invocation opts into the deterministic startup-reveal fixture.
    // act: the helper test is launched by the native PTY owner.
    // assert: the owner drives and terminates the real TUI through its PTY.
    scenario::run_helper();
}

#[test]
fn p1_03_native_pty_owner_records_startup_reveal_terminal_states() {
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // arrange: an isolated artifact root selected by the signoff lane.
    // act: the serialized native PTY owner captures every variant and size.
    // assert: the complete manifest-backed artifact tree and receipts are verified.
    support::record_startup_reveal_terminal_states();
}
