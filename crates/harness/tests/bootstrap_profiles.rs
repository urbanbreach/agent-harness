use harness_core::config::load_config_from_str;
use harness_core::perm::{PermissionKind, PolicyDecision};

#[allow(dead_code)]
#[path = "../src/bootstrap.rs"]
mod bootstrap;

#[test]
fn interactive_bootstrap_builds_runtime_state_from_profiles() {
    let config = load_config_from_str(
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
                "gpt-4o-mini": {
                  display_name: "GPT-4o mini",
                },
              },
            },
          },
          profiles: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              tools: [],
            },
            planner: {
              description: "Planning mode",
              model_ref: "default:gpt-4o-mini",
              plan_mode: true,
              exit_target_profile: "deep",
              permissions: {
                task: "deny",
              },
              tools: [],
            },
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow",
            },
            shell_allowlist: {
              executables: ["git"],
              cwd_roots: ["."],
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
          },
        }
        "#,
    )
    .expect("config should parse");

    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");

    assert!(coordinator_config.agent_profiles.contains_key("deep"));
    assert!(coordinator_config.agent_profiles.contains_key("planner"));
    assert_eq!(
        coordinator_config.agent_profiles["planner"].system_prompt,
        "You are the planner agent. Planning mode"
    );

    assert!(coordinator_config.plan_profiles["planner"].plan_mode);
    assert_eq!(
        coordinator_config.plan_profiles["planner"]
            .exit_target_profile
            .as_deref(),
        Some("deep")
    );

    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("planner"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}
