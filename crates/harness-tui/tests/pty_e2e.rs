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
fn pty_helper_type_first_startup() {
    pty_e2e_impl::pty_helper_type_first_startup();
}

#[test]
fn pty_helper_connect_auth() {
    pty_e2e_impl::pty_helper_connect_auth();
}
