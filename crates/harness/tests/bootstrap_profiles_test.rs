use harness_core::config::{
    load_config_from_file_with_context, load_config_from_str, ConfigLoadContext, HarnessConfig,
};
use harness_core::perm::{PermissionKind, PolicyDecision};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const V1_PROMPT_PROFILES: &str = "build plan discipline general explore visual-engineering artistry ultrabrain deep quick unspecified-low unspecified-high writing";

#[allow(dead_code)]
#[path = "../src/dynamic_prompt.rs"]
mod dynamic_prompt;

#[allow(dead_code)]
#[path = "../src/bootstrap.rs"]
mod bootstrap;

#[allow(dead_code)]
#[path = "../src/cli_config.rs"]
mod cli_config;

fn write_agent_markdown(repo_root: &Path, name: &str, body: &str) {
    let path = repo_root
        .join(".agent-harness")
        .join("agents")
        .join(format!("{name}.md"));
    fs::create_dir_all(path.parent().expect("agent markdown parent"))
        .expect("create agent markdown dir");
    fs::write(path, body).expect("write agent markdown");
}

fn load_config_from_repo_file(config_path: &Path, repo: &Path) -> HarnessConfig {
    let context = ConfigLoadContext::from_env().with_current_dir(repo.to_path_buf());
    load_config_from_file_with_context(config_path, &context).expect("load harness config")
}

#[test]
fn shipped_v1_prompt_assets_have_contract_bodies() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);

    // act
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");

    // assert
    for profile in V1_PROMPT_PROFILES.split_whitespace() {
        let prompt = &coordinator_config.agent_profiles[profile].system_prompt;
        for required in [
            "## Identity",
            "## Goal",
            "## Use When",
            "## Do Not Use When",
            "## Scope Guard",
            "## Runtime-Enforced Permissions",
            "## Behavioral Guidance",
            "## Operating Loop",
            "## Ask Gate",
            "## Failure Recovery",
            "## Output Contract",
            "## Verification Gate",
        ] {
            assert!(
                prompt.contains(required),
                "{profile} prompt missing required V1 prompt section {required}"
            );
        }
    }
}

#[test]
fn shipped_v1_prompt_asset_snapshot_matches_source() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("v1_prompt_assets.json");

    // act
    let actual = shipped_v1_prompt_asset_snapshot(&repo_root);
    let expected = fs::read_to_string(&snapshot_path).unwrap_or_else(|_| {
        panic!(
            "missing prompt snapshot {}; expected:\n{}",
            snapshot_path.display(),
            serde_json::to_string_pretty(&actual).expect("render actual prompt snapshot")
        )
    });
    let expected: serde_json::Value =
        serde_json::from_str(&expected).expect("parse prompt asset snapshot");

    // assert
    assert_eq!(actual, expected, "shipped V1 prompt asset snapshot drifted");
}

fn shipped_v1_prompt_asset_snapshot(repo_root: &Path) -> serde_json::Value {
    let mut profiles = serde_json::Map::new();
    for profile in V1_PROMPT_PROFILES.split_whitespace() {
        let asset_path = repo_root
            .join(".agent-harness")
            .join("agents")
            .join(format!("{profile}.md"));
        let markdown = fs::read_to_string(&asset_path)
            .unwrap_or_else(|err| panic!("read prompt asset {}: {err}", asset_path.display()));
        let body = prompt_body_from_markdown(&markdown);
        let digest12 = blake3::hash(body.as_bytes())
            .to_hex()
            .chars()
            .take(12)
            .collect::<String>();
        let sections = body
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .map(str::to_string)
            .collect::<Vec<_>>();
        profiles.insert(
            profile.to_string(),
            serde_json::json!({
                "digest12": digest12,
                "line_count": body.lines().count(),
                "sections": sections,
            }),
        );
    }

    serde_json::json!({
        "schema_version": "v1-prompt-assets-snapshot-v1",
        "profiles": profiles,
    })
}

fn prompt_body_from_markdown(markdown: &str) -> String {
    let mut lines = markdown.lines();
    if lines.next() == Some("---") {
        for line in &mut lines {
            if line == "---" {
                break;
            }
        }
        return lines.collect::<Vec<_>>().join("\n").trim().to_string();
    }
    markdown.trim().to_string()
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
    let review_prompt = &coordinator_config.agent_profiles["review"].system_prompt;
    assert!(review_prompt.starts_with("Review prompt"));
    assert!(review_prompt.contains("The exact model ID is default/gpt-4o-mini"));

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
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");
    fs::create_dir_all(&repo).expect("create repo");

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

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");
    let prompt = &coordinator_config.agent_profiles["build"].system_prompt;
    assert!(prompt.starts_with("Markdown-backed build prompt."));
    assert!(prompt.contains("The exact model ID is default/gpt-4o-mini"));
}

#[test]
fn interactive_bootstrap_uses_discovered_discipline_markdown_prompt() {
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");
    fs::create_dir_all(&repo).expect("create repo");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "test-key",
              apiMode: "responses",
              models: {
                "gpt-5.4-mini": {
                  name: "GPT-5.4 mini",
                },
              },
            },
          },
          model: "default/gpt-5.4-mini",
          agent: {
            discipline: {
              enable: true,
            },
          },
          default_agent: "discipline",
          permission: "ask",
        }
        "#,
    )
    .expect("write config");
    write_agent_markdown(
        &repo,
        "discipline",
        r#"---
{ description: "Temp discipline prompt" }
---

Unique markdown discipline prompt.
"#,
    );

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");
    let prompt = &coordinator_config.agent_profiles["discipline"].system_prompt;
    assert!(prompt.starts_with("Unique markdown discipline prompt."));
    assert!(prompt.contains("The exact model ID is default/gpt-5.4-mini"));
}

#[test]
fn interactive_bootstrap_allows_discipline_without_markdown_prompt() {
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");
    fs::create_dir_all(&repo).expect("create repo");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "test-key",
              apiMode: "responses",
              models: {
                "gpt-5.4-mini": {
                  name: "GPT-5.4 mini",
                },
              },
            },
          },
          model: "default/gpt-5.4-mini",
          agent: {
            discipline: {
              enable: true,
            },
          },
          default_agent: "discipline",
          permission: "ask",
        }
        "#,
    )
    .expect("write config");

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");
    let prompt = &coordinator_config.agent_profiles["discipline"].system_prompt;
    assert!(prompt.starts_with("## Identity"));
    assert!(prompt.contains("You are the Discipline agent for Harness"));
    assert!(prompt.contains("The exact model ID is default/gpt-5.4-mini"));
}

#[test]
fn interactive_bootstrap_prepends_project_agents_md_to_agent_prompt() {
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");
    fs::create_dir_all(&repo).expect("create repo");

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

    let config = load_config_from_repo_file(&config_path, &repo);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).expect("build config");
    let prompt = &coordinator_config.agent_profiles["build"].system_prompt;
    assert!(prompt.contains("Instructions from:"));
    assert!(prompt.contains("Project-wide instructions."));
    assert!(prompt.starts_with("Build prompt"));
    assert!(prompt.contains("The exact model ID is default/gpt-4o-mini"));
}
