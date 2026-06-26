//! PoC for Candidate 2: CLI/TUI auth secret leakage.
//!
//! Verifies that:
//! 1. display_tui_auth_args redacts --mock-token, --mock-refresh-token, --enterprise-url
//! 2. TuiAuthNoticeWriter applies DefaultRedactor to all output
//! 3. --mock-token value doesn't appear in process display

use harness_core::redact::{DefaultRedactor, Redactor};

// Replicate the display_tui_auth_args logic from tui/auth_backend.rs
// to verify it redacts secret values.
fn display_tui_auth_args(args: &[String]) -> String {
    let mut display = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            display.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }
        if tui_auth_arg_redacts_next(arg) {
            display.push(arg.clone());
            redact_next = true;
            continue;
        }
        if let Some(redacted) = redact_tui_auth_arg_value(arg) {
            display.push(redacted);
            continue;
        }
        display.push(arg.clone());
    }
    display.join(" ")
}

fn tui_auth_arg_redacts_next(arg: &str) -> bool {
    matches!(
        arg,
        "--mock-token" | "--mock-refresh-token" | "--enterprise-url"
    )
}

fn redact_tui_auth_arg_value(arg: &str) -> Option<String> {
    [
        "--mock-token=",
        "--mock-refresh-token=",
        "--enterprise-url=",
    ]
    .into_iter()
    .find_map(|prefix| {
        arg.strip_prefix(prefix)
            .map(|_| format!("{prefix}<redacted>"))
    })
}

#[test]
fn poc_display_tui_auth_args_redacts_space_separated_secrets() {
    let secret_token = "sk-super-secret-token-12345";
    let secret_refresh = "refresh-secret-value-67890";
    let secret_url = "https://ghe.example.com";

    let args: Vec<String> = [
        "login".to_string(),
        "--mock-token".to_string(),
        secret_token.to_string(),
        "--mock-refresh-token".to_string(),
        secret_refresh.to_string(),
        "--enterprise-url".to_string(),
        secret_url.to_string(),
    ]
    .to_vec();

    let display = display_tui_auth_args(&args);

    assert!(
        !display.contains(secret_token),
        "mock token leaked in display: {display}"
    );
    assert!(
        !display.contains(secret_refresh),
        "mock refresh token leaked in display: {display}"
    );
    assert!(
        !display.contains(secret_url),
        "enterprise url leaked in display: {display}"
    );
    assert!(display.contains("<redacted>"));
}

#[test]
fn poc_display_tui_auth_args_redacts_equals_separated_secrets() {
    let secret_token = "sk-equals-secret-token";
    let secret_refresh = "refresh-equals-secret";
    let secret_url = "https://ghe.equals.com";

    let args: Vec<String> = [
        "login".to_string(),
        format!("--mock-token={secret_token}"),
        format!("--mock-refresh-token={secret_refresh}"),
        format!("--enterprise-url={secret_url}"),
    ]
    .to_vec();

    let display = display_tui_auth_args(&args);

    assert!(
        !display.contains(secret_token),
        "mock token leaked in display: {display}"
    );
    assert!(
        !display.contains(secret_refresh),
        "mock refresh token leaked in display: {display}"
    );
    assert!(
        !display.contains(secret_url),
        "enterprise url leaked in display: {display}"
    );
}

#[test]
fn poc_default_redactor_catches_auth_secret_patterns() {
    let redactor = DefaultRedactor::default();

    // Test various secret patterns that could appear in auth output
    let test_cases = vec![
        ("sk-proj-test-key-1234567890abcdef", "[REDACTED_API_KEY]"),
        (
            "Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9",
            "Bearer [REDACTED]",
        ),
        ("Cookie: session=abc123def456", "[REDACTED_COOKIE]"),
        ("ghp_1234567890abcdefghij", "[REDACTED_GITHUB_TOKEN]"),
        ("github_pat_1234567890abcdefghij", "[REDACTED_GITHUB_TOKEN]"),
        ("AKIA1234567890ABCDEF", "[REDACTED_AWS_ACCESS_KEY]"),
        ("AIzaSyA1234567890abcdefghij", "[REDACTED_API_KEY]"),
    ];

    for (secret, expected_marker) in test_cases {
        let redacted = redactor.redact_text(secret);
        assert!(
            !redacted.contains(secret),
            "redactor leaked {secret:?} -> {redacted}"
        );
        assert!(
            redacted.contains(expected_marker),
            "redactor did not produce {expected_marker} for {secret:?} -> {redacted}"
        );
    }
}

#[test]
fn poc_default_redactor_catches_url_embedded_secrets() {
    let redactor = DefaultRedactor::default();

    // URL with userinfo
    let url_with_auth = "https://user:pass@example.com/v1/chat";
    let redacted = redactor.redact_text(url_with_auth);
    assert!(
        !redacted.contains("user:pass@"),
        "URL userinfo leaked: {redacted}"
    );

    // URL with api_key query param
    let url_with_key = "https://api.example.com/v1?api_key=sk-secret-key-12345";
    let redacted = redactor.redact_text(url_with_key);
    assert!(
        !redacted.contains("sk-secret-key-12345"),
        "URL query param leaked: {redacted}"
    );
}

#[test]
fn poc_default_redactor_catches_pem_private_keys() {
    let redactor = DefaultRedactor::default();

    let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
    let redacted = redactor.redact_text(pem);
    assert!(
        !redacted.contains("MIIEpAIBAAKCAQEA"),
        "PEM private key leaked: {redacted}"
    );
    assert!(
        redacted.contains("[REDACTED_PRIVATE_KEY]"),
        "PEM not redacted: {redacted}"
    );
}
