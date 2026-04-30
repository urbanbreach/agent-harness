use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, load_config_from_file,
    OpenAiApiMode, PermissionMode, ProviderConfig, PublicRuntimeConfig, PublicTuiConfig,
};
use serde_json::Value;
use tempfile::tempdir;

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn openai_api_key_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[allow(unsafe_code)]
fn with_env_var_state<T>(name: &str, value: Option<&str>, run: impl FnOnce() -> T) -> T {
    let _lock = openai_api_key_env_lock()
        .lock()
        .expect("openai api key env lock poisoned");
    let previous = env::var_os(name);

    match value {
        Some(value) => unsafe { env::set_var(name, value) },
        None => unsafe { env::remove_var(name) },
    }

    let result = run();

    match previous {
        Some(value) => unsafe { env::set_var(name, value) },
        None => unsafe { env::remove_var(name) },
    }

    result
}

fn write_config(path: &Path, config: &serde_json::Value) {
    fs::write(
        path,
        serde_json::to_string_pretty(config).expect("serialize config"),
    )
    .expect("write config");
}

fn harness_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_harness"));
    command
        .env_remove("HARNESS_CONFIG")
        .env_remove("HARNESS_CONFIG_CONTENT")
        .env_remove("HARNESS_TUI_CONFIG")
        .env("HOME", "/nonexistent")
        .env("XDG_CONFIG_HOME", "/nonexistent");
    command
}

fn canonical_runtime_config() -> serde_json::Value {
    serde_json::json!({
        "provider": {
            "default": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "apiKey": "DUMMY",
                "apiMode": "responses",
                "models": {
                    "gpt-4o": {
                        "name": "GPT-4o"
                    },
                    "gpt-4o-mini": {
                        "name": "GPT-4o mini"
                    }
                }
            }
        },
        "model": "default/gpt-4o",
        "small_model": "default/gpt-4o-mini",
        "default_agent": "build",
        "permission": {
            "edit": "allow",
            "bash": "allow",
            "shell_allowlist": {
                "executables": ["git"],
                "cwd_roots": ["."]
            }
        },
        "mcp": {
            "gh_grep": {
                "transport": "http",
                "endpoint": "https://mcp.grep.app",
                "enabled": true
            }
        }
    })
}

fn legacy_runtime_config(session_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "api_mode": "responses",
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini"
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            }
        },
        "default_agent": "deep",
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "session_dir": session_dir,
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "ui": {
            "default_profile": "deep"
        }
    })
}

