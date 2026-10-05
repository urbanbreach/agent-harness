//! CLI adapters. The coordinator owns run state and all execution.
extern crate self as harness;
use clap::{CommandFactory, Parser, Subcommand};
use std::{ffi::OsString, io::IsTerminal, path::PathBuf, process::ExitCode};
mod archives;
mod auth;
mod bootstrap;
pub use auth::{execute_auth_backend_args, execute_auth_backend_args_with_io, AuthBackendOutput};
mod cli_config;
mod cli_io;
mod cli_labels;
mod config_commands;
mod defaults;
mod deps;
mod exports;
mod inspect;
mod logging;
mod models;
mod operations;
mod plugin;
mod prompt;
mod queue;
mod recovery;
mod replay;
mod runtime_catalog;
mod scenarios;
mod sessions;
pub mod tui;
mod update;
mod workspace;
mod worktrees;
pub use cli_io::CliIo;
pub use deps::CliDeps;
pub use harness_core::UnwrapOrAbort;
pub use tui::replay_workspace_root_from_events;

#[derive(Parser)]
#[command(
    name = "harness",
    version,
    about = "Launch the interactive harness UI or run subcommands"
)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    session_dir: Option<PathBuf>,
    #[arg(long, global = true, value_name = "DIR")]
    cwd: Option<PathBuf>,
    /// Enable debug logging.
    #[arg(long, global = true)]
    debug: bool,
    /// Write diagnostic logs to FILE.
    #[arg(long, global = true, value_name = "FILE")]
    debug_file: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    mock: bool,
    /// Automatically approve ordinary tool requests; remember this mode for the session.
    #[arg(long)]
    yolo: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Prompt(Box<prompt::PromptCommand>),
    /// Run a prompt, combining arguments, piped input and an optional prompt file.
    Run(Box<prompt::RunCommand>),
    Replay(replay::ReplayCommand),
    /// List, search, inspect and manage saved sessions.
    Sessions(sessions::SessionsCommand),
    /// Summarize saved sessions and local configuration.
    Dashboard(sessions::DashboardCommand),
    /// Export the visible session conversation as redacted Markdown.
    Export(exports::ExportCommand),
    /// Archive a redacted session diagnostic locally.
    Trace(archives::TraceCommand),
    /// Package workspace files, with optional redacted session diagnostics.
    Wrap(archives::WrapCommand),
    Tui(tui::TuiCommand),
    Auth(auth::AuthCommand),
    /// Check local configuration without contacting providers or creating files.
    Doctor(inspect::DoctorCommand),
    /// Validate configuration and inspect its values and source layers.
    Config(config_commands::ConfigCommand),
    /// Print the runtime or terminal configuration JSON schema.
    Schema {
        #[arg(long)]
        tui: bool,
    },
    /// List configured models, variants and token-limit provenance.
    Models(inspect::ModelsCommand),
    /// Inspect implemented provider protocols.
    Providers(inspect::ProvidersCommand),
    /// Read and write durable workspace memory.
    Memory(workspace::MemoryCommand),
    /// Build and query the local code index.
    CodeGraph(workspace::CodeGraphCommand),
    /// Inspect recorded agent edits and external changes.
    Attribution(workspace::AttributionCommand),
    /// Record due schedules at a supplied civil time.
    Cron(operations::CronCommand),
    /// Manage local teams and their durable mailboxes.
    Team(operations::TeamCommand),
    /// Launch a local stdio peer and report its transport exchange.
    Agent(operations::AgentCommand),
    /// List and remove managed session worktrees.
    Worktree(worktrees::WorktreeCommand),
    /// Inspect and update the durable session-local prompt queue.
    PromptQueue(queue::PromptQueueCommand),
    /// Manage local plugin packages and discover extension descriptors.
    Plugin(plugin::PluginCommand),
    /// Check a local update manifest, download, replace or restart the executable.
    Update(update::UpdateCommand),
    /// Generate shell completions.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitOutcome {
    pub code: i32,
}

