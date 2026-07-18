#[path = "support/pty_e2e_impl.rs"]
mod pty_e2e_impl;

#[test]
fn pty_smoke_starts_accepts_input_resizes_and_exits() {
    pty_e2e_impl::pty_smoke_starts_accepts_input_resizes_and_exits();
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
fn pty_draft_esc_esc_clears_composer() {
    pty_e2e_impl::pty_draft_esc_esc_clears_composer();
}

#[test]
fn pty_helper_type_first_startup() {
    pty_e2e_impl::pty_helper_type_first_startup();
}

#[test]
fn pty_helper_connect_auth() {
    pty_e2e_impl::pty_helper_connect_auth();
}

#[test]
fn pty_helper_permission_overlay() {
    pty_e2e_impl::pty_helper_permission_overlay();
}
