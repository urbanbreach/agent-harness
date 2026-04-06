use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use harness_core::config::{load_config_from_file, OpenAiApiMode, ProviderConfig};
use tempfile::tempdir;

fn openai_api_key_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_env_var_state<T>(name: &str, value: Option<&str>, run: impl FnOnce() -> T) -> T {
    let _lock = openai_api_key_env_lock()
        .lock()
        .expect("openai api key env lock poisoned");
    let previous = env::var_os(name);

    match value {
        Some(value) => env::set_var(name, value),
        None => env::remove_var(name),
    }

    let result = run();

    match previous {
        Some(value) => env::set_var(name, value),
        None => env::remove_var(name),
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

fn valid_config_value(label: &str, session_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini"
                    }
                }
            }
        },
        "profiles": {
            "deep": {
                "description": format!("{label} profile"),
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            },
            "shell_allowlist": {
                "executables": ["git"],
                "cwd_roots": ["."]
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

fn write_valid_config(path: &Path, label: &str, session_dir: &Path) {
    let config = valid_config_value(label, session_dir);
    write_config(path, &config);
}

fn rich_parity_config_value(session_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "gpt-5.4-mini": {
                        "display_name": "GPT-5.4 Mini",
                        "metadata": {
                            "family": "gpt-5",
                            "release_stage": "stable",
                            "context_window_tokens": 128000,
                            "supports_tool_calls": true,
                            "supports_reasoning_summaries": true
                        },
                        "max_input_tokens": 128000,
                        "max_output_tokens": 16384,
                        "variants": {
                            "deterministic": {
                                "display_name": "Deterministic",
                                "max_output_tokens": 4096,
                                "metadata": {
                                    "description": "Deterministic mode",
                                    "reasoning_effort": "minimal",
                                    "text_verbosity": "low",
                                    "recommended_for": "deep"
                                }
                            },
                            "creative": {
                                "display_name": "Creative",
                                "metadata": {
                                    "description": "Creative mode",
                                    "reasoning_effort": "high",
                                    "text_verbosity": "high",
                                    "recommended_for": "writer"
                                }
                            }
                        }
                    }
                }
            }
        },
        "profiles": {
            "deep": {
                "description": "Rich parity profile",
                "model_ref": "default:gpt-5.4-mini",
                "variant": "deterministic",
                "tools": []
            }
        },
        "hooks": {
            "lifecycle": [
                {
                    "event": "run_started",
                    "command": ["bash", "-lc", "printf 'run started\\n'"],
                    "timeout_ms": 4000,
                    "critical": false,
                    "env": {
                        "HARNESS_HOOK_SOURCE": "test"
                    }
                },
                {
                    "event": "tool_call_finished",
                    "command": ["bash", "-lc", "printf 'tool done\\n'"],
                    "timeout_ms": 4000,
                    "critical": false
                }
            ]
        },
        "skills": {
            "project_roots": [".opencode/skills", ".claude/skills", ".agents/skills"],
            "global_roots": ["~/.config/opencode/skills", "~/.claude/skills", "~/.agents/skills"],
            "walk_to_git_root": true,
            "permissions": {
                "*": "allow",
                "internal-*": "deny",
                "experimental-*": "ask"
            }
        },
        "lsp": {
            "disabled": false,
            "servers": {
                "rust": {
                    "disabled": false,
                    "command": ["rust-analyzer"],
                    "extensions": [".rs"],
                    "env": {
                        "RUST_LOG": "warn"
                    },
                    "initialization": {
                        "cargo": {
                            "allFeatures": true
                        }
                    }
                },
                "typescript": {
                    "disabled": false,
                    "command": ["typescript-language-server", "--stdio"],
                    "extensions": [".ts", ".tsx", ".js", ".jsx"],
                    "initialization": {
                        "preferences": {
                            "importModuleSpecifierPreference": "relative"
                        }
                    }
                }
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            },
            "shell_allowlist": {
                "executables": ["git"],
                "cwd_roots": ["."]
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
            "default_profile": "deep",
            "parity": {
                "variant_cycle_enabled": true,
                "child_session_navigation_enabled": true,
                "keybindings": {
                    "variant_cycle": "tab",
                    "session_child_first": "ctrl+]",
                    "session_child_cycle": "]",
                    "session_child_cycle_reverse": "[",
                    "session_parent": "ctrl+["
                }
            }
        }
    })
}

