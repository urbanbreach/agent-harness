use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, load_config_from_file,
    load_config_from_str, AgentMode, OpenAiApiMode, PermissionMode, ProviderConfig,
    PublicRuntimeConfig, PublicTuiConfig,
};
use serde_json::Value;
use tempfile::tempdir;

mod common;

use common::repo_root;

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

    if let Some(value) = value {
        unsafe { env::set_var(name, value) };
    } else {
        unsafe { env::remove_var(name) };
    }

    let result = run();

    if let Some(value) = previous {
        unsafe { env::set_var(name, value) };
    } else {
        unsafe { env::remove_var(name) };
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

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(
        path,
        serde_json::to_string_pretty(value).expect("serialize json"),
    )
    .expect("write json");
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

#[test]
fn models_probe_generates_harness_catalog_fragment_from_models_dev_json() {
    let temp = tempdir().expect("create tempdir");
    let source_path = temp.path().join("models-dev.json");
    write_json(
        &source_path,
        &serde_json::json!({
            "openai": {
                "id": "openai",
                "name": "OpenAI",
                "env": ["OPENAI_API_KEY"],
                "npm": "@ai-sdk/openai",
                "doc": "https://platform.openai.com/docs",
                "api": "https://api.openai.com/v1",
                "models": {
                    "gpt-5-mini": {
                        "id": "gpt-5-mini",
                        "name": "GPT-5 mini",
                        "attachment": true,
                        "reasoning": true,
                        "temperature": false,
                        "tool_call": true,
                        "structured_output": true,
                        "release_date": "2026-01-15",
                        "last_updated": "2026-02-01",
                        "cost": {
                            "input": 0.25,
                            "output": 2.0,
                            "cache_read": 0.025,
                            "cache_write": 0.25
                        },
                        "limit": {
                            "context": 128000,
                            "input": 120000,
                            "output": 16000
                        },
                        "modalities": {
                            "input": ["text", "image"],
                            "output": ["text"]
                        }
                    },
                    "gpt-old": {
                        "id": "gpt-old",
                        "name": "GPT old",
                        "reasoning": false,
                        "tool_call": true,
                        "status": "deprecated",
                        "limit": { "context": 4096, "output": 1024 }
                    },
                    "gpt-text-only": {
                        "id": "gpt-text-only",
                        "name": "GPT text only",
                        "reasoning": false,
                        "tool_call": false,
                        "limit": { "context": 8192, "output": 2048 }
                    }
                }
            },
            "anthropic": {
                "id": "anthropic",
                "name": "Anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "models": {
                    "claude-sonnet": {
                        "id": "claude-sonnet",
                        "name": "Claude Sonnet",
                        "reasoning": true,
                        "tool_call": true,
                        "limit": { "context": 200000, "output": 8192 }
                    }
                }
            }
        }),
    );

    let output = harness_command()
        .args([
            "models",
            "probe",
            "--input",
            source_path.to_str().expect("utf8 source path"),
            "--provider",
            "openai",
            "--emit-reasoning-variants",
        ])
        .output()
        .expect("run harness models probe");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let catalog: Value = serde_json::from_slice(&output.stdout).expect("catalog json");
    assert_eq!(catalog["$schema"], "./config.json");
    assert!(catalog["provider"].get("anthropic").is_none());

    let openai = &catalog["provider"]["openai"];
    assert_eq!(openai["type"], "openai_compatible");
    assert_eq!(openai["name"], "OpenAI");
    assert_eq!(openai["options"]["baseURL"], "https://api.openai.com/v1");
    assert_eq!(openai["options"]["apiKeyEnv"][0], "OPENAI_API_KEY");

    let models = openai["models"].as_object().expect("models object");
    assert!(models.contains_key("gpt-5-mini"));
    assert!(!models.contains_key("gpt-old"));
    assert!(!models.contains_key("gpt-text-only"));

    let model = &models["gpt-5-mini"];
    assert_eq!(model["metadata"]["contextWindowTokens"], 128000);
    assert_eq!(model["metadata"]["supportsToolCalls"], true);
    assert_eq!(model["limit"]["input"], 120000);
    assert_eq!(model["limit"]["output"], 16000);
    assert_eq!(model["modalities"]["input"][1], "image");
    assert_eq!(
        model["options"]["modelsDev"]["source"],
        format!("file://{}", source_path.display())
    );
    assert_eq!(
        model["options"]["modelsDev"]["capabilities"]["reasoning"],
        true
    );
    assert_eq!(model["options"]["modelsDev"]["cost"]["cache_read"], 0.025);
    assert_eq!(
        model["variants"]["high"]["metadata"]["reasoningEffort"],
        "high"
    );
}

