use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use harness_core::config::{configured_model_catalog, load_config_from_file, resolve_config_path};

use crate::bootstrap;

#[derive(Debug, Args, Clone, Default)]
pub struct ModelsCommand {}

pub fn execute(cmd: ModelsCommand, config_path: Option<PathBuf>) -> ExitCode {
    let _ = cmd;

    let Some(config_path) = resolve_config_path(config_path.as_deref()) else {
        eprintln!("{}", bootstrap::models_config_guidance());
        return ExitCode::from(2);
    };

    let config = match load_config_from_file(&config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("failed to load config {}: {err}", config_path.display());
            return ExitCode::from(1);
        }
    };

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
