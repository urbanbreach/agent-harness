use harness_core::auth::codex::{
    AuthHttpMethod, AuthHttpRequest, AuthHttpResponse, CodexLoopbackSession, CodexOAuthClient,
    CodexOAuthError, PkceCodes,
};
use harness_core::auth::{AuthProviderId, CredentialStore, StoredCredential, StoredCredentialKind};
use harness_core::config::load_config_from_str;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    complete_codex_pasted_callback, handle_codex_loopback_stream, onboarding_required_for_config,
};

fn codex_config(provider_fields: &str) -> harness_core::config::HarnessConfig {
    load_config_from_str(&format!(
        r#"
        {{
          provider: {{
            codex_route: {{
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              authProvider: "codex",
              {provider_fields}
              models: {{
                "gpt-5.4-mini": {{ name: "GPT-5.4 mini" }},
              }},
            }},
          }},
          model: "codex_route/gpt-5.4-mini",
          permission: "ask",
        }}
        "#
    ))
    .expect("load auth config")
}

fn auth_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn auth_deps(data_home: &Path) -> crate::CliDeps {
    crate::CliDeps::real().with_env(
        "HARNESS_DATA_HOME",
        data_home.to_string_lossy().into_owned(),
    )
}

fn load_stored(data_home: &Path, provider: AuthProviderId) -> StoredCredential {
    CredentialStore::new(data_home.join("harness"))
        .load(provider)
        .expect("load stored credential")
        .expect("credential stored")
}

