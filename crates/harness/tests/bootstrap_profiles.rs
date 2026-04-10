use std::fs;
use std::path::PathBuf;

use harness_core::config::{load_config_from_str, named_agent_system_prompt};
use harness_core::perm::{PermissionKind, PolicyDecision};

#[allow(dead_code)]
#[path = "../src/bootstrap.rs"]
mod bootstrap;

#[test]
fn interactive_bootstrap_builds_runtime_state_from_agents() {
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
          agents: {
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

#[test]
fn opencode_primary_agents_enforce_build_and_plan_permissions() {
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
          agent: {
            build: {
              description: "Implementation mode",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                edit: "allow",
                shell: "allow",
                task: "allow",
              },
              tools: [],
            },
            plan: {
              description: "Planning mode",
              model_ref: "default:gpt-4o-mini",
              plan_mode: true,
              exit_target_profile: "build",
              permissions: {
                edit: "deny",
                shell: "deny",
                task: "deny",
              },
              tools: [],
            },
          },
          default_agent: "build",
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow",
              task: "allow",
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

    assert_eq!(bootstrap::interactive_profile_name(&config), "build");
    assert_eq!(
        coordinator_config.agent_profiles["plan"].system_prompt,
        named_agent_system_prompt("plan").expect("named plan prompt")
    );
    assert_eq!(
        coordinator_config.agent_profiles["build"].system_prompt,
        named_agent_system_prompt("build").expect("named build prompt")
    );
    assert!(coordinator_config.plan_profiles["plan"].plan_mode);
    assert_eq!(
        coordinator_config.plan_profiles["plan"]
            .exit_target_profile
            .as_deref(),
        Some("build")
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("build"), PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("build"), PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("plan"), PermissionKind::EditFs),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("plan"), PermissionKind::Shell),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("plan"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}

#[test]
fn shipped_plan_build_docs_exist_and_stay_linked_from_readme() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let readme = fs::read_to_string(repo_root.join("README.md")).expect("read README");

    for relative in [
        "docs/config.md",
        "docs/testing.md",
        "docs/plan-build-workflow.md",
    ] {
        let path = repo_root.join(relative);
        assert!(path.is_file(), "expected {relative} to exist");
        assert!(
            readme.contains(relative),
            "README should keep linking to {relative}"
        );
    }

    let workflow = fs::read_to_string(repo_root.join("docs/plan-build-workflow.md"))
        .expect("read plan/build workflow doc");
    assert!(workflow.contains("plan.exit"));
    assert!(workflow.contains("read-only"));
    assert!(workflow.contains("build"));

    let testing =
        fs::read_to_string(repo_root.join("docs/testing.md")).expect("read testing signoff doc");
    assert!(testing.contains("live_proxy_e2e"));
    assert!(testing.contains("plan_exit_handoff"));
}