#[test]
fn models_generate_updates_static_catalog_artifact_from_models_dev_json() {
    let temp = tempdir().expect("create tempdir");
    let source_path = temp.path().join("models-dev.json");
    let output_path = temp.path().join("provider-catalog.generated.json");
    write_json(
        &source_path,
        &serde_json::json!({
            "openai": {
                "id": "openai",
                "name": "OpenAI",
                "env": ["OPENAI_API_KEY"],
                "api": "https://api.openai.com/v1",
                "models": {
                    "gpt-5-mini": {
                        "id": "gpt-5-mini",
                        "name": "GPT-5 mini",
                        "reasoning": true,
                        "tool_call": true,
                        "limit": { "context": 128000, "input": 120000, "output": 16000 },
                        "modalities": { "input": ["text"], "output": ["text"] }
                    }
                }
            }
        }),
    );

    let output = harness_command()
        .args([
            "models",
            "generate",
            "--input",
            source_path.to_str().expect("utf8 source path"),
            "--output",
            output_path.to_str().expect("utf8 output path"),
            "--emit-reasoning-variants",
        ])
        .output()
        .expect("run harness models generate");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "generate should write the artifact instead of stdout"
    );

    let generated = fs::read_to_string(&output_path).expect("read generated catalog");
    let compact: Value = serde_json::from_str(&generated).expect("generated catalog json");
    assert_eq!(
        generated.matches('\n').count(),
        1,
        "generate should produce one compact JSON line"
    );
    assert!(
        generated.ends_with('\n'),
        "generate should preserve a trailing newline"
    );
    assert_eq!(
        compact["provider"]["openai"]["models"]["gpt-5-mini"]["name"],
        "GPT-5 mini"
    );
    assert_eq!(
        compact["provider"]["openai"]["models"]["gpt-5-mini"]["variants"]["medium"]["metadata"]
            ["reasoningEffort"],
        "medium"
    );
}

