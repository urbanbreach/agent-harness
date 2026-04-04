use std::path::PathBuf;

use harness_core::config::{harness_schema_pretty_json, HarnessConfig, McpServerConfig};

fn config_with_mcp_servers() -> HarnessConfig {
    json5::from_str(
        r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {
                "gpt-5.4-mini": {
                  display_name: "GPT-5.4 Mini",
                },
              },
            },
          },
          profiles: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            },
          },
          permissions: {
            defaults: {
              edit: "ask",
              shell: "ask",
              network: "deny",
            },
          },
          runtime: {
            background_tasks: {
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            },
            session_dir: ".agent-harness/sessions",
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp",
            },
            mcp: {
              servers: {
                fixture_stdio: {
                  transport: "stdio",
                  command: ["python3", "fixtures/mcp_stdio_server.py"],
                  env: {
                    MCP_FIXTURE_MODE: "stdio",
                  },
                  cwd: "fixtures",
                  timeout_secs: 12,
                },
                fixture_http: {
                  transport: "streamable_http",
                  endpoint: "https://example.test/mcp",
                  headers: {
                    Authorization: "Bearer demo-token",
                  },
                  timeout_secs: 45,
                },
              },
            },
          },
        }
        "#,
    )
    .expect("config shape should deserialize")
}

#[test]
fn integrations_mcp_accepts_stdio_and_http_server_shapes() {
    let parsed = config_with_mcp_servers();

    let stdio = parsed
        .integrations
        .mcp
        .servers
        .get("fixture_stdio")
        .expect("stdio server config");
    match stdio {
        McpServerConfig::Stdio {
            command,
            env,
            cwd,
            timeout_secs,
        } => {
            assert_eq!(command, &["python3", "fixtures/mcp_stdio_server.py"]);
            assert_eq!(
                env.get("MCP_FIXTURE_MODE").map(String::as_str),
                Some("stdio")
            );
            assert_eq!(cwd.as_ref(), Some(&PathBuf::from("fixtures")));
            assert_eq!(*timeout_secs, 12);
        }
        other => panic!("expected stdio config, got {other:?}"),
    }

    let http = parsed
        .integrations
        .mcp
        .servers
        .get("fixture_http")
        .expect("http server config");
    match http {
        McpServerConfig::Http {
            endpoint,
            headers,
            timeout_secs,
        } => {
            assert_eq!(endpoint, "https://example.test/mcp");
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer demo-token")
            );
            assert_eq!(*timeout_secs, 45);
        }
        other => panic!("expected http config, got {other:?}"),
    }
}

#[test]
fn config_schema_exports_integrations_mcp_servers() {
    let schema = harness_schema_pretty_json().expect("schema generation should succeed");

    assert!(schema.contains("\"mcp\""));
    assert!(schema.contains("\"servers\""));
    assert!(schema.contains("\"transport\""));
    assert!(schema.contains("\"http\""));
}
