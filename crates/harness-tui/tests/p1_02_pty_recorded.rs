#[path = "support/p1_02_pty.rs"]
mod scenario;
#[path = "support/p1_02_pty_support_recorded.rs"]
mod support;

#[test]
fn p1_02_pty_helper() {
    // arrange
    // act
    scenario::run_helper();
    // assert
}

#[test]
fn p1_02_real_pty_records_modal_chrome_at_canonical_sizes() {
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // arrange
    // act
    for (cols, rows) in [(80, 24), (120, 40), (160, 50)] {
        support::run_modal_journey(cols, rows);
    }
    // assert
}