#[test]
fn onboarding_required_only_when_configured_auth_provider_has_no_usable_fallback() {
    let missing = codex_config("");
    assert!(onboarding_required_for_config(
        Some(&missing),
        &|_| false,
        None
    ));

    let env_config = codex_config(r#"apiKeyEnv: ["HARNESS_ONBOARDING_KEY"],"#);
    assert!(!onboarding_required_for_config(
        Some(&env_config),
        &|name| name == "HARNESS_ONBOARDING_KEY",
        None
    ));

    let inline_config = codex_config(r#"apiKey: "INLINE_TEST_KEY","#);
    assert!(!onboarding_required_for_config(
        Some(&inline_config),
        &|_| false,
        None
    ));

    let temp = tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    store
        .save(&StoredCredential::oauth(
            AuthProviderId::Codex,
            "stored-access-secret",
            "stored-refresh-secret",
            Some("2099-01-02T03:04:05Z".to_string()),
            "2026-05-30T00:00:00Z",
        ))
        .expect("save stored credential");
    assert!(!onboarding_required_for_config(
        Some(&missing),
        &|_| false,
        Some(&store)
    ));
}

#[test]
fn interactive_auth_login_provider_picker_cancels_without_stacktrace() {
    let temp = tempdir().expect("tempdir");
    let args = auth_args(&["login"]);

    let output = super::execute_backend_args(&args, None, None, "\x1b", &auth_deps(temp.path()));

    assert_eq!(output.code, 1);
    assert!(output.stdout.contains("Add credential"));
    assert!(output.stdout.contains("Select provider"));
    assert!(output.stdout.contains("OpenAI"));
    assert!(output.stdout.contains("GitHub Copilot"));
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);
    assert!(
        !temp.path().join("harness/credentials/codex.json").exists(),
        "cancelled picker must not store credentials"
    );
}

#[test]
fn interactive_codex_api_key_stores_without_echoing_secret() {
    let temp = tempdir().expect("tempdir");
    let secret = "sk-interactive-auth-secret-value";
    let args = auth_args(&["login"]);
    let stdin = format!("\n\x1b[B\x1b[B\n{secret}\n");

    let output = super::execute_backend_args(&args, None, None, &stdin, &auth_deps(temp.path()));

    assert_eq!(
        output.code, 0,
        "stdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    assert!(output.stdout.contains("Manually enter API Key"));
    assert!(output.stdout.contains("Done"));
    assert!(!output.stdout.contains(secret));
    assert!(!output.stderr.contains(secret));
    let stored = load_stored(temp.path(), AuthProviderId::Codex);
    assert_eq!(stored.kind, StoredCredentialKind::ApiKey);
    assert_eq!(stored.api_key.as_deref(), Some(secret));
}

#[test]
fn interactive_codex_browser_and_device_resolve_to_mockable_oauth_paths() {
    for (stdin, expected_label) in [
        ("\n\n", "ChatGPT Pro/Plus (browser)"),
        ("\n\x1b[B\n", "ChatGPT Pro/Plus (headless)"),
    ] {
        let temp = tempdir().expect("tempdir");
        let token = format!("oauth-{expected_label}-secret");
        let args = auth_args(&[
            "login",
            "--mock-token",
            &token,
            "--mock-refresh-token",
            "refresh-secret",
        ]);

        let output = super::execute_backend_args(&args, None, None, stdin, &auth_deps(temp.path()));

        assert_eq!(
            output.code, 0,
            "label: {expected_label}\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(output.stdout.contains(expected_label));
        assert!(output.stdout.contains("Login successful"));
        assert!(!output.stdout.contains(&token));
        assert!(!output.stderr.contains(&token));
        let stored = load_stored(temp.path(), AuthProviderId::Codex);
        assert_eq!(stored.kind, StoredCredentialKind::Oauth);
        assert_eq!(stored.access_token.as_deref(), Some(token.as_str()));
    }
}

#[test]
fn interactive_github_copilot_resolves_to_mockable_device_flow() {
    let temp = tempdir().expect("tempdir");
    let token = "copilot-interactive-secret";
    let args = auth_args(&["login", "--mock-token", token]);

    let output =
        super::execute_backend_args(&args, None, None, "\x1b[B\n\n", &auth_deps(temp.path()));

    assert_eq!(
        output.code, 0,
        "stdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    assert!(output.stdout.contains("GitHub Copilot"));
    assert!(output.stdout.contains("Select GitHub deployment type"));
    assert!(output.stdout.contains("Login successful"));
    assert!(!output.stdout.contains(token));
    assert!(!output.stderr.contains(token));
    let stored = load_stored(temp.path(), AuthProviderId::GithubCopilot);
    assert_eq!(stored.kind, StoredCredentialKind::Oauth);
    assert_eq!(stored.access_token.as_deref(), Some(token));
}

#[test]
fn explicit_auth_login_args_bypass_interactive_picker() {
    let temp = tempdir().expect("tempdir");
    let secret = "sk-explicit-auth-secret-value";
    let args = auth_args(&[
        "login",
        "OpenAI",
        "--method",
        "Manually enter API Key",
        "--api-key-stdin",
    ]);

    let output = super::execute_backend_args(
        &args,
        None,
        None,
        &format!("{secret}\n"),
        &auth_deps(temp.path()),
    );

    assert_eq!(
        output.code, 0,
        "stdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    assert!(!output.stdout.contains("Select provider"));
    assert!(!output.stdout.contains(secret));
    assert!(!output.stderr.contains(secret));
    let stored = load_stored(temp.path(), AuthProviderId::Codex);
    assert_eq!(stored.kind, StoredCredentialKind::ApiKey);
    assert_eq!(stored.api_key.as_deref(), Some(secret));
}

#[test]
fn supported_method_labels_parse_for_supported_providers() {
    assert_eq!(
        super::parse_login_method_arg("ChatGPT Pro/Plus (browser)"),
        Ok(super::AuthLoginMethod::Browser)
    );
    assert_eq!(
        super::parse_login_method_arg("ChatGPT Pro/Plus (headless)"),
        Ok(super::AuthLoginMethod::Device)
    );
    assert_eq!(
        super::parse_login_method_arg("Manually enter API Key"),
        Ok(super::AuthLoginMethod::ApiKey)
    );
    assert_eq!(
        super::parse_login_method_arg("Login with GitHub Copilot"),
        Ok(super::AuthLoginMethod::Device)
    );
}

#[derive(Debug)]
struct MockCodexAuthHttpClient {
    responses: Mutex<VecDeque<AuthHttpResponse>>,
    requests: Mutex<Vec<AuthHttpRequest>>,
}

#[async_trait::async_trait]
impl super::CodexAuthHttpClient for MockCodexAuthHttpClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        self.requests.lock().expect("requests").push(request);
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .ok_or_else(|| CodexOAuthError::Http {
                message: "no mocked auth response".to_string(),
            })
    }
}

impl MockCodexAuthHttpClient {
    fn new(response: AuthHttpResponse) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(VecDeque::from([response])),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<AuthHttpRequest> {
        self.requests.lock().expect("requests").clone()
    }
}

#[tokio::test]
async fn codex_browser_login_accepts_pasted_localhost_callback_url() {
    let http = MockCodexAuthHttpClient::new(AuthHttpResponse {
        status: 200,
        body: serde_json::json!({
            "access_token": "pasted-access-secret",
            "refresh_token": "pasted-refresh-secret",
            "expires_in": 3600
        })
        .to_string(),
    });
    let client = CodexOAuthClient::new(http.clone()).with_issuer("https://issuer.test");
    let session = CodexLoopbackSession::with_redirect_uri(
        PkceCodes {
            verifier: "pasted-verifier-123".to_string(),
            challenge: "pasted-challenge-123".to_string(),
        },
        "state-123",
        "http://localhost:1455/auth/callback",
        "https://issuer.test",
    );
    let temp = tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());

    let credential = complete_codex_pasted_callback(
        &client,
        &session,
        &store,
        "http://localhost:1455/auth/callback?code=pasted-code-123&state=state-123",
    )
    .await
    .expect("complete pasted callback");

    assert_eq!(
        credential.access_token.as_deref(),
        Some("pasted-access-secret")
    );
    let stored = store
        .load(AuthProviderId::Codex)
        .expect("load credential")
        .expect("stored credential");
    assert_eq!(stored.access_token.as_deref(), Some("pasted-access-secret"));
    assert_eq!(
        stored.refresh_token.as_deref(),
        Some("pasted-refresh-secret")
    );
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, AuthHttpMethod::Post);
    assert_eq!(requests[0].url, "https://issuer.test/oauth/token");
    assert!(requests[0].body.contains("grant_type=authorization_code"));
    assert!(requests[0].body.contains("code=pasted-code-123"));
    assert!(requests[0]
        .body
        .contains("code_verifier=pasted-verifier-123"));
}

#[tokio::test]
async fn codex_browser_login_loopback_uses_cli_listener_and_stores_credential() {
    let http = MockCodexAuthHttpClient::new(AuthHttpResponse {
        status: 200,
        body: serde_json::json!({
            "access_token": "browser-access-secret",
            "refresh_token": "browser-refresh-secret",
            "expires_in": 3600
        })
        .to_string(),
    });
    let client = CodexOAuthClient::new(http.clone()).with_issuer("https://issuer.test");
    let session = CodexLoopbackSession::with_redirect_uri(
        PkceCodes {
            verifier: "browser-verifier-123".to_string(),
            challenge: "browser-challenge-123".to_string(),
        },
        "state-123",
        "http://localhost:14567/auth/callback",
        "https://issuer.test",
    );
    let temp = tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let (server_stream, mut browser_stream) = tokio::io::duplex(16 * 1024);
    let handler = tokio::spawn({
        let store = store.clone();
        async move { handle_codex_loopback_stream(server_stream, &client, &session, &store).await }
    });
    browser_stream
        .write_all(
            b"GET /auth/callback?code=browser-code-123&state=state-123 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .await
        .expect("write callback");
    browser_stream
        .shutdown()
        .await
        .expect("finish callback request");
    let mut response = String::new();
    browser_stream
        .read_to_string(&mut response)
        .await
        .expect("read callback response");

    assert!(response.contains("Authorization Successful"));
    handler
        .await
        .expect("loopback handler task")
        .expect("loopback handler should store credential");
    let stored = store
        .load(AuthProviderId::Codex)
        .expect("load credential")
        .expect("stored credential");
    assert_eq!(
        stored.access_token.as_deref(),
        Some("browser-access-secret")
    );
    assert_eq!(
        stored.refresh_token.as_deref(),
        Some("browser-refresh-secret")
    );
    assert!(
        stored.expires_at.is_some(),
        "CLI Codex OAuth storage must preserve token expiry"
    );
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, AuthHttpMethod::Post);
    assert_eq!(requests[0].url, "https://issuer.test/oauth/token");
    assert!(requests[0].body.contains("grant_type=authorization_code"));
    assert!(requests[0].body.contains("code=browser-code-123"));
    assert!(requests[0]
        .body
        .contains("code_verifier=browser-verifier-123"));
    assert!(!response.contains("browser-access-secret"));
}