#[test]
fn schema_cli_prints_runtime_json_schema() {
    let output = harness_command()
        .arg("schema")
        .output()
        .expect("run harness schema");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let schema: Value = serde_json::from_slice(&output.stdout).expect("schema json");
    let root_properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("runtime schema properties");
    assert!(root_properties.contains_key("provider"));
    assert!(root_properties.contains_key("model"));
    assert!(root_properties.contains_key("small_model"));
    assert!(root_properties.contains_key("model_profile"));
    assert!(root_properties.contains_key("permission"));
    assert!(root_properties.contains_key("mcp"));
    assert!(root_properties.contains_key("runtime"));
    assert!(!root_properties.contains_key("integrations"));

    let definitions = schema
        .get("definitions")
        .and_then(Value::as_object)
        .expect("schema definitions");

    let model_properties = definitions["ModelConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("ModelConfig properties");
    assert!(model_properties.contains_key("name"));
    assert!(!model_properties.contains_key("display_name"));

    let variant_properties = definitions["ModelVariantConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("ModelVariantConfig properties");
    assert!(variant_properties.contains_key("name"));
    assert!(!variant_properties.contains_key("display_name"));

    let provider_properties = definitions["ProviderConfig"]["oneOf"][0]
        .get("properties")
        .and_then(Value::as_object)
        .expect("ProviderConfig properties");
    assert!(provider_properties.contains_key("baseURL"));
    assert!(provider_properties.contains_key("apiKey"));
    assert!(provider_properties.contains_key("apiKeyEnv"));
    assert!(provider_properties.contains_key("apiMode"));
    assert!(provider_properties.contains_key("timeoutMs"));
    assert!(!provider_properties.contains_key("base_url"));
    assert!(!provider_properties.contains_key("api_key"));

    let options_properties = definitions["OpenAiCompatibleProviderOptions"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("OpenAiCompatibleProviderOptions properties");
    assert!(options_properties.contains_key("baseURL"));
    assert!(options_properties.contains_key("apiKey"));
    assert!(options_properties.contains_key("apiKeyEnv"));
    assert!(options_properties.contains_key("apiMode"));
    assert!(options_properties.contains_key("timeoutMs"));
    assert!(!options_properties.contains_key("base_url"));
    assert!(!options_properties.contains_key("api_key"));

    let permission_properties = definitions["PublicPermissionConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("PublicPermissionConfig properties");
    assert!(permission_properties.contains_key("bash"));
    assert!(!permission_properties.contains_key("shell"));
    assert!(!permission_properties.contains_key("network"));
}

#[test]
fn schema_cli_prints_tui_json_schema() {
    let output = harness_command()
        .args(["schema", "--tui"])
        .output()
        .expect("run harness schema --tui");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let schema: Value = serde_json::from_slice(&output.stdout).expect("tui schema json");
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("tui schema properties");
    assert!(properties.contains_key("keybinds"));
    assert!(!properties.contains_key("parity"));
    assert!(!properties.contains_key("default_profile"));
}

#[test]
fn config_validate_cli_reports_missing_config() {
    let temp = tempdir().expect("tempdir");
    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate without config");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(stderr.contains("./harness.jsonc"));
    assert!(stderr.contains("./harness.json"));
    assert!(stderr.contains("$XDG_CONFIG_HOME/harness/harness.jsonc"));
    assert!(stderr.contains("configs/harness.example.jsonc"));
}

#[test]
fn config_validate_cli_accepts_shipped_example_config() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with shipped example config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains("configs/harness.example.jsonc"));
}

#[test]
fn config_validate_cli_merges_xdg_defaults_with_local_project_override() {
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");
    let local_config_path = temp.path().join("harness.json");

    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
    write_config(&xdg_config_path, &canonical_runtime_config());
    write_config(
        &local_config_path,
        &serde_json::json!({
            "default_agent": "plan"
        }),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with merged discovery");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(xdg_config_path.to_str().expect("xdg path utf-8")));
    assert!(stdout.contains("harness.json"));
}

#[test]
fn load_config_allows_public_agents_without_explicit_description() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["agent"] = serde_json::json!({
        "plan": {
            "use_small_model": true,
            "tools": []
        }
    });
    config["default_agent"] = serde_json::json!("plan");
    write_config(&config_path, &config);

    let parsed = load_config_from_file(&config_path)
        .expect("public agent without explicit description should still load");
    let plan = parsed
        .agents
        .get("plan")
        .expect("plan profile should be translated from public config");

    assert_eq!(plan.description, "The Plan agent");
    assert_eq!(plan.model_ref, "default/gpt-4o-mini");
}

#[test]
fn config_validate_cli_accepts_legacy_harness_native_shape() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    write_config(
        &config_path,
        &legacy_runtime_config(&temp.path().join("sessions")),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with legacy config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_validate_cli_accepts_legacy_xdg_config_path_for_migration() {
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let legacy_xdg_config = xdg_root.join("harness/config.jsonc");
    fs::create_dir_all(legacy_xdg_config.parent().expect("legacy xdg parent"))
        .expect("create legacy xdg dir");
    write_config(&legacy_xdg_config, &canonical_runtime_config());

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with legacy xdg path");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("config.jsonc"));
}

#[test]
fn config_validate_cli_loads_separate_tui_config() {
    let temp = tempdir().expect("tempdir");
    write_config(
        &temp.path().join("harness.jsonc"),
        &canonical_runtime_config(),
    );
    write_config(
        &temp.path().join("tui.jsonc"),
        &serde_json::json!({
            "keybinds": {
                "variant_cycle": "tab"
            }
        }),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with separate tui config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tui.jsonc"));
}

#[test]
fn config_validate_cli_accepts_harness_config_env_path() {
    let temp = tempdir().expect("tempdir");
    let env_config_path = temp.path().join("env/custom-harness.jsonc");
    fs::create_dir_all(env_config_path.parent().expect("env config parent"))
        .expect("create env config dir");
    write_config(&env_config_path, &canonical_runtime_config());

    let output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_CONFIG", &env_config_path)
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with HARNESS_CONFIG");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains(env_config_path.to_str().expect("env config path utf-8")));
}

#[test]
fn config_validate_cli_applies_harness_config_content_last() {
    let temp = tempdir().expect("tempdir");
    write_config(
        &temp.path().join("harness.jsonc"),
        &canonical_runtime_config(),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env(
            "HARNESS_CONFIG_CONTENT",
            r#"{ default_agent: "plan", permission: { bash: "deny" } }"#,
        )
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with HARNESS_CONFIG_CONTENT");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_validate_cli_explicit_path_bypasses_discovery_layers() {
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let explicit_path = temp.path().join("explicit.jsonc");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");

    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
    write_config(&xdg_config_path, &canonical_runtime_config());
    write_config(
        &explicit_path,
        &serde_json::json!({
            "provider": {
                "default": {
                    "type": "openai_compatible",
                    "baseURL": "http://127.0.0.1:1/v1",
                    "apiKey": "DUMMY",
                    "models": {
                        "gpt-4o": {
                            "name": "GPT-4o"
                        }
                    }
                }
            },
            "mystery": true
        }),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args([
            "--config",
            explicit_path.to_str().expect("explicit config utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with explicit path");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown top-level config keys"));
    assert!(stderr.contains("`mystery`"));
}

#[test]
fn config_validate_cli_rejects_unknown_top_level_keys() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["mystery"] = serde_json::json!({"enabled": true});
    write_config(&config_path, &config);

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with unknown key");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown top-level config keys:"));
    assert!(stderr.contains("`mystery`"));
}

#[test]
fn config_validate_cli_rejects_unknown_provider_reference() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["agent"] = serde_json::json!({
        "deep": {
            "description": "Deep profile",
            "model": "missing/gpt-4o-mini",
            "tools": []
        }
    });
    write_config(&config_path, &config);

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with invalid provider ref");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("agent `deep` has invalid model selection `missing/gpt-4o-mini`"));
    assert!(stderr.contains("unknown provider `missing`"));
}

#[test]
fn config_validate_cli_rejects_unsupported_upstream_top_level_key() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["plugin"] = serde_json::json!({ "enabled": true });
    write_config(&config_path, &config);

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with unsupported upstream key");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown top-level config keys:"));
    assert!(stderr.contains("`plugin`"));
}

#[test]
fn shipped_example_config_uses_explicit_local_key_without_openai_api_key() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    with_env_var_state("OPENAI_API_KEY", None, || {
        let parsed = load_config_from_file(&config_path)
            .expect("shipped example config should use its explicit local loopback key");
        let ProviderConfig::OpenAiCompatible(provider) = parsed
            .providers
            .get("default")
            .expect("default provider present in shipped example config");

        assert_eq!(provider.name.as_deref(), Some("CLIProxyAPI (OpenAI)"));
        assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
        assert_eq!(provider.api_key, "sk-zerolimit");
        assert_eq!(provider.timeout_ms, 1_800_000);
        assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
    });
}