#[test]
fn models_generated_prints_embedded_static_catalog() {
    let output = harness_command()
        .args(["models", "generated"])
        .output()
        .expect("run harness models generated");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let catalog: Value = serde_json::from_slice(&output.stdout).expect("embedded catalog json");
    assert_eq!(catalog["$schema"], "./config.json");
    assert!(catalog["provider"]
        .as_object()
        .expect("provider object")
        .contains_key("openai"));
    serde_json::from_slice::<PublicRuntimeConfig>(&output.stdout)
        .expect("embedded generated catalog should parse as public runtime config fragment");
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
    assert!(root_properties.contains_key("mode"));
    assert!(root_properties.contains_key("server"));
    assert!(root_properties.contains_key("command"));
    assert!(root_properties.contains_key("plugin"));
    assert!(root_properties.contains_key("formatter"));
    assert!(root_properties.contains_key("tool_output"));
    assert!(!root_properties.contains_key("integrations"));

    let definitions = schema
        .get("definitions")
        .and_then(Value::as_object)
        .expect("schema definitions");

    let agent_map_properties = definitions["PublicAgentMap"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("PublicAgentMap properties");
    for agent in [
        "build",
        "plan",
        "discipline",
        "general",
        "explore",
        "visual-engineering",
        "artistry",
        "ultrabrain",
        "deep",
        "quick",
        "unspecified-low",
        "unspecified-high",
        "writing",
        "title",
        "summary",
        "compaction",
    ] {
        assert!(
            agent_map_properties.contains_key(agent),
            "agent schema should expose built-in agent `{agent}`"
        );
    }
    assert_eq!(
        definitions["PublicAgentMap"]
            .get("additionalProperties")
            .and_then(|value| value.get("$ref"))
            .and_then(Value::as_str),
        Some("#/definitions/PublicAgentConfig"),
        "agent schema should type custom agent names as PublicAgentConfig"
    );

    let model_properties = definitions["ModelConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("ModelConfig properties");
    assert!(model_properties.contains_key("name"));
    assert!(model_properties.contains_key("limit"));
    assert!(model_properties.contains_key("modalities"));
    assert!(model_properties.contains_key("options"));
    assert!(model_properties.contains_key("variants"));
    assert!(!model_properties.contains_key("reasoningEfforts"));
    assert!(!model_properties.contains_key("display_name"));

    let variant_properties = definitions["ModelVariantConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("ModelVariantConfig properties");
    assert!(variant_properties.contains_key("name"));
    assert!(variant_properties.contains_key("limit"));
    assert!(variant_properties.contains_key("modalities"));
    assert!(variant_properties.contains_key("options"));
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
    assert!(permission_properties.contains_key("task"));
    assert!(!permission_properties.contains_key("shell"));
    assert!(!permission_properties.contains_key("network"));

    let agent_properties = definitions["PublicAgentConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .expect("PublicAgentConfig properties");
    for key in [
        "name",
        "description",
        "system_prompt",
        "model",
        "top_p",
        "mode",
        "hidden",
        "color",
        "options",
        "enable",
        "disable",
        "max_iters",
        "tools",
    ] {
        assert!(
            agent_properties.contains_key(key),
            "missing agent schema key {key}"
        );
    }
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

    let parsed = load_config_from_file(&config_path).expect("shipped example config should parse");
    assert_eq!(parsed.providers.len(), 1);
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("default")
        .expect("default provider present in shipped example config");
    assert_eq!(provider.models.len(), 2);
    assert!(provider.models.contains_key("gpt-5.5"));
    assert!(provider.models.contains_key("gpt-5.4-mini"));
    assert!(parsed.agents.contains_key("build"));
    assert!(parsed.agents.contains_key("discipline"));
    for category in [
        "visual-engineering",
        "artistry",
        "ultrabrain",
        "deep",
        "quick",
        "unspecified-low",
        "unspecified-high",
        "writing",
    ] {
        let agent = parsed
            .agents
            .get(category)
            .unwrap_or_else(|| panic!("missing category route profile {category}"));
        assert_eq!(agent.mode, AgentMode::Subagent);
        assert!(!agent.hidden);
        assert_eq!(
            agent
                .permissions
                .as_ref()
                .and_then(|permissions| permissions.task.as_ref()),
            Some(&PermissionMode::Deny),
            "category route {category} should not recursively redelegate by default"
        );
    }
    assert!(!parsed.runtime.compaction.model_backed);
    assert!(parsed.runtime.compaction.auto_retry_overflow);
    assert!(parsed.runtime.compaction.structured_summary_contract);
    assert!(parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 32_768);
}

#[test]
fn doctor_cli_reports_shipped_orchestration_health() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
        ])
        .output()
        .expect("run harness doctor with shipped example config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor ok:"));
    assert!(stdout.contains("provider_credentials"));
    assert!(stdout.contains("model_references"));
    assert!(stdout.contains("workflow_profiles"));
    assert!(stdout.contains("category_routes"));
    assert!(stdout.contains("discipline"));
    assert!(stdout.contains("visual-engineering"));
}

#[test]
fn doctor_cli_emits_json_report() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run harness doctor --json with shipped example config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    assert!(report["config"]
        .as_str()
        .expect("config display")
        .contains("configs/harness.example.jsonc"));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "workflow_profiles" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "category_routes" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "provider_credentials" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "model_references" && check["status"] == "pass" }));
}

#[test]
fn doctor_cli_fails_invalid_category_routes_even_when_some_are_missing() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "DUMMY",
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "default/gpt-5.4-mini",
          agent: {
            "visual-engineering": { hidden: true },
            artistry: { enable: false },
          },
          permission: "ask",
        }
        "#,
    )
    .expect("write invalid category route config");

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
        ])
        .output()
        .expect("run harness doctor with invalid category routes");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor found issues:"));
    assert!(stdout.contains("[FAIL] category_routes"));
    assert!(stdout.contains("visual-engineering"));
    assert!(stdout.contains("artistry"));
}