#[test]
fn schema_cli_prints_json_schema() {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("schema")
        .output()
        .expect("run harness schema");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(body.contains("\"type\": \"object\""));
    assert!(body.contains("\"profiles\"") || body.contains("\"runtime\""));
    assert!(body.contains("\"integrations\""));
    assert!(body.contains("\"hooks\""));
    assert!(body.contains("\"skills\""));
    assert!(body.contains("\"lsp\""));
    assert!(body.contains("\"variant_cycle_enabled\""));
}

#[test]
fn schema_cli_documents_current_integrations_boundary() {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("schema")
        .output()
        .expect("run harness schema");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let schema: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("schema output should be json");
    let integrations = &schema["definitions"]["IntegrationsConfig"];
    let integration_properties = integrations["properties"]
        .as_object()
        .expect("integrations properties object");
    assert_eq!(integration_properties.len(), 2);
    assert!(integration_properties.contains_key("mcp"));
    assert!(integration_properties.contains_key("remote_search"));

    let integrations_description = integrations["description"]
        .as_str()
        .expect("integrations description");
    assert!(integrations_description.contains("remote search transport"));
    assert!(integrations_description.contains("configured MCP servers"));

    let remote_search_description = schema["definitions"]["RemoteSearchConfig"]["description"]
        .as_str()
        .expect("remote search description");
    assert!(remote_search_description.contains("Exa-compatible MCP endpoint"));
    assert!(remote_search_description.contains("`web_search` and `code_search`"));
}

#[test]
fn config_validate_cli_reports_missing_config() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate without config");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(stderr.contains("configs/harness.example.jsonc"));
}

#[test]
fn config_validate_cli_accepts_shipped_example_config() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
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
fn shipped_example_config_resolves_loopback_placeholder_key_without_openai_api_key() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    with_env_var_state("OPENAI_API_KEY", None, || {
        let parsed = load_config_from_file(&config_path)
            .expect("shipped example config should resolve without OPENAI_API_KEY on loopback");
        let ProviderConfig::OpenAiCompatible(provider) = parsed
            .providers
            .get("default")
            .expect("default provider present in shipped example config");

        assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
        assert_eq!(provider.api_key, "sk-zerolimit");
        assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
    });
}

#[test]
fn shipped_example_config_uses_placeholder_key_when_openai_api_key_is_empty() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    with_env_var_state("OPENAI_API_KEY", Some(""), || {
        let parsed = load_config_from_file(&config_path)
            .expect("shipped example config should use fallback key when OPENAI_API_KEY is empty");
        let ProviderConfig::OpenAiCompatible(provider) = parsed
            .providers
            .get("default")
            .expect("default provider present in shipped example config");

        assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
        assert_eq!(provider.api_key, "sk-zerolimit");
        assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
    });
}

#[test]
fn config_validate_cli_accepts_valid_config_and_session_override() {
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let config_path = temp.path().join("harness.test.jsonc");
    let local_config_path = temp.path().join("harness.jsonc");
    let xdg_config_path = xdg_root.join("harness/config.jsonc");
    let session_dir = temp.path().join("override-sessions");
    write_valid_config(&config_path, "explicit", &temp.path().join("sessions"));
    write_valid_config(
        &local_config_path,
        "local",
        &temp.path().join("local-sessions"),
    );
    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
    write_valid_config(&xdg_config_path, "xdg", &temp.path().join("xdg-sessions"));

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with valid config");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(config_path.to_str().expect("config path utf-8")));
    assert!(!stdout.contains(local_config_path.to_str().expect("local config path utf-8")));
    assert!(!stdout.contains(xdg_config_path.to_str().expect("xdg config path utf-8")));
}

#[test]
fn config_validate_cli_accepts_rich_parity_config() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.rich.jsonc");
    let session_dir = temp.path().join("rich-sessions");
    let config = rich_parity_config_value(&temp.path().join("sessions"));
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with rich parity config");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(config_path.to_str().expect("config path utf-8")));
}

