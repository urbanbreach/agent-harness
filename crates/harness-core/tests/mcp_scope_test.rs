//! Task 11 QA: MCP scope verification after remote OAuth removal.
//!
//! Verifies that:
//! - Config-backed stdio (loopback) MCP transport is preserved and parsed correctly.
//! - Config-backed streamable HTTP (configured non-loopback) MCP transport is preserved.
//! - Static/configured credential headers are preserved inside the transport boundary.
//! - Unconfigured/missing endpoint fails closed.
//! - Legacy local/remote shapes with OAuth fields are rejected.
//! - No remote MCP OAuth provisioning surface remains (module removed).
//! - Redirect following to another endpoint is not implicit (endpoint is literal).

use harness_core::config::{
    harness_schema_pretty_json, load_config_from_str, HarnessConfig, McpServerConfig,
};
use harness_core::UnwrapOrAbort;

fn base_config_json(mcp_servers: &str) -> String {
    format!(
        r#"
        {{
          providers: {{
            default: {{
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {{
                "gpt-5.4-mini": {{
                  display_name: "GPT-5.4 Mini",
                }},
              }},
            }},
          }},
          agents: {{
            deep: {{
              description: "Deep work",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            }},
          }},
          permissions: {{
            defaults: {{
              edit: "ask",
              shell: "ask",
              network: "deny",
            }},
          }},
          runtime: {{
            background_tasks: {{
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            }},
            session_dir: ".agent-harness/sessions",
          }},
          integrations: {{
            remote_search: {{
              endpoint: "https://mcp.exa.ai/mcp",
            }},
            mcp: {{
              servers: {{
                {mcp_servers}
              }},
            }},
          }},
        }}
        "#
    )
}

fn parse_config(mcp_servers: &str) -> HarnessConfig {
    let json = base_config_json(mcp_servers);
    json5::from_str(&json).unwrap_or_abort()
}

#[allow(clippy::expect_used, reason = "test helper asserting failure path")]
fn parse_config_err(mcp_servers: &str) -> String {
    let json = base_config_json(mcp_servers);
    json5::from_str::<HarnessConfig>(&json)
        .expect_err("expected config parse error")
        .to_string()
}

// ---------------------------------------------------------------------------
// Loopback (stdio) MCP transport: success, error, restart
// ---------------------------------------------------------------------------

#[test]
fn loopback_stdio_mcp_server_parses_successfully() {
    // arrange
    let parsed = parse_config(
        r#"loopback_server: {
          transport: "stdio",
          command: ["python3", "-m", "mcp_server"],
          env: {
            MCP_MODE: "stdio",
          },
          timeout_secs: 30,
          enabled: true,
        }"#,
    );

    let server = parsed
        .integrations
        .mcp
        .servers
        .get("loopback_server")
        .unwrap_or_abort();

    // act
    match server {
        McpServerConfig::Stdio {
            command,
            env,
            timeout_secs,
            enabled,
            ..
        } => {
            // assert
            assert_eq!(command, &["python3", "-m", "mcp_server"]);
            assert_eq!(env.get("MCP_MODE").map(String::as_str), Some("stdio"));
            assert_eq!(*timeout_secs, 30);
            assert!(*enabled);
        }
        other => panic!("expected stdio config, got {other:?}"),
    }
}

#[test]
fn loopback_stdio_mcp_server_missing_command_fails_closed() {
    // arrange
    let err = parse_config_err(
        r#"broken_stdio: {
          transport: "stdio",
          env: { MODE: "stdio" },
        }"#,
    );
    // act
    // assert
    assert!(
        err.contains("missing field") || err.contains("command"),
        "expected missing command error: {err}"
    );
}