#[test]
fn doctor_cli_reports_model_profile_fallback_targets() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "DUMMY",
              models: {
                "gpt-5.5": { name: "GPT-5.5" },
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model_profile: {
            fast: {
              model: "default/gpt-5.5",
              fallback: [{ model: "default/gpt-5.4-mini" }],
            },
          },
          model: "fast",
          agent: {
            build: { enable: true, model: "fast" },
          },
          permission: "ask",
        }
        "#,
    )
    .expect("write config with model profile fallback");

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run harness doctor with model profile fallback");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    let model_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "model_references")
        .expect("model references check");
    assert_eq!(model_check["status"], "pass");
    let message = model_check["message"]
        .as_str()
        .expect("model check message");
    assert!(message.contains("1 model profile(s) resolve"));
    assert!(message.contains("fallback target(s)"));
}

#[test]
fn doctor_cli_warns_when_provider_credentials_are_missing() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "default/gpt-5.4-mini",
          permission: "ask",
        }
        "#,
    )
    .expect("write config without credentials");

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
        ])
        .output()
        .expect("run harness doctor with missing credentials");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor ok with warnings:"));
    assert!(stdout.contains("[WARN] provider_credentials"));
    assert!(stdout.contains("default (set apiKey or apiKeyEnv)"));
}

#[test]
fn doctor_cli_reports_env_provider_credentials_without_revealing_values() {
    with_env_var_state(
        "HARNESS_DOCTOR_TEST_API_KEY",
        Some("super-secret-test-key"),
        || {
            let temp = tempdir().expect("tempdir");
            fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
            let config_path = temp.path().join("harness.jsonc");
            fs::write(
                &config_path,
                r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  apiKeyEnv: ["HARNESS_DOCTOR_TEST_API_KEY"],
                  models: {
                    "gpt-5.4-mini": { name: "GPT-5.4 mini" },
                  },
                },
              },
              model: "default/gpt-5.4-mini",
              permission: "ask",
            }
            "#,
            )
            .expect("write env credential config");

            let output = harness_command()
                .current_dir(temp.path())
                .args([
                    "--config",
                    config_path.to_str().expect("config path utf-8"),
                    "doctor",
                    "--json",
                ])
                .output()
                .expect("run harness doctor with env credentials");

            assert!(
                output.status.success(),
                "stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("super-secret-test-key"));
            let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
            let credential_check = report["checks"]
                .as_array()
                .expect("checks array")
                .iter()
                .find(|check| check["name"] == "provider_credentials")
                .expect("provider credential check");
            assert_eq!(credential_check["status"], "pass");
            assert!(credential_check["message"]
                .as_str()
                .expect("credential message")
                .contains("1 via environment"));
        },
    );
}

