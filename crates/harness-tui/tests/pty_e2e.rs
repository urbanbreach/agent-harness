#[path = "support/manual_live_turn_visual_capture_events.rs"]
mod capture_events;
#[path = "support/manual_live_turn_visual_capture_runtime.rs"]
mod capture_runtime;
#[path = "support/p0_06_artifact_support.rs"]
mod p0_06_artifact_support;
#[path = "support/p0_06_artifacts.rs"]
mod p0_06_artifacts;
#[path = "support/p0_06_terminal_emulator.rs"]
mod p0_06_terminal_emulator;
#[path = "support/pty_e2e_impl.rs"]
mod pty_e2e_impl;

#[test]
fn pty_smoke_starts_accepts_input_resizes_and_exits() {
    pty_e2e_impl::pty_smoke_starts_accepts_input_resizes_and_exits();
}

#[test]
fn pty_scroll_follow_requires_second_clamped_page_down() {
    pty_e2e_impl::pty_scroll_follow_requires_second_clamped_page_down();
}

#[test]
fn pty_connect_auth_drives_provider_connection() {
    pty_e2e_impl::pty_connect_auth_drives_provider_connection();
}

#[test]
fn pty_permission_overlay_resolves_and_preserves_draft() {
    pty_e2e_impl::pty_permission_overlay_resolves_and_preserves_draft();
}

#[test]
fn pty_status_dialog_opens_without_sidebar_copy() {
    pty_e2e_impl::pty_status_dialog_opens_without_sidebar_copy();
}

#[test]
fn pty_waiting_for_response_matches_grok_layout_and_timer_motion() {
    pty_e2e_impl::pty_waiting_for_response_matches_grok_layout_and_timer_motion();
}

#[test]
fn pty_draft_esc_esc_clears_composer() {
    pty_e2e_impl::pty_draft_esc_esc_clears_composer();
}

#[test]
fn p0_06_native_pty_forwards_terminal_query_replies() {
    p0_06_terminal_emulator::native_pty_forwards_terminal_query_replies();
}

#[test]
fn p0_06_emulator_replies_at_query_position_within_one_chunk() {
    p0_06_terminal_emulator::emulator_replies_with_cursor_state_at_query_in_same_chunk();
}

#[test]
fn p0_06_native_pty_writes_canonical_artifacts_and_provenance() {
    p0_06_artifacts::native_pty_writes_canonical_artifacts_and_provenance();
}

#[test]
fn p0_06_emulator_structures_terminal_state_and_scrollback() {
    p0_06_terminal_emulator::emulator_structures_terminal_state_and_scrollback();
}

#[test]
fn pty_helper_type_first_startup() {
    pty_e2e_impl::pty_helper_type_first_startup();
}

#[test]
fn pty_helper_scroll_follow() {
    pty_e2e_impl::pty_helper_scroll_follow();
}

#[test]
fn pty_helper_connect_auth() {
    pty_e2e_impl::pty_helper_connect_auth();
}

#[test]
fn pty_helper_permission_overlay() {
    pty_e2e_impl::pty_helper_permission_overlay();
}

#[test]
fn pty_helper_waiting_for_response() {
    pty_e2e_impl::pty_helper_waiting_for_response();
}