#[test]
fn config_validate_cli_accepts_gpt_5_4_mini_defaults() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.gpt-5.4-mini.jsonc");
    let session_dir = temp.path().join("gpt-5.4-mini-sessions");
    let config = rich_parity_config_value(&temp.path().join("sessions"));
    assert_eq!(
        config["profiles"]["deep"]["model_ref"],
        serde_json::json!("default:gpt-5.4-mini")
    );
    assert!(config["providers"]["default"]["models"]
        .get("gpt-5.4-mini")
        .is_some());
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with gpt-5.4-mini defaults");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(config_path.to_str().expect("config path utf-8")));
}

#[test]
fn config_validate_cli_accepts_custom_local_lsp_and_disabled_unreferenced_variant() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.custom-lsp.jsonc");
    let mut config = rich_parity_config_value(&temp.path().join("sessions"));
    config["providers"]["default"]["models"]["gpt-5.4-mini"]["variants"]["legacy"] = serde_json::json!({
        "display_name": "Legacy",
        "disabled": true,
        "metadata": {
            "description": "Retired fallback"
        }
    });
    config["skills"]["permissions"]["team-*"] = serde_json::json!("allow");
    config["skills"]["permissions"]["team-secret"] = serde_json::json!("ask");
    config["lsp"]["servers"]["custom-local"] = serde_json::json!({
        "disabled": false,
        "command": ["custom-local-lsp", "--stdio"],
        "extensions": [".foo", ".bar"],
        "env": {
            "CUSTOM_LSP_MODE": "enabled"
        },
        "initialization": {
            "feature": {
                "mode": "custom"
            }
        }
    });
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with custom local lsp");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(config_path.to_str().expect("config path utf-8")));
}

#[test]
fn config_validate_cli_does_not_merge_lower_precedence_files_into_explicit_config() {
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let config_path = temp.path().join("harness.test.jsonc");
    let local_config_path = temp.path().join("harness.jsonc");
    let xdg_config_path = xdg_root.join("harness/config.jsonc");

    write_valid_config(
        &local_config_path,
        "local",
        &temp.path().join("local-sessions"),
    );
    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
    write_valid_config(&xdg_config_path, "xdg", &temp.path().join("xdg-sessions"));

    let incomplete_config = serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "api_mode": "responses"
            }
        }
    });
    write_config(&config_path, &incomplete_config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with incomplete explicit config");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed:"));
    assert!(stderr.contains(
        "missing required config sections: integrations, permissions, profiles, runtime"
    ));
}

#[test]
fn config_validate_cli_rejects_retired_public_keys_with_migration_guidance() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let config = serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini"
                    }
                }
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
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
            "session_dir": ".agent-harness/sessions",
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        },
        "profiles": {},
        "categories": {},
        "backgroundTask": {
            "defaultConcurrency": 2
        },
        "paths": {
            "session_dir": ".agent-harness/sessions"
        },
        "deterministic": {
            "enabled": false,
            "seed": 42
        }
    });
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with retired keys");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed:"));
    assert!(stderr.contains("retired config keys detected:"));
    assert!(stderr.contains("top-level `categories` was retired; rename it to `profiles`"));
    assert!(stderr.contains(
        "top-level `backgroundTask` was retired; move its fields under `runtime.background_tasks`"
    ));
}

#[test]
fn config_validate_cli_rejects_retired_categories_with_migration_guidance() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = valid_config_value("legacy-categories", &temp.path().join("sessions"));
    config["categories"] = serde_json::json!({
        "deep": {
            "description": "Legacy deep profile",
            "model_ref": "default:gpt-4o-mini",
            "tools": []
        }
    });
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with retired categories");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed:"));
    assert!(stderr.contains("retired config keys detected:"));
    assert!(stderr.contains("top-level `categories` was retired; rename it to `profiles`"));
}

#[test]
fn config_validate_cli_rejects_unknown_top_level_keys() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = valid_config_value("unknown-key", &temp.path().join("sessions"));
    config["mystery"] = serde_json::json!({"enabled": true});
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with unknown key");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed: unknown top-level config keys:"));
    assert!(stderr.contains("`mystery`"));
    assert!(stderr.contains("expected only"));
}