#[test]
fn config_validate_cli_accepts_provider_catalog_reference_config_by_explicit_path() {
    let repo_root = repo_root();
    let config_path = repo_root
        .join("configs")
        .join("provider-catalog.reference.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with reference catalog config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains("configs/provider-catalog.reference.jsonc"));

    let parsed = load_config_from_file(&config_path).expect("reference catalog should parse");
    assert_eq!(parsed.providers.len(), 1);
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("default")
        .expect("default provider present in reference catalog");
    assert!(provider.models.len() > 1);
}

#[test]
fn config_validate_cli_does_not_auto_discover_provider_catalog_reference_config() {
    let temp = tempdir().expect("tempdir");
    let configs_dir = temp.path().join("configs");
    fs::create_dir_all(&configs_dir).expect("create configs dir");
    fs::copy(
        repo_root()
            .join("configs")
            .join("provider-catalog.reference.jsonc"),
        configs_dir.join("provider-catalog.reference.jsonc"),
    )
    .expect("copy reference catalog fixture");

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with only reference catalog present");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(!stderr.contains("provider-catalog.reference.jsonc"));
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

    assert_eq!(
        plan.description,
        "Plan mode. Disallows all edit tools except the active plan file."
    );
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
fn config_validate_cli_rejects_unsupported_upstream_top_level_keys() {
    for key in [
        "server",
        "command",
        "plugin",
        "share",
        "autoshare",
        "autoupdate",
        "enterprise",
    ] {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("harness.jsonc");
        let mut config = canonical_runtime_config();
        config[key] = serde_json::json!({ "enabled": true });
        write_config(&config_path, &config);

        let output = harness_command()
            .current_dir(temp.path())
            .args(["config", "validate"])
            .output()
            .unwrap_or_else(|err| {
                panic!("run harness config validate with unsupported key {key}: {err}")
            });

        assert!(
            !output.status.success(),
            "unsupported key {key} unexpectedly validated"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unsupported active OpenCode config keys:"),
            "unsupported key {key} stderr did not mention unsupported active OpenCode keys:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("`{key}`")),
            "unsupported key {key} stderr did not name the key:\n{stderr}"
        );
    }
}

#[test]
fn opencode_config_shape_accepts_subagents_and_safe_inert_keys() {
    let parsed = load_config_from_str(
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:1/v1",
                apiKey: "DUMMY"
              },
              models: {
                "gpt-4o": { name: "GPT-4o" }
              }
            }
          },
          model: "default/gpt-4o",
          default_agent: "build",
          shell: "bash",
          logLevel: "INFO",
          username: "operator",
          watcher: { ignore: ["target/**"] },
          snapshot: false,
          disabled_providers: ["unused"],
          enabled_providers: ["default"],
          formatter: false,
          lsp: false,
          layout: "stretch",
          tools: { bash: true },
          tool_output: { max_lines: 20 },
          compaction: { auto: true, tail_turns: 2 },
          experimental: { batch_tool: true },
          skills: { paths: [".opencode/skill"], urls: ["https://example.test/skills"] },
          mcp: {
            local_docs: {
              type: "local",
              command: ["docs-mcp"],
              enabled: false
            },
            disabled_only: { enabled: false }
          },
          mode: {
            legacy: {
              description: "Legacy mode alias",
              mode: "subagent"
            }
          },
          agent: {
            general: {
              description: "Configured general subagent",
              mode: "subagent",
              reasoningEffort: "high"
            },
            reviewer: {
              description: "Review work",
              mode: "subagent",
              customFlag: true
            },
            summary: { enable: false },
            compaction: { enabled: false }
          },
          permission: "allow"
        }
        "#,
    )
    .expect("OpenCode-compatible config shape should parse");

    assert!(parsed.agents.contains_key("build"));
    assert_eq!(
        parsed.agents["general"].mode,
        harness_core::config::AgentMode::Subagent
    );
    assert_eq!(
        parsed.agents["general"].options.get("reasoningEffort"),
        Some(&serde_json::json!("high"))
    );
    assert_eq!(
        parsed.agents["reviewer"].options.get("customFlag"),
        Some(&serde_json::json!(true))
    );
    assert!(parsed.agents.contains_key("legacy"));
    assert!(!parsed.agents.contains_key("summary"));
    assert!(!parsed.agents.contains_key("compaction"));
    assert!(parsed.lsp.disabled);
    assert!(parsed
        .skills
        .project_roots
        .contains(&Path::new(".opencode/skill").to_path_buf()));
    assert!(parsed.integrations.mcp.servers.contains_key("local_docs"));
    assert!(!parsed
        .integrations
        .mcp
        .servers
        .contains_key("disabled_only"));
}

#[test]
fn shipped_example_config_uses_placeholder_safe_provider_without_openai_api_key() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    with_env_var_state("OPENAI_API_KEY", None, || {
        let parsed = load_config_from_file(&config_path)
            .expect("shipped example config should use its explicit placeholder key");
        let ProviderConfig::OpenAiCompatible(provider) = parsed
            .providers
            .get("default")
            .expect("default provider present in shipped example config");

        assert_eq!(
            provider.name.as_deref(),
            Some("Local OpenAI-Compatible Provider")
        );
        assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
        assert_eq!(provider.api_key, "placeholder-api-key");
        assert_eq!(provider.timeout_ms, 1_800_000);
        assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
    });
}

