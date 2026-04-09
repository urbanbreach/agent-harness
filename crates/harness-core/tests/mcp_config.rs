use std::path::PathBuf;

use harness_core::config::{
    harness_schema_pretty_json, load_config_from_str, HarnessConfig, McpServerConfig,
};

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
          agents: {
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
        .expect("http server config");
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
fn integrations_mcp_accepts_opencode_local_and_remote_server_shapes() {
    let parsed: HarnessConfig = json5::from_str(
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
          agents: {
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
                docs_rs: {
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
                },
              },
            },
          },
        }
        "#,
    )
    .expect("opencode-shaped MCP config should deserialize");

    match parsed
        .integrations
        .mcp
        .servers
        .get("docs_rs")
        .expect("docs-rs server")
    {
        McpServerConfig::Stdio {
            command,
            env,
            timeout_secs,
            enabled,
            ..
        } => {
            assert_eq!(command, &["bunx", "-y", "@nuskey8/docs-rs-mcp@latest"]);
            assert_eq!(env.get("RUST_LOG").map(String::as_str), Some("warn"));
            assert_eq!(*timeout_secs, 19);
            assert!(*enabled);
        }
        other => panic!("expected local server to normalize to stdio, got {other:?}"),
    }

    match parsed
        .integrations
        .mcp
        .servers
        .get("gh_grep")
        .expect("gh_grep server")
    {
        McpServerConfig::Http {
            endpoint,
            headers,
            timeout_secs,
            enabled,
        } => {
            assert_eq!(endpoint, "https://mcp.grep.app");
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer token")
            );
            assert_eq!(*timeout_secs, 33);
            assert!(!enabled);
        }
        other => panic!("expected remote server to normalize to http, got {other:?}"),
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

#[test]
fn integrations_mcp_rejects_invalid_server_ids() {
    let err = load_config_from_str(
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
          agents: {
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
                "bad.name": {
                  type: "remote",
                  url: "https://example.test/mcp",
                },
              },
            },
          },
        }
        "#,
    )
    .expect_err("invalid MCP server ids should fail validation");

    assert!(err.to_string().contains("invalid server id"));
}
