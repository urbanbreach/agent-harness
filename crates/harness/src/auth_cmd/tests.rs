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

use super::support::onboarding_required_for_config;
use super::{complete_codex_pasted_callback, handle_codex_loopback_stream};

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

fn assert_no_terminal_control_bytes(output: &str) {
    assert!(!output.contains('\u{1b}'), "output: {output:?}");
    assert!(!output.contains('\u{7}'), "output: {output:?}");
    assert!(!output.contains('\u{7f}'), "output: {output:?}");
}

fn assert_explicit_auth_login_rejects_provider(provider: &str) {
    let temp = tempdir().expect("tempdir");
    let args = auth_args(&["login", provider, "--method", "api-key", "--api-key-stdin"]);

    let output =
        super::execute_backend_args(&args, None, None, "sk-test\n", &auth_deps(temp.path()));

    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("invalid auth provider"));
    assert_no_terminal_control_bytes(&output.stderr);
    assert_no_terminal_control_bytes(&output.stdout);
}

fn auth_deps(data_home: &Path) -> crate::CliDeps {
    crate::CliDeps::real().with_env(
        "HARNESS_DATA_HOME",
        data_home.to_string_lossy().into_owned(),
    )
}

fn load_stored(data_home: &Path, provider: AuthProviderId) -> StoredCredential {
    CredentialStore::new(data_home.join("harness"))
        .load(&provider)
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
            AuthProviderId::codex(),
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
    let stored = load_stored(temp.path(), AuthProviderId::codex());
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
        let stored = load_stored(temp.path(), AuthProviderId::codex());
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
    let stored = load_stored(temp.path(), AuthProviderId::github_copilot());
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
    let stored = load_stored(temp.path(), AuthProviderId::codex());
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

#[test]
fn explicit_auth_logout_rejects_control_provider_without_echoing_it() {
    let temp = tempdir().expect("tempdir");
    let provider = "codex\u{1b}]52;c;SGFja2Vk\u{7}";
    let args = auth_args(&["logout", provider]);

    let output = super::execute_backend_args(&args, None, None, "", &auth_deps(temp.path()));

    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("invalid auth provider"));
    assert_no_terminal_control_bytes(&output.stderr);
    assert_no_terminal_control_bytes(&output.stdout);
}

#[test]
fn explicit_auth_login_rejects_control_provider_without_echoing_it() {
    let provider = "codex\u{1b}]52;c;SGFja2Vk\u{7}";

    assert_explicit_auth_login_rejects_provider(provider);
}

#[test]
fn explicit_auth_login_rejects_leading_newline_codex_alias_without_echoing_it() {
    let provider = "\ncodex";

    assert_explicit_auth_login_rejects_provider(provider);
}

#[test]
fn explicit_auth_login_rejects_trailing_newline_codex_alias_without_echoing_it() {
    let provider = "codex\n";

    assert_explicit_auth_login_rejects_provider(provider);
}

#[test]
fn explicit_auth_login_rejects_leading_newline_openai_alias_without_echoing_it() {
    let provider = "\nopenai";

    assert_explicit_auth_login_rejects_provider(provider);
}

#[test]
fn explicit_auth_login_rejects_leading_newline_copilot_alias_without_echoing_it() {
    let provider = "\ngithub-copilot";

    assert_explicit_auth_login_rejects_provider(provider);
}

#[test]
fn auth_list_sanitizes_control_provider_key_config_error_without_echoing_it() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    std::fs::write(
        &config_path,
        r#"
        {
          provider: {
            "evil\u001b]52;c;SGFja2Vk\u0007": {
              type: "openai_compatible",
              options: {
                authProvider: "codex",
                baseURL: "http://127.0.0.1:8317/v1",
                apiKeyEnv: ["HARNESS_TEST_API_KEY"],
              },
              models: {
                "m": { name: "M" },
              },
            },
          },
          model: "evil\u001b]52;c;SGFja2Vk\u0007/m",
          agent: {
            build: { system_prompt: "Build work" },
          },
          default_agent: "build",
          permission: "ask",
        }
        "#,
    )
    .expect("write malicious provider-key config");
    let args = auth_args(&["list"]);

    let output =
        super::execute_backend_args(&args, Some(config_path), None, "", &auth_deps(temp.path()));

    assert_eq!(output.code, 0);
    assert!(output.stderr.contains("invalid provider id"));
    assert_no_terminal_control_bytes(&output.stderr);
    assert_no_terminal_control_bytes(&output.stdout);
}

