use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use harness_core::config::{configured_model_catalog, load_resolved_config};

#[derive(Debug, Args, Clone, Default)]
pub struct ModelsCommand {}

pub fn execute(cmd: ModelsCommand, config_path: Option<PathBuf>) -> ExitCode {
    let _ = cmd;

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