#[test]
fn shipped_example_config_keeps_placeholder_safe_provider_even_when_openai_api_key_is_set() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    with_env_var_state("OPENAI_API_KEY", Some("test-openai-api-key"), || {
        let parsed = load_config_from_file(&config_path)
            .expect("shipped example config should keep its explicit placeholder key");
        let ProviderConfig::OpenAiCompatible(provider) = parsed
            .providers
            .get("default")
            .expect("default provider present in shipped example config");

        assert_eq!(
            provider.name.as_deref(),
            Some("Local OpenAI-Compatible Provider")
        );
        assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
        assert_eq!(provider.api_key, "placeholder-api-key");
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
        Some(&"ctrl+t".to_string())
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
    assert_eq!(parsed.model.as_deref(), Some("default/gpt-5.4-mini"));
    assert_eq!(parsed.small_model.as_deref(), None);
    assert_eq!(parsed.provider.len(), 1);
    assert!(parsed.provider.contains_key("default"));
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .provider
        .get("default")
        .expect("default provider present in public example");
    assert_eq!(provider.models.len(), 2);
    assert!(provider.models.contains_key("gpt-5.5"));
    assert!(provider.models.contains_key("gpt-5.4-mini"));
    assert!(parsed.agent.build.is_some());
    assert!(parsed.agent.plan.is_some());
    assert!(parsed.agent.discipline.is_some());
    assert!(parsed.agent.general.is_some());
    assert!(parsed.agent.explore.is_some());
    assert!(parsed.agent.visual_engineering.is_some());
    assert!(parsed.agent.artistry.is_some());
    assert!(parsed.agent.ultrabrain.is_some());
    assert!(parsed.agent.deep.is_some());
    assert!(parsed.agent.quick.is_some());
    assert!(parsed.agent.unspecified_low.is_some());
    assert!(parsed.agent.unspecified_high.is_some());
    assert!(parsed.agent.writing.is_some());
    assert!(parsed.agent.title.is_some());
    assert!(parsed.agent.summary.is_some());
    assert!(parsed.agent.compaction.is_some());
    assert_eq!(
        parsed.agent.general.as_ref().and_then(|agent| agent.enable),
        Some(true)
    );
    assert_eq!(
        parsed.agent.title.as_ref().and_then(|agent| agent.hidden),
        Some(true)
    );
    let mut variants = provider.models["gpt-5.4-mini"]
        .variants
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    variants.sort_unstable();
    assert_eq!(variants, vec!["high", "low", "medium"]);
    assert!(!parsed.provider.contains_key("providers"));
    assert!(!shipped.contains("\"base_url\""));
    assert!(!shipped.contains("\"api_key\""));
    assert!(!shipped.contains("sk-zerolimit"));
    assert!(!shipped.contains("\"api_mode\""));
    assert!(!shipped.contains("\"timeout_ms\""));
    assert!(!shipped.contains("\"model_backed\""));
    assert!(!shipped.contains("\"split_oversized_turns\""));
    assert!(!shipped.contains("\"auto_retry_overflow\""));
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
    assert_eq!(parsed.small_model.as_deref(), Some("default/gpt-5.4-mini"));
    assert!(parsed.agent.build.is_some());
    assert!(parsed.agent.plan.is_some());
    assert!(parsed.agent.discipline.is_some());
    assert!(parsed.agent.general.is_some());
    assert!(parsed.agent.explore.is_some());
    assert!(parsed.agent.visual_engineering.is_some());
    assert!(parsed.agent.artistry.is_some());
    assert!(parsed.agent.ultrabrain.is_some());
    assert!(parsed.agent.deep.is_some());
    assert!(parsed.agent.quick.is_some());
    assert!(parsed.agent.unspecified_low.is_some());
    assert!(parsed.agent.unspecified_high.is_some());
    assert!(parsed.agent.writing.is_some());
    assert!(parsed.agent.title.is_some());
    assert!(parsed.agent.summary.is_some());
    assert!(parsed.agent.compaction.is_some());
    assert!(!root_example.contains("\"base_url\""));
    assert!(!root_example.contains("\"api_key\""));
    assert!(!root_example.contains("\"api_mode\""));
    assert!(!root_example.contains("\"timeout_ms\""));
    assert!(!root_example.contains("\"model_backed\""));

    let provider = parsed.provider.get("default").expect("default provider");
    let ProviderConfig::OpenAiCompatible(provider) = provider;
    let mini = provider
        .models
        .get("gpt-5.4-mini")
        .expect("mini model keeps variant cycle options");
    let mut variants = mini.variants.keys().map(String::as_str).collect::<Vec<_>>();
    variants.sort_unstable();
    assert_eq!(variants, vec!["high", "low", "medium", "xhigh"]);
    assert!(mini
        .variants
        .values()
        .all(|variant| variant.metadata.reasoning_effort.is_some()));
}
