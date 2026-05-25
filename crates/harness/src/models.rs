use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::config::{configured_model_catalog, load_resolved_config_with_context};

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

pub fn execute_with_io(
    cmd: ModelsCommand,
    config_path: Option<PathBuf>,
    io: &mut crate::CliIo<'_>,
    deps: &crate::CliDeps,
) -> i32 {
    execute_with_writers(cmd, config_path, io.stdout, io.stderr, deps)
}

fn execute_with_writers(
    cmd: ModelsCommand,
    config_path: Option<PathBuf>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    deps: &crate::CliDeps,
) -> i32 {
    if let Some(command) = cmd.command {
        return match command {
            ModelsSubcommand::Generate(command) => {
                crate::model_probe::execute_generate_with_writers(command, stdout, stderr)
            }
            ModelsSubcommand::Generated(command) => {
                crate::model_probe::execute_generated_with_writers(command, stdout, stderr)
            }
            ModelsSubcommand::Probe(command) => {
                crate::model_probe::execute_probe_with_writers(command, stdout, stderr)
            }
        };
    }

    let config_context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(stderr, "failed to resolve config context: {err}");
            return 2;
        }
    };
    let Some(loaded) =
        (match load_resolved_config_with_context(config_path.as_deref(), &config_context) {
            Ok(loaded) => loaded,
            Err(err) => {
                let _ = writeln!(stderr, "failed to load config: {err}");
                return 1;
            }
        })
    else {
        let _ = writeln!(
            stderr,
            "models requires a config file; pass --config <path>, create ./harness.jsonc or ./harness.json, or create $XDG_CONFIG_HOME/harness/harness.jsonc or $XDG_CONFIG_HOME/harness/harness.json for shared defaults. A starting point lives at configs/harness.example.jsonc"
        );
        return 2;
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

        let _ = writeln!(stdout, "{}", segments.join(" | "));
    }

    0
}