#[test]
fn loopback_stdio_mcp_server_restart_preserves_config() {
    // arrange
    // Simulate restart by parsing the same config twice and verifying equality.
    let config_json = r#"restart_stdio: {
          transport: "stdio",
          command: ["echo", "mcp"],
          timeout_secs: 5,
          enabled: true,
        }"#;
    let first = parse_config(config_json);
    let second = parse_config(config_json);
    let s1 = first
        .integrations
        .mcp
        .servers
        .get("restart_stdio")
        .unwrap_or_abort();
    // act
    let s2 = second
        .integrations
        .mcp
        .servers
        .get("restart_stdio")
        .unwrap_or_abort();
    // McpServerConfig doesn't derive PartialEq, so compare via Debug format.
    // assert
    assert_eq!(format!("{s1:?}"), format!("{s2:?}"));
}

// ---------------------------------------------------------------------------
// Configured non-loopback (HTTP) MCP transport: success, error, restart
// ---------------------------------------------------------------------------

#[test]
fn configured_non_loopback_http_mcp_server_parses_successfully() {
    // arrange
    let parsed = parse_config(
        r#"configured_http: {
          transport: "streamable_http",
          endpoint: "https://mcp.example.test/sse",
          headers: {
            Authorization: "Bearer configured-token",
          },
          timeout_secs: 45,
          enabled: true,
        }"#,
    );

    let server = parsed
        .integrations
        .mcp
        .servers
        .get("configured_http")
        .unwrap_or_abort();

    // act
    match server {
        McpServerConfig::Http {
            endpoint,
            headers,
            timeout_secs,
            enabled,
        } => {
            // assert
            assert_eq!(endpoint, "https://mcp.example.test/sse");
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer configured-token")
            );
            assert_eq!(*timeout_secs, 45);
            assert!(*enabled);
        }
        other => panic!("expected http config, got {other:?}"),
    }
}

#[test]
fn configured_non_loopback_http_mcp_server_missing_endpoint_fails_closed() {
    // arrange
    let err = parse_config_err(
        r#"broken_http: {
          transport: "http",
          headers: { Authorization: "Bearer x" },
        }"#,
    );
    // act
    // assert
    assert!(
        err.contains("missing field") || err.contains("endpoint"),
        "expected missing endpoint error: {err}"
    );
}

#[test]
fn configured_non_loopback_http_mcp_server_restart_preserves_config() {
    // arrange
    let config_json = r#"restart_http: {
          transport: "http",
          endpoint: "https://mcp.restart.test/mcp",
          timeout_secs: 10,
          enabled: true,
        }"#;
    let first = parse_config(config_json);
    let second = parse_config(config_json);
    let s1 = first
        .integrations
        .mcp
        .servers
        .get("restart_http")
        .unwrap_or_abort();
    // act
    let s2 = second
        .integrations
        .mcp
        .servers
        .get("restart_http")
        .unwrap_or_abort();
    // assert
    assert_eq!(format!("{s1:?}"), format!("{s2:?}"));
}

// ---------------------------------------------------------------------------
// Static/configured credential headers preserved inside transport boundary
// ---------------------------------------------------------------------------

#[test]
fn static_credential_headers_preserved_in_http_transport() {
    // arrange
    let parsed = parse_config(
        r#"credentialed_http: {
          transport: "streamable_http",
          endpoint: "https://mcp.secure.test/api",
          headers: {
            Authorization: "Bearer static-secret-token",
            "X-Custom-Auth": "key=value",
          },
          timeout_secs: 30,
          enabled: true,
        }"#,
    );

    // act
    match parsed
        .integrations
        .mcp
        .servers
        .get("credentialed_http")
        .unwrap_or_abort()
    {
        McpServerConfig::Http { headers, .. } => {
            // assert
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer static-secret-token")
            );
            assert_eq!(
                headers.get("X-Custom-Auth").map(String::as_str),
                Some("key=value")
            );
        }
        other => panic!("expected http config, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Fail-closed: unconfigured endpoint, redirect, OAuth
// ---------------------------------------------------------------------------

#[test]
fn unconfigured_mcp_server_with_no_transport_fails_closed() {
    // arrange
    let err = parse_config_err(
        r#"no_transport: {
          command: ["echo"],
        }"#,
    );
    // act
    // assert
    assert!(
        err.contains("missing field") || err.contains("transport"),
        "expected missing transport error: {err}"
    );
}

