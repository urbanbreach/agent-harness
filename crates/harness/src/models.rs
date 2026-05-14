use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use harness_core::config::{configured_model_catalog, load_resolved_config};

use crate::model_probe::{GeneratedModelCatalogCommand, ModelGenerateCommand, ModelProbeCommand};

#[derive(Debug, Args, Clone, Default)]
pub struct ModelsCommand {
    #[command(subcommand)]
    command: Option<ModelsSubcommand>,
}

#[derive(Debug, Subcommand, Clone)]
enum ModelsSubcommand {
    /// Write the Pi-style generated provider catalog artifact.
    Generate(ModelGenerateCommand),
    /// Print the generated provider catalog embedded in this build.
    Generated(GeneratedModelCatalogCommand),
    /// Probe models.dev capability data and emit a harness provider catalog fragment.
    Probe(ModelProbeCommand),
}

pub fn execute(cmd: ModelsCommand, config_path: Option<PathBuf>) -> ExitCode {
    if let Some(command) = cmd.command {
        return match command {
            ModelsSubcommand::Generate(command) => crate::model_probe::execute_generate(command),
            ModelsSubcommand::Generated(command) => crate::model_probe::execute_generated(command),
            ModelsSubcommand::Probe(command) => crate::model_probe::execute(command),
        };
    }

    let Some(loaded) = (match load_resolved_config(config_path.as_deref()) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("failed to load config: {err}");
            return ExitCode::from(1);
        }
    }) else {
        eprintln!(
            "models requires a config file; pass --config <path>, create ./harness.jsonc or ./harness.json, or create $XDG_CONFIG_HOME/harness/harness.jsonc or $XDG_CONFIG_HOME/harness/harness.json for shared defaults. A starting point lives at configs/harness.example.jsonc"
        );
        return ExitCode::from(2);
    };

    let config = loaded.config;

    for entry in configured_model_catalog(&config) {
        let mut segments = vec![format!("{}:{}", entry.provider, entry.model)];

        if let Some(variant) = entry.variant.as_deref() {
            segments.push(format!("variant={variant}"));
        }
        segments.push(format!("label={}", entry.display_label));
        if let Some(reasoning_effort) = entry.reasoning_effort.as_deref() {
            segments.push(format!("reasoning={reasoning_effort}"));
        }
        if let Some(text_verbosity) = entry.text_verbosity.as_deref() {
            segments.push(format!("verbosity={text_verbosity}"));
        }
        if let Some(token_window_label) = entry.token_window_label.as_deref() {
            segments.push(format!("tokens={token_window_label}"));
        }

        println!("{}", segments.join(" | "));
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn execute_returns_error_when_config_is_missing() {
        let cmd = ModelsCommand { command: None };
        // We set HARNESS_CONFIG_CONTENT to empty or unset to make sure it tries to load from file
        std::env::remove_var("HARNESS_CONFIG_CONTENT");
        let exit_code = execute(cmd, Some(PathBuf::from("non_existent_config.jsonc")));
        // Since load_resolved_config with a missing explicit path might return an Err
        // if it exists, or return None if not found, we expect it to return 1 or 2.
        // If we provide an explicit path and it doesn't exist, load_resolved_config_from_paths
        // will return an error about file not found -> Err -> ExitCode::from(1).
        // Since std::process::ExitCode cannot be compared, we convert it indirectly or check format.
        assert_eq!(format!("{:?}", exit_code), "ExitCode(unix_exit_status(1))");
    }

    #[test]
    #[serial]
    fn execute_returns_success_with_valid_config() {
        let temp = tempdir().expect("failed to create temp dir");
        let config_path = temp.path().join("harness.jsonc");
        fs::write(
            &config_path,
            r#"{
                "providers": {
                    "test_provider": {
                        "type": "openai_compatible",
                        "options": { "baseURL": "http://localhost" },
                        "models": {
                            "test_model": {
                                "name": "Test Model",
                                "limit": {
                                    "context": 8192
                                },
                                "metadata": {
                                    "supportsToolCalls": true
                                },
                                "modalities": {
                                    "input": ["text"],
                                    "output": ["text"]
                                }
                            }
                        }
                    }
                },
                "model_profiles": {
                    "default": {
                        "model": "test_provider:test_model"
                    }
                },
                "agents": {
                    "build": {
                        "model_ref": "default",
                        "description": "test agent",
                        "system_prompt": "test prompt"
                    }
                }
            }"#
        ).expect("failed to write config");

        let cmd = ModelsCommand { command: None };
        std::env::remove_var("HARNESS_CONFIG_CONTENT");
        let exit_code = execute(cmd, Some(config_path));

        // When successful, it prints to stdout and returns ExitCode::SUCCESS
        assert_eq!(format!("{:?}", exit_code), "ExitCode(unix_exit_status(0))");
    }
}
