use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use harness_core::config::{load_config_from_file, load_config_from_str};
use harness_core::perm::{PermissionKind, PolicyDecision};
use tempfile::tempdir;

#[allow(dead_code)]
#[path = "../src/bootstrap.rs"]
mod bootstrap;

static DISCOVERY_TEST_LOCK: Mutex<()> = Mutex::new(());

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn set(path: &Path) -> Self {
        let previous = env::current_dir().expect("capture current dir");
        env::set_current_dir(path).expect("set current dir");
        Self { previous }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.previous);
    }
}

fn write_agent_markdown(repo_root: &Path, name: &str, body: &str) {
    let path = repo_root
        .join(".agent-harness")
        .join("agents")
        .join(format!("{name}.md"));
    fs::create_dir_all(path.parent().expect("agent markdown parent"))
        .expect("create agent markdown dir");
    fs::write(path, body).expect("write agent markdown");
}

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
              system_prompt: "Deep prompt",
              model_ref: "default:gpt-4o-mini",
              tools: [],
            },
            review: {
              description: "Review mode",
              system_prompt: "Review prompt",
              model_ref: "default:gpt-4o-mini",
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
    assert!(coordinator_config.agent_profiles.contains_key("review"));
    assert_eq!(
        coordinator_config.agent_profiles["review"].system_prompt,
        "Review prompt"
    );

    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("review"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}

#[test]
fn build_and_review_permissions_follow_configured_policy() {
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
            build: {
              description: "Implementation mode",
              system_prompt: "Build prompt",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                edit: "allow",
                shell: "allow",
                task: "allow",
              },
              tools: [],
            },
            review: {
              description: "Review mode",
              system_prompt: "Review prompt",
              model_ref: "default:gpt-4o-mini",
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
            .evaluate(Some("review"), PermissionKind::EditFs),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("review"), PermissionKind::Shell),
        PolicyDecision::Deny
    );
    assert_eq!(
        coordinator_config
            .permission_policy
            .evaluate(Some("review"), PermissionKind::Task),
        PolicyDecision::Deny
    );
}

#[test]
fn interactive_bootstrap_uses_discovered_markdown_prompt_when_inline_missing() {
    let _lock = DISCOVERY_TEST_LOCK.lock().expect("lock discovery tests");
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");
    fs::create_dir_all(&repo).expect("create repo");
    let _cwd = CurrentDirGuard::set(&repo);

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
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
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
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
    .expect("write config");
    write_agent_markdown(&repo, "build", "Markdown-backed build prompt.");

    let config = load_config_from_file(&config_path).expect("load harness config");
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");
    assert_eq!(
        coordinator_config.agent_profiles["build"].system_prompt,
        "Markdown-backed build prompt."
    );
}

#[test]
fn interactive_bootstrap_prepends_project_agents_md_to_agent_prompt() {
    let _lock = DISCOVERY_TEST_LOCK.lock().expect("lock discovery tests");
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");
    fs::create_dir_all(&repo).expect("create repo");
    let _cwd = CurrentDirGuard::set(&repo);

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
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
            build: {
              description: "Build work",
              system_prompt: "Build prompt",
              model_ref: "default:gpt-4o-mini",
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
    .expect("write config");
    fs::write(repo.join("AGENTS.md"), "Project-wide instructions.")
        .expect("write project instructions");

    let config = load_config_from_file(&config_path).expect("load harness config");
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");
    let prompt = &coordinator_config.agent_profiles["build"].system_prompt;
    assert!(prompt.contains("Instructions from:"));
    assert!(prompt.contains("Project-wide instructions."));
    assert!(prompt.ends_with("Build prompt"));
}
