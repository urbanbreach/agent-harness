use super::*;
use crate::auth::AuthProviderId;
use std::sync::Mutex;

static CONFIG_DISCOVERY_TEST_LOCK: Mutex<()> = Mutex::new(());
fn discovery_context(cwd: &Path, xdg_config_home: Option<&Path>) -> ConfigLoadContext {
    ConfigLoadContext {
        discovery: ConfigDiscoveryContext {
            current_dir: cwd.to_path_buf(),
            xdg_config_home: xdg_config_home.map(Path::to_path_buf),
            home: Some(cwd.to_path_buf()),
            runtime_config_path: None,
            tui_config_path: None,
        },
        runtime_content: None,
    }
}

fn config_fixture(
    agents: &str,
    api_key: &str,
    ui_section: Option<&str>,
    schema: Option<&str>,
) -> String {
    let ui_section = ui_section.unwrap_or("");
    let schema_section = schema
        .map(|value| format!(r#""$schema": "{value}","#))
        .unwrap_or_default();

    format!(
        r#"
        {{
          {schema_section}
          providers: {{
            default: {{
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "{api_key}",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {{
                "gpt-4o-mini": {{
                  display_name: "GPT-4o mini",
                }},
              }},
            }},
          }},
          agents: {{
            {agents}
          }},
          permissions: {{
            defaults: {{
              edit: "ask",
              shell: "ask",
              network: "deny",
              question: "ask",
              task: "ask",
              webfetch: "deny",
              websearch: "deny",
              codesearch: "deny",
              lsp: "allow",
            }},
            shell_allowlist: {{
              executables: ["git"],
              cwd_roots: ["."],
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
            permissions: {{
              ask_timeout_ms: 45000,
            }},
            prompt: {{
              wait_timeout_ms: 15000,
            }},
            deterministic: {{
              enabled: false,
              seed: 42,
            }},
          }},
          integrations: {{
            remote_search: {{
              endpoint: "https://mcp.exa.ai/mcp",
            }},
          }},
          {ui_section}
        }}
        "#,
        schema_section = schema_section,
        api_key = api_key,
        agents = agents,
        ui_section = ui_section,
    )
}

fn deep_profile(extra_fields: &str) -> String {
    format!(
        r#"
            deep: {{
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              {extra_fields}
            }},
            "#,
        extra_fields = extra_fields,
    )
}

fn write_agent_markdown_in(repo_root: &Path, prompt_root: &str, name: &str, content: &str) {
    let path = repo_root
        .join(prompt_root)
        .join("agents")
        .join(format!("{name}.md"));
    fs::create_dir_all(path.parent().expect("agent markdown parent"))
        .expect("create agent markdown parent");
    fs::write(path, content).expect("write agent markdown");
}

fn write_agent_markdown(repo_root: &Path, name: &str, content: &str) {
    write_agent_markdown_in(repo_root, ".agent-harness", name, content);
}

fn write_legacy_agent_markdown(repo_root: &Path, name: &str, content: &str) {
    write_agent_markdown_in(repo_root, ".agent-harness", name, content);
}

fn public_minimal_config_with_permission(permission: &str) -> String {
    format!(
        r#"
        {{
          provider: {{
            default: {{
              type: "openai_compatible",
              options: {{
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              }},
              models: {{
                "gpt-4o-mini": {{
                  name: "GPT-4o mini"
                }}
              }}
            }}
          }},
          model: "default/gpt-4o-mini",
          agent: {{
            build: {{
              system_prompt: "Build work"
            }}
          }},
          default_agent: "build",
          permission: {permission}
        }}
        "#
    )
}

mod agents_profiles_test;
mod discovery_schema_test;
mod env_assets_test;
mod permissions_models_test;
mod public_basics_test;
