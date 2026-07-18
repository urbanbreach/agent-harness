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
    reference_parity_pty_impl::startup_welcome_panel_renders_and_focuses_composer();
}

#[test]
fn startup_breadcrumb_warning_visible() {
    reference_parity_pty_impl::startup_breadcrumb_warning_visible();
}

#[test]
fn startup_type_dismisses_welcome() {
    reference_parity_pty_impl::startup_type_dismisses_welcome();
}

#[test]
fn composer_bordered_strip_visible() {
    reference_parity_pty_impl::composer_bordered_strip_visible();
}

#[test]
fn shortcut_footer_updates_on_draft() {
    reference_parity_pty_impl::shortcut_footer_updates_on_draft();
}

#[test]
fn ovl_palette_pty() {
    reference_parity_pty_impl::ovl_palette_pty();
}

#[test]
fn ovl_session_pty() {
    reference_parity_pty_impl::ovl_session_pty();
}

#[test]
fn ovl_help_pty() {
    reference_parity_pty_impl::ovl_help_pty();
}

#[test]
fn ovl_perm_pty() {
    reference_parity_pty_impl::ovl_perm_pty();
}

#[test]
fn shell_idle_pty() {
    reference_parity_pty_impl::shell_idle_pty();
}

#[test]
fn shell_perm_pty() {
    reference_parity_pty_impl::shell_perm_pty();
}

#[test]
fn shell_stream_pty() {
    reference_parity_pty_impl::shell_stream_pty();
}

#[test]
fn shell_fail_pty() {
    reference_parity_pty_impl::shell_fail_pty();
}

#[test]
fn shell_complete_pty() {
    reference_parity_pty_impl::shell_complete_pty();
}

#[test]
fn tx_user_pty() {
    reference_parity_pty_impl::tx_user_pty();
}

#[test]
fn tx_assistant_pty() {
    reference_parity_pty_impl::tx_assistant_pty();
}

#[test]
fn shell_cancel_pty() {
    reference_parity_pty_impl::shell_cancel_pty();
}

#[test]
fn shell_recover_pty() {
    reference_parity_pty_impl::shell_recover_pty();
}

#[test]
fn shell_scroll_pty() {
    reference_parity_pty_impl::shell_scroll_pty();
}

#[test]
fn tx_tool_pty() {
    reference_parity_pty_impl::tx_tool_pty();
}

#[test]
fn tx_diff_pty() {
    reference_parity_pty_impl::tx_diff_pty();
}

#[test]
fn shell_question_pty() {
    reference_parity_pty_impl::shell_question_pty();
}

#[test]
fn ovl_question_pty() {
    reference_parity_pty_impl::ovl_question_pty();
}

#[test]
fn resp_60x20_pty() {
    reference_parity_pty_impl::resp_60x20_pty();
}

#[test]
fn resp_79x24_pty() {
    reference_parity_pty_impl::resp_79x24_pty();
}

#[test]
fn resp_80x24_pty() {
    reference_parity_pty_impl::resp_80x24_pty();
}

#[test]
fn resp_100x30_pty() {
    reference_parity_pty_impl::resp_100x30_pty();
}

#[test]
fn resp_120x40_pty() {
    reference_parity_pty_impl::resp_120x40_pty();
}

#[test]
fn resp_120x50_pty() {
    reference_parity_pty_impl::resp_120x50_pty();
}

#[test]
fn resp_wide_pty() {
    reference_parity_pty_impl::resp_wide_pty();
}

#[test]
fn pty_helper_type_first_startup() {
    reference_parity_pty_impl::pty_helper_type_first_startup();
}

#[test]
fn pty_helper_live_draft() {
    reference_parity_pty_impl::pty_helper_live_draft();
}

#[test]
fn pty_helper_live_stream() {
    reference_parity_pty_impl::pty_helper_live_stream();
}

#[test]
fn pty_helper_live_fail() {
    reference_parity_pty_impl::pty_helper_live_fail();
}

#[test]
fn pty_helper_live_complete() {
    reference_parity_pty_impl::pty_helper_live_complete();
}

#[test]
fn pty_helper_live_cancel() {
    reference_parity_pty_impl::pty_helper_live_cancel();
}

#[test]
fn pty_helper_live_recover() {
    reference_parity_pty_impl::pty_helper_live_recover();
}

#[test]
fn pty_helper_live_tool() {
    reference_parity_pty_impl::pty_helper_live_tool();
}

#[test]
fn pty_helper_live_diff() {
    reference_parity_pty_impl::pty_helper_live_diff();
}

#[test]
fn pty_helper_live_scroll() {
    reference_parity_pty_impl::pty_helper_live_scroll();
}

#[test]
fn pty_helper_question_overlay() {
    reference_parity_pty_impl::pty_helper_question_overlay();
}

#[test]
fn pty_helper_permission_overlay() {
    reference_parity_pty_impl::pty_helper_permission_overlay();
}
