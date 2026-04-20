//! CLI entrypoint for running, replaying, validating, and launching the
//! interactive Agent Harness TUI.
//!
//! Keep subcommand wiring here and push domain invariants into `harness-core`
//! and TUI state/rendering contracts into `harness-tui`.

use std::{path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};
use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, load_resolved_config,
};

mod bootstrap;
mod logging;
mod models;
mod prompt;
mod recovery;
mod replay;
mod run;
mod scenarios;
mod sessions;
mod tui;

use crate::prompt::PromptCommand;
use crate::tui::TuiCommand;
use models::ModelsCommand;
use replay::ReplayCommand;
use run::RunCommand;
use sessions::SessionsCommand;

#[derive(Debug, Parser)]
#[command(name = "harness")]
#[command(about = "Launch the interactive harness UI or run subcommands", long_about = None)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[arg(long, global = true)]
    session_dir: Option<PathBuf>,

    #[command(flatten)]
    interactive: RootInteractiveArgs,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Args, Clone, Default)]
struct RootInteractiveArgs {
    #[arg(long)]
    profile: Option<String>,

    #[arg(long, default_value_t = false)]
    mock: bool,
}

impl RootInteractiveArgs {
    fn is_empty(&self) -> bool {
        self.profile.is_none() && !self.mock
    }

    fn into_tui_command(self) -> TuiCommand {
        TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: self.mock,
            deterministic: false,
            session_dir: None,
            exit_on_finish: false,
            profile: self.profile,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    Tui(TuiCommand),
    Run(RunCommand),
    Models(ModelsCommand),
    Prompt(PromptCommand),
    Replay(ReplayCommand),
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },
    Schema(SchemaCommand),
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Debug, Args, Clone, Default)]
struct SchemaCommand {
    #[arg(long, default_value_t = false)]
    tui: bool,
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    Validate,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let Cli {
        config,
        session_dir,
        interactive,
        command,
    } = cli;

    let Some(command) = command else {
        return crate::tui::execute(interactive.into_tui_command(), config, session_dir);
    };

    if !interactive.is_empty() {
        eprintln!(
            "root interactive flags (--profile, --mock) are only supported for bare `harness`"
        );
        return ExitCode::from(2);
    }

    match command {
        Commands::Tui(command) => crate::tui::execute(command, config, session_dir),
        Commands::Run(command) => run::execute(command, config, session_dir),
        Commands::Models(command) => models::execute(command, config),
        Commands::Prompt(command) => prompt::execute(command, config, session_dir),
        Commands::Replay(command) => replay::execute(command),
        Commands::Sessions { command } => sessions::execute(command, config, session_dir),
        Commands::Schema(command) => match if command.tui {
            harness_tui_schema_pretty_json()
        } else {
            harness_schema_pretty_json()
        } {
            Ok(schema) => {
                println!("{schema}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("schema generation failed: {err}");
                ExitCode::from(1)
            }
        },
        Commands::Config { command } => match command {
            ConfigCommands::Validate => {
                let Some(loaded) = (match load_resolved_config(config.as_deref()) {
                    Ok(loaded) => loaded,
                    Err(err) => {
                        eprintln!("config validation failed: {err}");
                        return ExitCode::from(1);
                    }
                }) else {
                    eprintln!(
        "no config file found; pass --config <path>, create ./harness.jsonc or ./harness.json, or create $XDG_CONFIG_HOME/harness/harness.jsonc or $XDG_CONFIG_HOME/harness/harness.json for shared defaults. A starting point lives at configs/harness.example.jsonc"
                    );
                    return ExitCode::from(2);
                };

                let path_display = loaded.path_display();
                let mut config = loaded.config;
                config.apply_session_dir_override(session_dir);
                println!("config valid: {path_display}");
                ExitCode::SUCCESS
            }
        },
    }
}

#[cfg(test)]
#[test]
fn startup_command_workflow_maps_model_and_session_intents_correctly() {
    tui::assert_startup_command_workflow_maps_model_and_session_intents_correctly();
}