#[test]
fn shipped_example_config_keeps_explicit_local_key_even_when_openai_api_key_is_set() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    with_env_var_state("OPENAI_API_KEY", Some("test-openai-api-key"), || {
        let parsed = load_config_from_file(&config_path)
            .expect("shipped example config should keep its explicit local loopback key");
        let ProviderConfig::OpenAiCompatible(provider) = parsed
            .providers
            .get("default")
            .expect("default provider present in shipped example config");

        assert_eq!(provider.name.as_deref(), Some("CLIProxyAPI (OpenAI)"));
        assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
        assert_eq!(provider.api_key, "sk-zerolimit");
        assert_eq!(provider.timeout_ms, 1_800_000);
        assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
    });
}

#[test]
fn checked_in_tui_schema_matches_generated_schema() {
    let generated = harness_tui_schema_pretty_json().expect("generate tui schema");
    let checked_in = fs::read_to_string(repo_root().join("configs").join("tui.json"))
        .expect("read checked-in tui schema");

    assert_eq!(checked_in.trim(), generated.trim());
}

#[test]
fn checked_in_runtime_schema_matches_generated_schema() {
    let generated = harness_schema_pretty_json().expect("generate runtime schema");
    let checked_in = fs::read_to_string(repo_root().join("configs").join("config.json"))
        .expect("read checked-in runtime schema");

    assert_eq!(checked_in.trim(), generated.trim());
}

