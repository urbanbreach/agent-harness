//! CLI entrypoint for running, replaying, validating, and launching the
//! interactive Agent Harness TUI.
//!
//! Keep subcommand wiring here and push domain invariants into `harness-core`
//! and TUI state/rendering contracts into `harness-tui`.

use std::{path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};
use harness_core::config::{
    harness_schema_pretty_json, load_config_from_file, profile_capability_notice_lines,
    resolve_config_path,
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
use bootstrap::ConfigInitTarget;
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
    Schema,
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    Validate,
    Init(ConfigInitCommand),
}

#[derive(Debug, Args, Clone, Default)]
struct ConfigInitCommand {
    #[arg(long, conflicts_with = "xdg")]
    path: Option<PathBuf>,

    #[arg(long, default_value_t = false, conflicts_with = "path")]
    xdg: bool,

    #[arg(long, default_value_t = false)]
    force: bool,
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
        Commands::Schema => match harness_schema_pretty_json() {
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
                let Some(config_path) = resolve_config_path(config.as_deref()) else {
                    eprintln!("{}", bootstrap::config_validate_guidance());
                    return ExitCode::from(2);
                };

                match load_config_from_file(&config_path) {
                    Ok(mut config) => {
                        config.apply_session_dir_override(session_dir);
                        println!("config valid: {}", config_path.display());
                        let notices = match profile_capability_notice_lines(&config) {
                            Ok(notices) => notices,
                            Err(err) => {
                                eprintln!("config validation failed: {err}");
                                return ExitCode::from(1);
                            }
                        };
                        for notice in notices {
                            println!("capability note: {notice}");
                        }
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("config validation failed: {err}");
                        ExitCode::from(1)
                    }
                }
            }
            ConfigCommands::Init(command) => {
                let target = match (command.path, command.xdg) {
                    (Some(path), false) => ConfigInitTarget::Explicit(path),
                    (None, true) => ConfigInitTarget::Xdg,
                    (None, false) => ConfigInitTarget::CurrentDir,
                    (Some(_), true) => unreachable!("clap enforces config init target exclusivity"),
                };

                match bootstrap::init_config(target, command.force) {
                    Ok(outcome) => {
                        println!("wrote config: {}", outcome.path.display());
                        println!("next steps:");
                        for step in bootstrap::config_init_next_steps(&outcome) {
                            println!("  - {step}");
                        }
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("config init failed: {err}");
                        ExitCode::from(1)
                    }
                }
            }
        },
    }
}

#[cfg(test)]
#[test]
fn startup_command_workflow_maps_model_and_session_intents_correctly() {
    tui::assert_startup_command_workflow_maps_model_and_session_intents_correctly();
}
