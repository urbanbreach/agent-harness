#[path = "support/pty_e2e_impl.rs"]
mod pty_e2e_impl;

#[test]
fn pty_smoke_starts_accepts_input_resizes_and_exits() {
    pty_e2e_impl::pty_smoke_starts_accepts_input_resizes_and_exits();
}

#[test]
fn pty_onboarding_auth_drives_required_screens() {
    pty_e2e_impl::pty_onboarding_auth_drives_required_screens();
}

#[test]
fn pty_helper_type_first_startup() {
    pty_e2e_impl::pty_helper_type_first_startup();
}

#[test]
fn pty_helper_onboarding_auth() {
    pty_e2e_impl::pty_helper_onboarding_auth();
}