pub fn run<I, T>(args: I, io: &mut CliIo<'_>, deps: CliDeps) -> ExitOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let writer = if error.use_stderr() {
                &mut io.stderr
            } else {
                &mut io.stdout
            };
            let written = write!(writer, "{error}");
            return ExitOutcome {
                code: if written.is_err() {
                    1
                } else {
                    error.exit_code()
                },
            };
        }
    };
    let deps = match cli.cwd {
        Some(path) => deps.with_current_dir(path),
        None => deps,
    };
    if cli.debug || cli.debug_file.is_some() {
        if let Err(error) = logging::init_debug_logging(cli.debug, cli.debug_file.as_deref()) {
            let _ = writeln!(io.stderr, "failed to initialize debug logging: {error}");
        }
    }
    let command = cli.command.unwrap_or({
        Commands::Tui(tui::TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: cli.mock,
            yolo: cli.yolo,
            deterministic: false,
            session_dir: None,
            exit_on_finish: false,
            profile: cli.profile.clone(),
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        })
    });
    let result = match command {
        Commands::Prompt(mut command) => {
            command.profile = command.profile.or(cli.profile);
            command.mock |= cli.mock;
            command.options.yolo |= cli.yolo;
            prompt::execute(*command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Run(mut command) => {
            command.profile = command.profile.or(cli.profile);
            command.mock |= cli.mock;
            command.options.yolo |= cli.yolo;
            prompt::execute_run(*command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Replay(command) => replay::execute(command, io),
        Commands::Sessions(command) => {
            sessions::execute(command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Dashboard(command) => {
            sessions::dashboard(command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Export(command) => {
            exports::markdown(command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Trace(command) => {
            archives::trace(command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Wrap(command) => {
            archives::wrap(command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Auth(command) => auth::execute(command, cli.config.as_deref(), io, &deps),
        Commands::Doctor(command) => inspect::doctor(command, cli.config.as_deref(), io, &deps),
        Commands::Config(command) => {
            config_commands::execute(command, cli.config.as_deref(), cli.session_dir, io, &deps)
        }
        Commands::Schema { tui } => config_commands::schema(tui, io),
        Commands::Models(command) => inspect::models(command, cli.config.as_deref(), io, &deps),
        Commands::Providers(command) => inspect::protocols(command, io),
        Commands::Memory(command) => workspace::memory(command, io, &deps),
        Commands::CodeGraph(command) => workspace::graph(command, io, &deps),
        Commands::Attribution(command) => {
            workspace::attribution(command, cli.config.as_deref(), io, &deps)
        }
        Commands::Cron(command) => operations::cron(command, io, &deps),
        Commands::Team(command) => operations::team(command, io, &deps),
        Commands::Agent(command) => operations::agent(command, cli.config.as_deref(), io, &deps),
        Commands::Worktree(command) => worktrees::execute(command, io, &deps),
        Commands::PromptQueue(command) => queue::execute(command, io, &deps),
        Commands::Plugin(command) => plugin::execute(command, io, &deps),
        Commands::Update(command) => update::execute(command, io, &deps),
        Commands::Completions { shell } => {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "harness", &mut script);
            io.stdout.write_all(&script).map_err(|e| e.to_string())
        }
        Commands::Tui(mut command) => {
            command.yolo |= cli.yolo;
            let code = tui::execute_with_io(
                command,
                cli.config,
                cli.session_dir,
                deps.config_load_context(),
                io.stderr,
            );
            return ExitOutcome {
                code: if code == ExitCode::SUCCESS {
                    0
                } else if code == ExitCode::from(2) {
                    2
                } else {
                    1
                },
            };
        }
    };
    ExitOutcome {
        code: match result {
            Ok(()) => 0,
            Err(error) => {
                let _ = writeln!(io.stderr, "error: {error}");
                1
            }
        },
    }
}

pub fn run_os() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "__eval-worker")
    {
        return harness_tools::eval_worker_main();
    }
    let _ = dotenvy::dotenv();
    let (stdin, stdout, stderr) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    let terminal = stdin.is_terminal();
    let (mut stdin, mut stdout, mut stderr) = (stdin.lock(), stdout, stderr);
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr).with_stdin_terminal(terminal);
    ExitCode::from(
        u8::try_from(run(std::env::args_os(), &mut io, CliDeps::real()).code).unwrap_or(1),
    )
}