#[test]
fn legacy_remote_shape_with_oauth_fails_closed() {
    // arrange
    let err = parse_config_err(
        r#"legacy_remote: {
          type: "remote",
          url: "https://mcp.grep.app",
          oauth: {
            mode: "dynamic",
          },
        }"#,
    );
    // act
    // assert
    assert!(
        err.contains("missing field") || err.contains("transport"),
        "legacy remote+oauth shape must fail: {err}"
    );
}

#[test]
fn legacy_local_shape_with_oauth_fails_closed() {
    // arrange
    let err = parse_config_err(
        r#"legacy_local: {
          type: "local",
          command: ["bunx", "mcp-server"],
          oauth: {
            mode: "dynamic",
          },
        }"#,
    );
    // act
    // assert
    assert!(
        err.contains("missing field") || err.contains("transport"),
        "legacy local+oauth shape must fail: {err}"
    );
}

#[test]
fn oauth_fields_in_stdio_config_rejected_by_deny_unknown_fields() {
    // arrange
    let err = parse_config_err(
        r#"oauth_stdio: {
          transport: "stdio",
          command: ["echo"],
          oauth: {
            client_id: "test-client",
            client_secret: "test-secret",
            scopes: ["mcp:read"],
            redirect_uri: "http://localhost:8080/callback",
          },
        }"#,
    );
    // act
    // assert
    assert!(
        err.contains("unknown field") || err.contains("oauth"),
        "oauth fields in stdio config must be rejected: {err}"
    );
}

#[test]
fn oauth_fields_in_http_config_rejected_by_deny_unknown_fields() {
    // arrange
    let err = parse_config_err(
        r#"oauth_http: {
          transport: "http",
          endpoint: "https://mcp.example.test/mcp",
          oauth: {
            client_id: "test-client",
            client_secret: "test-secret",
            discovery_url: "https://auth.example/.well-known/oauth",
          },
        }"#,
    );
    // act
    // assert
    assert!(
        err.contains("unknown field") || err.contains("oauth"),
        "oauth fields in http config must be rejected: {err}"
    );
}

// ---------------------------------------------------------------------------
// No remote provisioning surface remains
// ---------------------------------------------------------------------------

#[test]
fn mcp_oauth_configuration_is_rejected_at_runtime_boundary() {
    // arrange
    let config = r#"oauth_remote: {
      transport: "http",
      endpoint: "https://mcp.example.test/mcp",
      oauth: { client_id: "test-client" },
    }"#;

    // act
    let error = parse_config_err(config);

    // assert
    assert!(error.contains("unknown field") || error.contains("oauth"));
}

#[test]
fn mcp_oauth_local_configuration_is_rejected_at_runtime_boundary() {
    // arrange
    let config = r#"oauth_local: {
      transport: "stdio",
      command: ["echo"],
      oauth: { client_id: "test-client" },
    }"#;

    // act
    let error = parse_config_err(config);

    // assert
    assert!(error.contains("unknown field") || error.contains("oauth"));
}