#[test]
fn config_validate_cli_rejects_unknown_provider_reference() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = valid_config_value("invalid-provider", &temp.path().join("sessions"));
    config["profiles"]["deep"]["model_ref"] =
        serde_json::Value::String("missing:gpt-4o-mini".to_string());
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with invalid provider ref");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed:"));
    assert!(stderr.contains(
        "profile `deep` references unknown provider `missing` in `model_ref` `missing:gpt-4o-mini`"
    ));
}

#[test]
fn config_validate_cli_rejects_unknown_model_reference() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = valid_config_value("invalid-model", &temp.path().join("sessions"));
    config["profiles"]["deep"]["model_ref"] =
        serde_json::Value::String("default:gpt-4.1".to_string());
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with invalid model ref");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed:"));
    assert!(stderr.contains(
        "profile `deep` references unknown model `gpt-4.1` in `model_ref` `default:gpt-4.1`"
    ));
    assert!(stderr.contains("available models for provider `default`: gpt-4o-mini"));
}

#[test]
fn config_validate_cli_rejects_invalid_hook_or_variant_refs() {
    let temp = tempdir().expect("tempdir");

    let invalid_hook_path = temp.path().join("invalid-hook.jsonc");
    let mut invalid_hook = rich_parity_config_value(&temp.path().join("sessions"));
    invalid_hook["hooks"]["lifecycle"][0]["command"] = serde_json::json!([]);
    write_config(&invalid_hook_path, &invalid_hook);

    let hook_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            invalid_hook_path.to_str().expect("hook config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with invalid hook");

    assert!(!hook_output.status.success());
    let hook_stderr = String::from_utf8_lossy(&hook_output.stderr);
    assert!(hook_stderr.contains("config validation failed:"));
    assert!(hook_stderr.contains("hooks.lifecycle[0]"));
    assert!(hook_stderr.contains("must include at least one command token"));

    let invalid_variant_path = temp.path().join("invalid-variant.jsonc");
    let mut invalid_variant = rich_parity_config_value(&temp.path().join("sessions"));
    invalid_variant["profiles"]["deep"]["variant"] = serde_json::Value::String("ghost".to_string());
    write_config(&invalid_variant_path, &invalid_variant);

    let variant_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            invalid_variant_path
                .to_str()
                .expect("variant config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with invalid variant ref");

    assert!(!variant_output.status.success());
    let variant_stderr = String::from_utf8_lossy(&variant_output.stderr);
    assert!(variant_stderr.contains("config validation failed:"));
    assert!(variant_stderr.contains(
        "profile `deep` references unknown variant `ghost` for model `default:gpt-5.4-mini`"
    ));

    let invalid_skill_root_path = temp.path().join("invalid-skill-root.jsonc");
    let mut invalid_skill_root = rich_parity_config_value(&temp.path().join("sessions"));
    invalid_skill_root["skills"]["project_roots"] = serde_json::json!(["../skills"]);
    write_config(&invalid_skill_root_path, &invalid_skill_root);

    let skill_root_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            invalid_skill_root_path
                .to_str()
                .expect("skill config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with invalid skill root");

    assert!(!skill_root_output.status.success());
    let skill_root_stderr = String::from_utf8_lossy(&skill_root_output.stderr);
    assert!(skill_root_stderr.contains("config validation failed:"));
    assert!(skill_root_stderr.contains("skills.project_roots[0]"));
    assert!(skill_root_stderr.contains("must not contain `..`"));
}

#[test]
fn config_validate_cli_rejects_unknown_exit_target_profile() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = valid_config_value("invalid-exit-target", &temp.path().join("sessions"));
    config["profiles"]["deep"]["exit_target_profile"] =
        serde_json::Value::String("ops".to_string());
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with invalid exit target profile");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed:"));
    assert!(stderr.contains(
        "profile `deep` references unknown `exit_target_profile` `ops`; available profiles: deep"
    ));
}

#[test]
fn config_validate_cli_rejects_unknown_ui_default_profile() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = valid_config_value("invalid-default-profile", &temp.path().join("sessions"));
    config["ui"]["default_profile"] = serde_json::Value::String("ops".to_string());
    write_config(&config_path, &config);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with invalid ui default profile");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("config validation failed:"));
    assert!(stderr
        .contains("ui.default_profile references unknown profile `ops`; available profiles: deep"));
}
