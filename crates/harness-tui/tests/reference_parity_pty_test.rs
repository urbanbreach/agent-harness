//! Reference-parity PTY owners for first-slice and overlay rows.
//!
//! These tests replace `pending:` manifest stubs with real fail-closed PTY
//! owners. They require `HARNESS_TUI_PTY_SIGNOFF=1` on Linux (same gate as
//! `pty_e2e`); without the env they no-op so ordinary nextest stays offline.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration PTY tests use fail-fast asserts"
)]

#[path = "support/reference_parity_pty_impl.rs"]
mod reference_parity_pty_impl;

#[test]
fn startup_welcome_panel_renders_and_focuses_composer() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::startup_welcome_panel_renders_and_focuses_composer();
}

#[test]
fn startup_breadcrumb_warning_visible() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::startup_breadcrumb_warning_visible();
}

#[test]
fn startup_type_dismisses_welcome() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::startup_type_dismisses_welcome();
}

#[test]
fn composer_bordered_strip_visible() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::composer_bordered_strip_visible();
}

#[test]
fn shortcut_footer_updates_on_draft() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shortcut_footer_updates_on_draft();
}

#[test]
fn ovl_palette_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::ovl_palette_pty();
}

#[test]
fn ovl_session_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::ovl_session_pty();
}

#[test]
fn ovl_perm_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::ovl_perm_pty();
}

#[test]
fn shell_idle_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_idle_pty();
}

#[test]
fn shell_perm_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_perm_pty();
}

#[test]
fn shell_stream_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_stream_pty();
}

#[test]
fn shell_fail_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_fail_pty();
}

#[test]
fn shell_complete_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_complete_pty();
}

#[test]
fn tx_user_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::tx_user_pty();
}

#[test]
fn tx_assistant_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::tx_assistant_pty();
}

#[test]
fn shell_cancel_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_cancel_pty();
}

#[test]
fn shell_recover_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_recover_pty();
}

#[test]
fn shell_scroll_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_scroll_pty();
}

#[test]
fn tx_tool_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::tx_tool_pty();
}

#[test]
fn tx_diff_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::tx_diff_pty();
}

#[test]
fn shell_question_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::shell_question_pty();
}

#[test]
fn ovl_question_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::ovl_question_pty();
}

#[test]
fn resp_60x20_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::resp_60x20_pty();
}

#[test]
fn resp_79x24_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::resp_79x24_pty();
}

#[test]
fn resp_80x24_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::resp_80x24_pty();
}

#[test]
fn resp_100x30_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::resp_100x30_pty();
}

#[test]
fn resp_120x40_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::resp_120x40_pty();
}

#[test]
fn resp_120x50_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::resp_120x50_pty();
}

#[test]
fn resp_wide_pty() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::resp_wide_pty();
}

#[test]
fn reference_parity_pty_helper_type_first_startup() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_type_first_startup();
}

#[test]
fn pty_helper_idle_shell() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_idle_shell();
}

#[test]
fn pty_helper_live_draft() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_draft();
}

#[test]
fn pty_helper_live_stream() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_stream();
}

#[test]
fn pty_helper_live_perm_stream() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_perm_stream();
}

#[test]
fn pty_helper_live_fail() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_fail();
}

#[test]
fn pty_helper_live_complete() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_complete();
}

#[test]
fn pty_helper_live_markdown() {
    reference_parity_pty_impl::pty_helper_live_markdown();
}

#[test]
fn pty_helper_live_cancel() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_cancel();
}

#[test]
fn pty_helper_live_recover() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_recover();
}

#[test]
fn pty_helper_live_tool() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_tool();
}

#[test]
fn pty_helper_live_tool_running() {
    reference_parity_pty_impl::pty_helper_live_tool_running();
}

#[test]
fn pty_helper_live_tool_finish_transition() {
    reference_parity_pty_impl::pty_helper_live_tool_finish_transition();
}

#[test]
fn pty_helper_live_tool_group_finish_transition() {
    reference_parity_pty_impl::pty_helper_live_tool_group_finish_transition();
}

#[test]
fn pty_helper_live_diff() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_diff();
}

#[test]
fn pty_helper_live_scroll() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_scroll();
}

#[test]
fn pty_helper_question_overlay() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_question_overlay();
}

#[test]
fn pty_helper_plan_composer() {
    // Given: the test binary is launched as the isolated plan-composer helper.
    // When: the matching scenario environment is active.
    // Then: the helper owns the real PTY lifecycle and renders until driven by the caller.
    reference_parity_pty_impl::pty_helper_plan_composer();
}

#[test]
fn pty_helper_live_question_stream() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_question_stream();
}

#[test]
fn pty_helper_live_thinking() {
    reference_parity_pty_impl::pty_helper_live_thinking();
}

#[test]
fn pty_helper_live_block_grammar() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_live_block_grammar();
}

#[test]
fn reference_parity_pty_helper_permission_overlay() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_permission_overlay();
}

#[test]
fn pty_helper_permission_overlay_empty_draft() {
    // arrange
    // act
    // assert
    reference_parity_pty_impl::pty_helper_permission_overlay_empty_draft();
}