#[test]
fn shipped_tui_example_parses_as_public_tui_config() {
    let shipped = fs::read_to_string(repo_root().join("configs").join("tui.example.jsonc"))
        .expect("read shipped tui example");

    let parsed: PublicTuiConfig = json5::from_str(&shipped).expect("parse shipped tui example");

    assert_eq!(
        parsed.keybindings.get("variant_cycle"),
        Some(&"tab".to_string())
    );
    assert_eq!(
        parsed.keybindings.get("session_child_first"),
        Some(&"ctrl+]".to_string())
    );
}

#[test]
fn shipped_runtime_example_parses_as_public_runtime_config() {
    let shipped = fs::read_to_string(repo_root().join("configs").join("harness.example.jsonc"))
        .expect("read shipped runtime example");

    let parsed: PublicRuntimeConfig =
        json5::from_str(&shipped).expect("parse shipped runtime example");

    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    assert_eq!(parsed.model.as_deref(), Some("default/gpt-5.4"));
    assert_eq!(parsed.small_model.as_deref(), Some("default/gpt-5.4-mini"));
    assert!(parsed.provider.contains_key("default"));
    assert!(parsed.agent.contains_key("build"));
    assert!(!parsed.provider.contains_key("providers"));
    assert!(!shipped.contains("\"base_url\""));
    assert!(!shipped.contains("\"api_key\""));
    assert!(!shipped.contains("\"api_mode\""));
    assert!(!shipped.contains("\"timeout_ms\""));
    assert!(shipped.contains("\"model_backed\""));
    assert!(shipped.contains("\"split_oversized_turns\""));
    assert!(shipped.contains("\"auto_retry_overflow\""));
    assert!(!shipped.contains("\"modelBacked\""));
    assert!(!shipped.contains("\"splitOversizedTurns\""));
    assert!(!shipped.contains("\"autoRetryOverflow\""));
}

#[test]
fn public_runtime_config_accepts_top_level_skills() {
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          skills: {
            walkToGitRoot: false,
            permissions: {
              "*": "allow"
            }
          }
        }
        "#,
    )
    .expect("parse runtime config with top-level skills");

    assert!(!parsed.skills.walk_to_git_root);
    assert_eq!(
        parsed.skills.permissions.get("*"),
        Some(&PermissionMode::Allow)
    );
}

#[test]
fn public_runtime_config_accepts_compaction_settings() {
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          runtime: {
            compaction: {
              modelBacked: true,
              model: "default/gpt-5.4-mini",
              splitOversizedTurns: true,
              autoRetryOverflow: false,
              structuredSummaryContract: false,
              estimatedTokenTriggers: false,
              fallbackInputTokens: 65536,
            }
          }
        }
        "#,
    )
    .expect("parse runtime compaction config");

    assert!(parsed.runtime.compaction.model_backed);
    assert_eq!(
        parsed.runtime.compaction.model_ref.as_deref(),
        Some("default/gpt-5.4-mini")
    );
    assert!(parsed.runtime.compaction.split_oversized_turns);
    assert!(!parsed.runtime.compaction.auto_retry_overflow);
    assert!(!parsed.runtime.compaction.structured_summary_contract);
    assert!(!parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 65_536);
}

#[test]
fn public_runtime_config_accepts_new_compaction_settings() {
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          runtime: {
            compaction: {
              structured_summary_contract: true,
              estimated_token_triggers: true,
              fallback_input_tokens: 32768,
            }
          }
        }
        "#,
    )
    .expect("parse runtime compaction config with new canonical keys");

    assert!(parsed.runtime.compaction.structured_summary_contract);
    assert!(parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 32_768);
}

#[test]
fn root_runtime_example_uses_canonical_public_keys() {
    let root_example =
        fs::read_to_string(repo_root().join("harness.jsonc")).expect("read root runtime example");

    let parsed: PublicRuntimeConfig =
        json5::from_str(&root_example).expect("parse root runtime example");

    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    assert_eq!(parsed.model.as_deref(), Some("default/gpt-5.4"));
    assert!(!root_example.contains("\"base_url\""));
    assert!(!root_example.contains("\"api_key\""));
    assert!(!root_example.contains("\"api_mode\""));
    assert!(!root_example.contains("\"timeout_ms\""));
}