#[test]
fn auth_list_reports_arbitrary_configured_auth_provider() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    std::fs::write(
        &config_path,
        r#"
        {
          provider: {
            anthropic_route: {
              type: "openai_compatible",
              options: {
                authProvider: "anthropic",
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "claude-test": { name: "Claude Test" },
              },
            },
          },
          model: "anthropic_route/claude-test",
          agent: {
            build: { system_prompt: "Build work" },
          },
          default_agent: "build",
          permission: "ask",
        }
        "#,
    )
    .expect("write arbitrary auth-provider config");
    let args = auth_args(&["list", "--json"]);

    let output =
        super::execute_backend_args(&args, Some(config_path), None, "", &auth_deps(temp.path()));
    let statuses: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("auth list JSON output");
    let anthropic = statuses
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["auth_provider"] == serde_json::json!("anthropic"))
        })
        .expect("anthropic auth provider status");

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);
    assert_eq!(
        anthropic["provider_ids"],
        serde_json::json!(["anthropic_route"])
    );
    assert_eq!(anthropic["source"], serde_json::json!("inline_apiKey"));
    assert_eq!(anthropic["presence"], serde_json::json!("inline"));
    assert_no_terminal_control_bytes(&output.stderr);
    assert_no_terminal_control_bytes(&output.stdout);
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
        .load(&AuthProviderId::codex())
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
        .load(&AuthProviderId::codex())
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

#[test]
fn config_with_arbitrary_auth_provider_parses() {
    // arrange
    let config_str = r#"
        {
          provider: {
            anthropic_route: {
              type: "openai_compatible",
              options: {
                authProvider: "anthropic",
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "claude-test": { name: "Claude Test" },
              },
            },
          },
          model: "anthropic_route/claude-test",
          permission: "ask",
        }
        "#;

    // act
    let config =
        load_config_from_str(config_str).expect("config with arbitrary authProvider should parse");
    let provider = config.providers.get("anthropic_route").expect("provider");

    // assert
    match provider {
        harness_core::config::ProviderConfig::OpenAiCompatible(opts) => {
            assert_eq!(
                opts.auth_provider.as_ref().map(|p| p.as_str()),
                Some("anthropic")
            );
        }
    }
}

#[test]
fn config_with_empty_auth_provider_fails() {
    // arrange
    let config_str = r#"
        {
          provider: {
            bad_route: {
              type: "openai_compatible",
              options: {
                authProvider: "",
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "test": { name: "Test" },
              },
            },
          },
          model: "bad_route/test",
          permission: "ask",
        }
        "#;

    // act
    let result = load_config_from_str(config_str);

    // assert
    assert!(
        result.is_err(),
        "config with empty authProvider must fail validation"
    );
}

#[test]
fn config_with_codex_auth_provider_backward_compat() {
    // arrange
    let config_str = r#"
        {
          provider: {
            codex_route: {
              type: "openai_compatible",
              options: {
                authProvider: "codex",
                baseURL: "http://127.0.0.1:8317/v1",
                apiKeyEnv: ["OPENAI_API_KEY"],
              },
              models: {
                "gpt-test": { name: "GPT Test" },
              },
            },
          },
          model: "codex_route/gpt-test",
          permission: "ask",
        }
        "#;

    // act
    let config = load_config_from_str(config_str)
        .expect("config with codex authProvider should parse for backward compat");
    let provider = config.providers.get("codex_route").expect("provider");

    // assert
    match provider {
        harness_core::config::ProviderConfig::OpenAiCompatible(opts) => {
            assert_eq!(
                opts.auth_provider.as_ref().map(|p| p.as_str()),
                Some("codex")
            );
        }
    }
}

#[test]
fn config_with_null_auth_provider_passes() {
    // arrange
    let config_str = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "test": { name: "Test" },
              },
            },
          },
          model: "default/test",
          permission: "ask",
        }
        "#;

    // act
    let config =
        load_config_from_str(config_str).expect("config without authProvider should parse");
    let provider = config.providers.get("default").expect("provider");

    // assert
    match provider {
        harness_core::config::ProviderConfig::OpenAiCompatible(opts) => {
            assert!(opts.auth_provider.is_none());
        }
    }
}
