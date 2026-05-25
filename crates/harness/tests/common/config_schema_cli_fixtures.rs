use std::fs;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, load_config_from_file,
    load_config_from_str, AgentMode, OpenAiApiMode, PermissionMode, ProviderConfig,
    PublicRuntimeConfig, PublicTuiConfig,
};
use serde_json::Value;
use tempfile::tempdir;

#[path = "mod.rs"]
mod common;

use common::{repo_root, CliHarness, CliHarnessOutput};

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

struct HarnessCommand {
    inner: CliHarness,
}

impl HarnessCommand {
    fn arg(mut self, value: impl Into<OsString>) -> Self {
        self.inner = self.inner.args([value]);
        self
    }

    fn args<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.inner = self.inner.args(values);
        self
    }

    fn current_dir(mut self, current_dir: impl Into<PathBuf>) -> Self {
        self.inner = self.inner.current_dir(current_dir);
        self
    }

    fn env<K, V>(mut self, name: K, value: V) -> Self
    where
        K: AsRef<std::ffi::OsStr>,
        V: AsRef<std::ffi::OsStr>,
    {
        self.inner = self.inner.env(name, value);
        self
    }

    fn output(self) -> io::Result<CliHarnessOutput> {
        Ok(self.inner.output())
    }
}

fn with_env_var_state<T>(name: &str, value: Option<&str>, run: impl FnOnce(HarnessCommand) -> T) -> T {
    run(harness_command_with_env(name, value))
}

fn harness_command_with_env(name: &str, value: Option<&str>) -> HarnessCommand {
    let command = harness_command();
    match value {
        Some(value) => command.env(name, value),
        None => command.env_remove(name),
    }
}

impl HarnessCommand {
    fn env_remove<K>(mut self, name: K) -> Self
    where
        K: AsRef<std::ffi::OsStr>,
    {
        self.inner = self.inner.env_remove(name);
        self
    }
}

fn harness_command() -> HarnessCommand {
    HarnessCommand {
        inner: CliHarness::new()
            .env_remove("HARNESS_CONFIG")
            .env_remove("HARNESS_CONFIG_CONTENT")
            .env_remove("HARNESS_TUI_CONFIG")
            .env("HOME", "/nonexistent")
            .env("XDG_CONFIG_HOME", "/nonexistent"),
    }
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