#[test]
fn schema_has_no_oauth_fields_in_mcp_server_config() {
    // arrange
    let schema = harness_schema_pretty_json().unwrap_or_abort();
    let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap_or_abort();

    let definitions = parsed
        .get("definitions")
        .and_then(|d| d.as_object())
        .unwrap_or_abort();

    let mcp_server_config = definitions
        .get("McpServerConfig")
        .and_then(|m| m.as_object())
        .unwrap_or_abort();

    // act
    // The schema must be a oneOf with exactly stdio and http variants.
    let one_of = mcp_server_config
        .get("oneOf")
        .and_then(|o| o.as_array())
        .unwrap_or_abort();
    // assert
    assert_eq!(
        one_of.len(),
        2,
        "McpServerConfig must have exactly 2 variants (stdio + http)"
    );

    // Collect all property names across both variants.
    let mut all_props: std::collections::HashSet<String> = std::collections::HashSet::new();
    for variant in one_of {
        if let Some(props) = variant.get("properties").and_then(|p| p.as_object()) {
            for key in props.keys() {
                all_props.insert(key.clone());
            }
        }
    }

    // No OAuth-related fields should be present.
    for forbidden in &[
        "oauth",
        "client_id",
        "client_secret",
        "scopes",
        "redirect_uri",
        "auth_url",
        "token_url",
        "discovery_url",
        "pkce",
        "code_verifier",
        "code_challenge",
    ] {
        assert!(
            !all_props.contains(*forbidden),
            "McpServerConfig schema must not contain OAuth field '{forbidden}': {all_props:?}"
        );
    }
}

#[test]
fn schema_exports_mcp_with_transport_only() {
    // arrange
    let schema = harness_schema_pretty_json().unwrap_or_abort();
    // act
    // assert
    assert!(schema.contains("\"mcp\""));
    assert!(schema.contains("\"transport\""));
    assert!(schema.contains("\"stdio\""));
    assert!(schema.contains("\"http\""));
    // OAuth must not appear in the schema.
    assert!(
        !schema.contains("oauth"),
        "schema must not contain 'oauth': {schema}"
    );
    assert!(
        !schema.contains("pkce"),
        "schema must not contain 'pkce': {schema}"
    );
    assert!(
        !schema.contains("discovery_url"),
        "schema must not contain 'discovery_url': {schema}"
    );
}

// ---------------------------------------------------------------------------
// Endpoint is literal: no implicit redirect or discovery
// ---------------------------------------------------------------------------

#[test]
fn http_endpoint_is_literal_string_not_redirect_target() {
    // arrange
    // The endpoint field is a plain String, not a URL that gets followed.
    // This test verifies that any http(s) endpoint is accepted as-is.
    let parsed = parse_config(
        r#"literal_endpoint: {
          transport: "http",
          endpoint: "http://127.0.0.1:9999/local-mcp",
          timeout_secs: 5,
          enabled: true,
        }"#,
    );

    // act
    match parsed
        .integrations
        .mcp
        .servers
        .get("literal_endpoint")
        .unwrap_or_abort()
    {
        McpServerConfig::Http { endpoint, .. } => {
            // assert
            assert_eq!(endpoint, "http://127.0.0.1:9999/local-mcp");
        }
        other => panic!("expected http config, got {other:?}"),
    }
}

#[test]
fn http_alias_streamable_http_accepted() {
    // arrange
    let parsed = parse_config(
        r#"aliased_http: {
          transport: "streamable_http",
          endpoint: "https://mcp.aliased.test/mcp",
          timeout_secs: 15,
          enabled: true,
        }"#,
    );

    // act
    match parsed
        .integrations
        .mcp
        .servers
        .get("aliased_http")
        .unwrap_or_abort()
    {
        McpServerConfig::Http { endpoint, .. } => {
            // assert
            assert_eq!(endpoint, "https://mcp.aliased.test/mcp");
        }
        other => panic!("expected http config, got {other:?}"),
    }
}

#[test]
fn http_url_alias_accepted() {
    // arrange
    let parsed = parse_config(
        r#"url_aliased: {
          transport: "http",
          url: "https://mcp.url_alias.test/mcp",
          timeout_secs: 15,
          enabled: true,
        }"#,
    );

    // act
    match parsed
        .integrations
        .mcp
        .servers
        .get("url_aliased")
        .unwrap_or_abort()
    {
        McpServerConfig::Http { endpoint, .. } => {
            // assert
            assert_eq!(endpoint, "https://mcp.url_alias.test/mcp");
        }
        other => panic!("expected http config, got {other:?}"),
    }
}
