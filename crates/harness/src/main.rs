use std::{path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand};
use harness_core::config::{
    harness_schema_pretty_json, load_config_from_file, resolve_config_path,
};

mod bootstrap;
mod logging;
mod prompt;
mod replay;
mod run;
mod scenarios;
mod sessions;
mod tui;

use crate::prompt::PromptCommand;
use crate::tui::TuiCommand;
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
        Commands::Prompt(command) => prompt::execute(command, config, session_dir),
        Commands::Replay(command) => replay::execute(command),
        Commands::Sessions { command } => sessions::execute(command, session_dir),
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
                    eprintln!(
                        "no config file found; pass --config <path> or create ./harness.jsonc or $XDG_CONFIG_HOME/harness/config.jsonc"
                    );
                    return ExitCode::from(2);
                };

                match load_config_from_file(&config_path) {
                    Ok(mut config) => {
                        config.apply_session_dir_override(session_dir);
                        println!("config valid: {}", config_path.display());
                        ExitCode::SUCCESS
                    }
                    Err(err) => {
                        eprintln!("config validation failed: {err}");
                        ExitCode::from(1)
                    }
                }
            }
        },
    }
}
