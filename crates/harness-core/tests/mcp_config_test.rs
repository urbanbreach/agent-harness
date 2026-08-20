use harness_core::UnwrapOrAbort;
use std::path::PathBuf;

use harness_core::config::{
    harness_schema_pretty_json, load_config_from_str, HarnessConfig, McpServerConfig,
};

fn config_with_mcp_servers_json(servers: &str) -> String {
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
{servers}
              }},
            }},
          }},
        }}
        "#
    )
}

fn config_with_mcp_servers(servers: &str) -> HarnessConfig {
    json5::from_str(&config_with_mcp_servers_json(servers)).unwrap_or_abort()
}

#[test]
fn integrations_mcp_accepts_stdio_and_http_server_shapes() {
    // arrange
    // act
    // assert
    let parsed = config_with_mcp_servers(
        r#"                fixture_stdio: {
                  transport: "stdio",
                  command: ["python3", "fixtures/mcp_stdio_server.py"],
                  env: {
                    MCP_FIXTURE_MODE: "stdio",
                  },
                  cwd: "fixtures",
                  timeout_secs: 12,
                  enabled: true,
                },
                fixture_http: {
                  transport: "streamable_http",
                  endpoint: "https://example.test/mcp",
                  headers: {
                    Authorization: "Bearer demo-token",
                  },
                  timeout_secs: 45,
                  enabled: true,
                },"#,
    );

    let stdio = parsed
        .integrations
        .mcp
        .servers
        .get("fixture_stdio")
        .unwrap_or_abort();
    match stdio {
        McpServerConfig::Stdio {
            command,
            env,
            cwd,
            timeout_secs,
            enabled,
        } => {
            assert_eq!(command, &["python3", "fixtures/mcp_stdio_server.py"]);
            assert_eq!(
                env.get("MCP_FIXTURE_MODE").map(String::as_str),
                Some("stdio")
            );
            assert_eq!(cwd.as_ref(), Some(&PathBuf::from("fixtures")));
            assert_eq!(*timeout_secs, 12);
            assert!(*enabled);
        }
        other => panic!("expected stdio config, got {other:?}"),
    }

    let http = parsed
        .integrations
        .mcp
        .servers
        .get("fixture_http")
        .unwrap_or_abort();
    match http {
        McpServerConfig::Http {
            endpoint,
            headers,
            timeout_secs,
            enabled,
        } => {
            assert_eq!(endpoint, "https://example.test/mcp");
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer demo-token")
            );
            assert_eq!(*timeout_secs, 45);
            assert!(*enabled);
        }
        other => panic!("expected http config, got {other:?}"),
    }
}

#[test]
fn integrations_mcp_rejects_legacy_local_and_remote_server_shapes() {
    // arrange
    // act
    // assert
    let error = json5::from_str::<HarnessConfig>(&config_with_mcp_servers_json(
        r#"                docs_rs: {
                  type: "local",
                  command: ["bunx", "-y", "@nuskey8/docs-rs-mcp@latest"],
                  environment: {
                    RUST_LOG: "warn",
                  },
                  timeout: 19,
                  enabled: true,
                },
                gh_grep: {
                  type: "remote",
                  url: "https://mcp.grep.app",
                  headers: {
                    Authorization: "Bearer token",
                  },
                  oauth: {
                    mode: "dynamic",
                  },
                  timeout: 33,
                  enabled: false,
                },"#,
    ))
    .expect_err("legacy-shaped MCP config should fail");

    assert!(error.to_string().contains("missing field `transport`"));
}

#[test]
fn config_schema_exports_top_level_mcp_servers() {
    // arrange
    // act
    // assert
    let schema = harness_schema_pretty_json().unwrap_or_abort();
    let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap_or_abort();

    assert!(schema.contains("\"mcp\""));
    assert!(schema.contains("\"transport\""));
    assert!(schema.contains("\"http\""));
    assert!(!schema.contains("\"integrations\""));
    // Top-level properties must not include "servers" (MCP servers live under "mcp")
    let top_level_props = parsed
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|m| m.keys().cloned().collect::<std::collections::HashSet<_>>())
        .unwrap_or_default();
    assert!(
        !top_level_props.contains("servers"),
        "top-level schema must not export a 'servers' property"
    );
}

#[test]
fn integrations_mcp_rejects_stdio_server_missing_command() {
    // arrange — a stdio server entry without the required command field
    let config = config_with_mcp_servers_json(
        r#"                broken_stdio: {
                  transport: "stdio",
                  env: { MODE: "stdio" },
                },"#,
    );

    // act
    let result = load_config_from_str(&config);

    // assert — shape validation fails closed: no partial stdio server is accepted
    assert!(
        result.is_err(),
        "stdio server without command must fail closed"
    );
}

#[test]
fn integrations_mcp_rejects_invalid_server_ids() {
    // arrange
    // act
    // assert
    let err = load_config_from_str(
        r#"
        {
          provider: {
            test: {
              type: "anthropic_messages",
              apiKey: "test-key",
              models: {
                model: { name: "Test Model" },
              },
            },
          },
          model: "test/model",
          mcp: {
            "bad.name": {
              transport: "http",
              endpoint: "https://example.test/mcp",
            },
          },
        }
        "#,
    )
    .expect_err("invalid MCP server ids should fail validation");

    assert!(
        err.to_string().contains("invalid server id"),
        "unexpected validation error: {err}"
    );
}
