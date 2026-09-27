#[test]
fn browser_launcher_rejects_non_web_urls_before_running_a_command() {
    for url in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "--help",
        "https://user:password@example.test",
        "https://example.test/\nargument",
    ] {
        assert!(
            harness_core::browser_oidc::launch_browser(url).is_err(),
            "accepted {url}"
        );
    }
}
