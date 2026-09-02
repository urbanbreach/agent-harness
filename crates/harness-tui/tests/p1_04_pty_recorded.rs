#[path = "support/p1_04_pty.rs"]
mod scenario;
#[path = "support/p1_04_recorded_owner.rs"]
mod support;

#[test]
fn p1_04_pty_helper() {
    // Given: direct invocation opts into the deterministic responsive fixture.
    // When: the helper test is launched by the native PTY owner.
    scenario::run_helper();
    // Then: the owner drives and terminates the real TUI through its PTY.
}

#[test]
fn p1_04_native_pty_owner_records_responsive_terminal_states() {
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // Given: an isolated artifact root selected by the signoff lane.
    // When: the serialized native PTY owner captures every variant and size.
    // Then: the complete manifest-backed artifact tree and receipts are verified.
    support::record_responsive_terminal_states();
}
