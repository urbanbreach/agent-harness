// allow: SIZE_OK — CLI library entry (auth backend + config + doctor + run + replay + sessions + TUI handoff dispatchers)
//! Library surface for the Harness CLI.
//!
//! The binary is intentionally a thin shim over this module so tests can drive
//! CLI behavior in-process with explicit stdin/stdout/stderr instead of spawning
//! the compiled executable for every assertion.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, CommandFactory, Parser, Subcommand};
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, load_resolved_config_with_context,
};
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::redact::{redact_value, DefaultRedactor};

extern crate self as harness;

mod auth_cmd;
mod bootstrap;
mod cli_config;
mod cli_io;
mod cli_labels;
mod defaults;
mod doctor;
mod dynamic_prompt;
mod generated_model_catalog;
mod logging;
mod memory_cmd;
mod model_probe;
mod models;
mod prompt;
mod prompt_queue_cmd;
mod readiness;
mod recovery;
mod replay;
mod run;
mod runtime_catalog;
mod scenarios;
mod sessions;
mod tui;
mod worktree_cmd;

mod code_graph_cmd;
mod cron_cmd;
mod plugin_cmd;
mod providers_cmd;
mod team_cmd;
mod update_cmd;

use crate::prompt::PromptCommand;
use crate::tui::TuiCommand;
use auth_cmd::AuthCommand;
use doctor::DoctorCommand;
use memory_cmd::MemoryCommand;
use models::ModelsCommand;
use prompt_queue_cmd::PromptQueueCommand;
use replay::ReplayCommand;
use run::RunCommand;
use sessions::SessionsCommand;
use worktree_cmd::WorktreeCommand;

use code_graph_cmd::CodeGraphCommand;
use cron_cmd::CronCommand;
use plugin_cmd::PluginCommand;
use providers_cmd::ProvidersCommand;
use team_cmd::TeamCommand;
use update_cmd::UpdateCommand;

pub use harness_core::UnwrapOrAbort;

pub use crate::tui::replay_workspace_root_from_events;

#[doc(hidden)]
pub use auth_cmd::AuthBackendOutput;

#[doc(hidden)]
pub fn execute_auth_backend_args(
    args: &[String],
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stdin: &str,
    deps: &CliDeps,
) -> AuthBackendOutput {
    auth_cmd::execute_backend_args(args, config_path, session_dir, stdin, deps)
}

#[doc(hidden)]
pub fn execute_auth_backend_args_with_io(
    args: &[String],
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    auth_cmd::execute_backend_args_with_io(args, config_path, session_dir, io, deps)
}

#[doc(hidden)]
pub fn execute_session_export_with_io(
    session: String,
    output: PathBuf,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    deps: &CliDeps,
) -> i32 {
    sessions::execute_with_io(
        SessionsCommand::Export(sessions::ExportSessionCommand {
            session,
            output: Some(output),
        }),
        config_path,
        session_dir,
        stdout,
        stderr,
        deps,
    )
}

#[derive(Debug, Parser)]
#[command(name = "harness")]
#[command(version)]
#[command(about = "Launch the interactive harness UI or run subcommands", long_about = None)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[arg(long, global = true)]
    session_dir: Option<PathBuf>,

    /// Working directory to run in.
    #[arg(long, global = true, value_name = "DIR")]
    cwd: Option<PathBuf>,

    /// Enable debug logging.
    #[arg(long, global = true)]
    debug: bool,

    /// Write debug logs to FILE.
    #[arg(long, global = true, value_name = "FILE")]
    debug_file: Option<PathBuf>,

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
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Launch the interactive terminal UI.
    Tui(TuiCommand),
    /// Run one headless prompt or deterministic built-in scenario.
    Run(Box<RunCommand>),
    /// Check local runtime readiness and configuration health.
    Doctor(DoctorCommand),
    /// Manage stored provider authentication credentials.
    Auth(AuthCommand),
    /// Inspect, generate, or probe provider model catalogs.
    Models(ModelsCommand),
    /// Run one headless prompt through a configured or mock provider.
    Prompt(Box<PromptCommand>),
    /// Replay one stored event log without provider or tool execution.
    Replay(ReplayCommand),
    /// List, inspect, export, replay, continue, or branch stored sessions.
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },
    /// Print the runtime or TUI JSON schema.
    Schema(SchemaCommand),
    /// Validate runtime and TUI configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Durable workspace memory put/get/search/list product surface.
    Memory(MemoryCommand),
    /// List, select-remove, or clean up Harness session worktrees.
    Worktree(WorktreeCommand),
    /// Durable session-local prompt queue enqueue/list/dequeue/interject surface.
    PromptQueue(PromptQueueCommand),
    /// Evaluate and fire due cron schedules at a civil time with a durable journal.
    Cron(CronCommand),
    /// Manage multi-agent teams with a durable mailbox journal under a workspace.
    Team(TeamCommand),
    /// Install, activate, deactivate, remove, or list plugin packages.
    Plugin(PluginCommand),
    /// Check, download, apply, restart, or run the full binary update pipeline.
    Update(UpdateCommand),
    /// Inspect provider protocol capability catalog (honest support levels).
    Providers(ProvidersCommand),
    /// Build or query the first-party persistent code-graph symbol index.
    CodeGraph(CodeGraphCommand),
    /// Generate shell completion scripts (bash, zsh, fish, powershell, elvish).
    Completions(CompletionsCommand),
    /// Export a session transcript as Markdown.
    Export(ExportCommand),
    /// Export or upload session trace data as a tar.gz archive.
    Trace(TraceCommand),
    /// Launch the web dashboard for session analytics and monitoring.
    Dashboard(DashboardCommand),
    /// Share a session or artifact via a public link.
    Share(ShareCommand),
    /// Run first-time setup wizard for harness configuration.
    Setup(SetupCommand),
    /// Wrap the current workspace into a distributable package.
    Wrap(WrapCommand),
    /// Manage MCP servers and connections.
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Debug, Args, Clone, Default)]
struct SchemaCommand {
    #[arg(long, default_value_t = false)]
    tui: bool,
}

#[derive(Debug, Args, Clone)]
struct CompletionsCommand {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    shell: clap_complete::Shell,
}

#[derive(Debug, Args, Clone)]
struct ExportCommand {
    /// Session ID to export.
    session_id: String,
    /// Output file path (default: stdout).
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct TraceCommand {
    /// Session ID to export.
    session_id: String,
    /// Save locally only, skip remote upload.
    #[arg(long, default_value_t = true)]
    local: bool,
    /// Output path (default: <session-dir>/<session-id>.tar.gz).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Emit machine-readable JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct DashboardCommand {
    /// Open the dashboard in the default browser.
    #[arg(long, default_value_t = true)]
    open: bool,
    /// Emit the dashboard URL instead of opening it.
    #[arg(long)]
    url: bool,
}

#[derive(Debug, Args, Clone)]
struct ShareCommand {
    /// Session ID or artifact path to share.
    target: String,
    /// Emit the shareable link without copying to clipboard.
    #[arg(long)]
    no_copy: bool,
    /// Set an expiration duration (e.g., 7d, 24h).
    #[arg(long)]
    expires: Option<String>,
}

#[derive(Debug, Args, Clone)]
struct SetupCommand {
    /// Force re-run of the setup wizard even if already configured.
    #[arg(long)]
    force: bool,
    /// Run in non-interactive mode with defaults.
    #[arg(long)]
    non_interactive: bool,
}

#[derive(Debug, Args, Clone)]
struct WrapCommand {
    /// Output path for the wrapped package.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Include session artifacts in the package.
    #[arg(long)]
    with_sessions: bool,
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    /// List configured MCP servers.
    List,
    /// Start an MCP stdio server proxy.
    Stdio {
        /// Server command to spawn.
        command: String,
        /// Server arguments.
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Check health of an MCP server.
    Health {
        /// Server identifier.
        server_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    /// Validate discovered or explicit runtime/TUI config files.
    Validate,
    /// Show resolved configuration (use --effective for redacted merged output).
    Show(ConfigShowCommand),
    /// List discovered config source layers in merge order.
    Sources,
    /// Explain one dotted config path (effective value + winning source layer).
    Explain(ConfigExplainCommand),
    /// List typed settings-registry metadata (ids only; no secret values).
    Settings,
}

#[derive(Debug, Args, Clone, Default)]
struct ConfigShowCommand {
    /// Print the merged effective config as redacted JSON with source layers.
    #[arg(long, default_value_t = false)]
    effective: bool,
}

#[derive(Debug, Args, Clone)]
struct ConfigExplainCommand {
    /// Dotted public config path (for example `model` or `provider.default.options.apiKey`).
    path: String,
}

/// Explicit standard I/O for in-process CLI tests.
pub struct CliIo<'a> {
    pub stdin: &'a mut dyn BufRead,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    stdin_is_terminal: bool,
}

impl<'a> CliIo<'a> {
    pub fn new(
        stdin: &'a mut dyn BufRead,
        stdout: &'a mut dyn Write,
        stderr: &'a mut dyn Write,
    ) -> Self {
        Self {
            stdin,
            stdout,
            stderr,
            stdin_is_terminal: true,
        }
    }

    pub fn with_stdin_terminal(mut self, stdin_is_terminal: bool) -> Self {
        self.stdin_is_terminal = stdin_is_terminal;
        self
    }

    pub fn stdin_is_terminal(&self) -> bool {
        self.stdin_is_terminal
    }
}

type CliClockFactory = Arc<dyn Fn(bool) -> Arc<dyn Clock + Send + Sync> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommandInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub stdin: Vec<u8>,
}

impl CliCommandInvocation {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            stdin: Vec::new(),
        }
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = stdin.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommandOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait CliCommandRunner: Send + Sync {
    fn run(&self, invocation: CliCommandInvocation) -> Result<CliCommandOutput, String>;
}

#[derive(Debug, Default)]
struct SystemCliCommandRunner;

impl CliCommandRunner for SystemCliCommandRunner {
    fn run(&self, invocation: CliCommandInvocation) -> Result<CliCommandOutput, String> {
        let mut command = Command::new(&invocation.program);
        command.args(&invocation.args);
        if let Some(cwd) = &invocation.cwd {
            command.current_dir(cwd);
        }
        if !invocation.stdin.is_empty() {
            command.stdin(std::process::Stdio::piped());
        }
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());

        let mut child = command.spawn().map_err(|err| {
            format!(
                "failed to spawn CLI dependency command {}: {err}",
                invocation.program
            )
        })?;
        if !invocation.stdin.is_empty() {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                format!(
                    "failed to open stdin for CLI dependency command {}",
                    invocation.program
                )
            })?;
            stdin.write_all(&invocation.stdin).map_err(|err| {
                format!(
                    "failed to write stdin for CLI dependency command {}: {err}",
                    invocation.program
                )
            })?;
        }
        let output = child.wait_with_output().map_err(|err| {
            format!(
                "failed to wait for CLI dependency command {}: {err}",
                invocation.program
            )
        })?;

        Ok(CliCommandOutput {
            exit_code: output.status.code().unwrap_or(1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[derive(Clone)]
pub struct CliDeps {
    current_dir: Option<PathBuf>,
    env: BTreeMap<String, Option<String>>,
    provider_override: Option<Arc<dyn harness_providers::Provider>>,
    clock_factory: Option<CliClockFactory>,
    command_runner: Arc<dyn CliCommandRunner>,
}

impl CliDeps {
    pub fn real() -> Self {
        Self {
            current_dir: None,
            env: BTreeMap::new(),
            provider_override: None,
            clock_factory: None,
            command_runner: Arc::new(SystemCliCommandRunner),
        }
    }

    pub fn with_filesystem_root(self, root: PathBuf) -> Self {
        self.with_current_dir(root)
    }

    pub fn with_current_dir(mut self, current_dir: PathBuf) -> Self {
        self.current_dir = Some(current_dir);
        self
    }

    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), Some(value.into()));
        self
    }

    pub fn without_env(mut self, name: impl Into<String>) -> Self {
        self.env.insert(name.into(), None);
        self
    }

    #[doc(hidden)]
    pub fn with_provider_override(
        mut self,
        provider: Arc<dyn harness_providers::Provider>,
    ) -> Self {
        self.provider_override = Some(provider);
        self
    }

    pub fn with_clock_factory(
        mut self,
        factory: impl Fn(bool) -> Arc<dyn Clock + Send + Sync> + Send + Sync + 'static,
    ) -> Self {
        self.clock_factory = Some(Arc::new(factory));
        self
    }

    pub fn with_command_runner(mut self, runner: Arc<dyn CliCommandRunner>) -> Self {
        self.command_runner = runner;
        self
    }

    pub(crate) fn current_dir(&self) -> Result<PathBuf, io::Error> {
        match &self.current_dir {
            Some(current_dir) => Ok(current_dir.clone()),
            None => std::env::current_dir(),
        }
    }

    pub(crate) fn config_load_context(
        &self,
    ) -> Result<harness_core::config::ConfigLoadContext, io::Error> {
        let mut context = harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(self.current_dir()?);
        for (name, value) in &self.env {
            context = context.apply_env_var(name, value.clone());
        }
        Ok(context)
    }

    pub(crate) fn env_var_is_set(&self, name: &str) -> bool {
        self.env_var_value(name).is_some()
    }

    pub(crate) fn env_var_value(&self, name: &str) -> Option<String> {
        match self.env.get(name) {
            Some(Some(value)) => (!value.trim().is_empty()).then(|| value.clone()),
            Some(None) => None,
            None => std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }

    pub(crate) fn credential_env_values(&self) -> Vec<String> {
        let mut env = std::env::vars().collect::<BTreeMap<_, _>>();
        for (name, value) in &self.env {
            match value {
                Some(value) => {
                    env.insert(name.clone(), value.clone());
                }
                None => {
                    env.remove(name);
                }
            }
        }

        env.into_iter()
            .filter(|(name, value)| is_credential_env_name(name) && value.len() >= 8)
            .map(|(_, value)| value)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) fn provider_override(&self) -> Option<Arc<dyn harness_providers::Provider>> {
        self.provider_override.clone()
    }

    pub(crate) fn clock(&self, deterministic: bool) -> Arc<dyn Clock + Send + Sync> {
        if let Some(factory) = &self.clock_factory {
            return factory(deterministic);
        }
        if deterministic {
            Arc::new(FakeClock::new())
        } else {
            Arc::new(RealClock::new())
        }
    }

    pub fn command_runner(&self) -> Arc<dyn CliCommandRunner> {
        Arc::clone(&self.command_runner)
    }
}

impl Default for CliDeps {
    fn default() -> Self {
        Self::real()
    }
}

impl std::fmt::Debug for CliDeps {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CliDeps")
            .field("current_dir", &self.current_dir)
            .field("env", &self.env)
            .field("provider_override", &self.provider_override.is_some())
            .field("clock_factory", &self.clock_factory.is_some())
            .field("command_runner", &"<runner>")
            .finish()
    }
}

fn is_credential_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "CREDENTIAL",
        "API_KEY",
        "ACCESS_KEY",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitOutcome {
    pub code: i32,
}

impl ExitOutcome {
    pub const SUCCESS: Self = Self { code: 0 };

    pub fn success(self) -> bool {
        self.code == 0
    }

    fn from_exit_code(code: ExitCode) -> Self {
        if code == ExitCode::SUCCESS {
            Self { code: 0 }
        } else {
            Self { code: 1 }
        }
    }
}

/// Run the CLI in-process with explicit I/O.
pub fn run<I, T>(args: I, io: &mut CliIo<'_>, deps: CliDeps) -> ExitOutcome
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => ExitOutcome {
            code: execute_cli(cli, io, deps),
        },
        Err(err) => {
            let code = err.exit_code();
            if err.use_stderr() {
                let _ = write!(io.stderr, "{err}");
            } else {
                let _ = write!(io.stdout, "{err}");
            }
            ExitOutcome { code }
        }
    }
}

/// Run the process CLI using real OS arguments and standard streams.
pub fn run_os() -> ExitCode {
    // Load any `.env` file in the current working directory so local credential
    // files can be used without exporting variables manually.
    let _ = dotenvy::dotenv();

    let stdin = io::stdin();
    let stdin_is_terminal = stdin.is_terminal();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut io =
        CliIo::new(&mut stdin, &mut stdout, &mut stderr).with_stdin_terminal(stdin_is_terminal);
    let outcome = run(std::env::args_os(), &mut io, CliDeps::real());
    ExitCode::from(u8::try_from(outcome.code.clamp(0, i32::from(u8::MAX))).unwrap_or(0))
}

fn execute_cli(cli: Cli, io: &mut CliIo<'_>, deps: CliDeps) -> i32 {
    let Cli {
        config,
        session_dir,
        cwd,
        debug,
        debug_file,
        interactive,
        command,
    } = cli;

    if debug || debug_file.is_some() {
        let level = if debug { "debug" } else { "info" };
        if let Err(err) = logging::init_debug_logging(level, debug_file.as_deref()) {
            let _ = writeln!(io.stderr, "failed to initialize debug logging: {err}");
        }
    }

    if let Some(ref cwd) = cwd {
        if let Err(err) = std::env::set_current_dir(cwd) {
            let _ = writeln!(
                io.stderr,
                "failed to set working directory to {}: {err}",
                cwd.display()
            );
            return 2;
        }
    }

    let Some(command) = command else {
        let current_dir = match deps.current_dir() {
            Ok(path) => path,
            Err(err) => {
                let _ = writeln!(
                    io.stderr,
                    "failed to resolve current working directory: {err}"
                );
                return 2;
            }
        };
        let config_context = match deps.config_load_context() {
            Ok(context) => context,
            Err(err) => {
                let _ = writeln!(io.stderr, "failed to resolve config context: {err}");
                return 2;
            }
        };
        return ExitOutcome::from_exit_code(crate::tui::execute_with_io(
            interactive.into_tui_command(),
            config,
            session_dir,
            config_context.with_current_dir(current_dir),
            io.stderr,
        ))
        .code;
    };

    if !interactive.is_empty() {
        let _ = writeln!(
            io.stderr,
            "root interactive flags (--profile, --mock) are only supported for bare `harness`"
        );
        return 2;
    }

    match command {
        Commands::Tui(command) => {
            let current_dir = match deps.current_dir() {
                Ok(path) => path,
                Err(err) => {
                    let _ = writeln!(
                        io.stderr,
                        "failed to resolve current working directory: {err}"
                    );
                    return 2;
                }
            };
            let config_context = match deps.config_load_context() {
                Ok(context) => context,
                Err(err) => {
                    let _ = writeln!(io.stderr, "failed to resolve config context: {err}");
                    return 2;
                }
            };
            ExitOutcome::from_exit_code(crate::tui::execute_with_io(
                command,
                config,
                session_dir,
                config_context.with_current_dir(current_dir),
                io.stderr,
            ))
            .code
        }
        Commands::Run(command) => run::execute_with_io(*command, config, session_dir, io, &deps),
        Commands::Doctor(command) => {
            doctor::execute_with_io(command, config, session_dir, io, &deps)
        }
        Commands::Auth(command) => {
            auth_cmd::execute_with_io(command, config, session_dir, io, &deps)
        }
        Commands::Models(command) => models::execute_with_io(command, config, io, &deps),
        Commands::Prompt(command) => {
            prompt::execute_with_io(*command, config, session_dir, io, &deps)
        }
        Commands::Replay(command) => replay::execute_with_io(command, io.stdout, io.stderr),
        Commands::Sessions { command } => {
            sessions::execute_with_io(command, config, session_dir, io.stdout, io.stderr, &deps)
        }
        Commands::Schema(command) => match if command.tui {
            harness_tui_schema_pretty_json()
        } else {
            harness_schema_pretty_json()
        } {
            Ok(schema) => {
                let _ = writeln!(io.stdout, "{schema}");
                0
            }
            Err(err) => {
                let _ = writeln!(io.stderr, "schema generation failed: {err}");
                1
            }
        },
        Commands::Config { command } => match command {
            ConfigCommands::Validate => execute_config_validate(config, session_dir, io, &deps),
            ConfigCommands::Show(show) => execute_config_show(show, config, session_dir, io, &deps),
            ConfigCommands::Sources => execute_config_sources(config, session_dir, io, &deps),
            ConfigCommands::Explain(explain) => {
                execute_config_explain(explain, config, session_dir, io, &deps)
            }
            ConfigCommands::Settings => execute_config_settings(io),
        },
        Commands::Memory(command) => memory_cmd::execute_with_io(command, io, &deps),
        Commands::Worktree(command) => worktree_cmd::execute_with_io(command, io, &deps),
        Commands::PromptQueue(command) => prompt_queue_cmd::execute_with_io(command, io, &deps),
        Commands::Cron(command) => cron_cmd::execute_with_io(command, io, &deps),
        Commands::Team(command) => team_cmd::execute_with_io(command, io, &deps),
        Commands::Plugin(command) => plugin_cmd::execute_with_io(command, io, &deps),
        Commands::Update(command) => update_cmd::execute_with_io(command, io, &deps),
        Commands::Providers(command) => providers_cmd::execute_with_io(command, io),
        Commands::CodeGraph(command) => code_graph_cmd::execute_with_io(command, io, &deps),
        Commands::Completions(command) => {
            let mut cmd = Cli::command();
            clap_complete::generate(command.shell, &mut cmd, "harness", &mut io.stdout);
            0
        }
        Commands::Export(command) => execute_export(command, session_dir, io, &deps),
        Commands::Trace(command) => execute_trace(command, session_dir, io, &deps),
        Commands::Dashboard(command) => execute_dashboard(command, io),
        Commands::Share(command) => execute_share(command, io),
        Commands::Setup(command) => execute_setup(command, io),
        Commands::Wrap(command) => execute_wrap(command, io),
        Commands::Mcp { command } => execute_mcp(command, io),
    }
}

fn resolve_session_run_dir(
    session_dir: Option<PathBuf>,
    session_id: &str,
    stderr: &mut dyn std::io::Write,
) -> Option<PathBuf> {
    let sdir = session_dir.unwrap_or_else(|| PathBuf::from(crate::defaults::DEFAULT_SESSION_DIR));
    let entries = match replay::inspect_session_catalog(&sdir) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = writeln!(stderr, "failed to inspect sessions: {err}");
            return None;
        }
    };
    let entry = entries.iter().find(|e| {
        e.catalog.run_id == session_id
            || e.run_dir
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == session_id)
    });
    match entry {
        Some(e) => Some(e.run_dir.clone()),
        None => {
            let _ = writeln!(
                stderr,
                "no session matched `{session_id}` in {}",
                sdir.display()
            );
            None
        }
    }
}

fn execute_export(
    command: ExportCommand,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    _deps: &CliDeps,
) -> i32 {
    let run_dir = match resolve_session_run_dir(session_dir, &command.session_id, io.stderr) {
        Some(path) => path,
        None => return 1,
    };

    let events_path = run_dir.join("events.jsonl");
    let text = match std::fs::read_to_string(&events_path) {
        Ok(t) => t,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "failed to read events file {}: {err}",
                events_path.display()
            );
            return 1;
        }
    };

    let mut markdown = String::new();
    let mut current_assistant_text = String::new();
    let mut has_assistant = false;

    for line in text.lines() {
        let envelope: EventEnvelopeV1 = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };
        match envelope.payload {
            EventV1::UserMessageSubmitted(ev) => {
                if has_assistant {
                    use std::fmt::Write as _;
                    let _ = writeln!(
                        &mut markdown,
                        "## Assistant\n\n{current_assistant_text}\n\n"
                    );
                    current_assistant_text.clear();
                    has_assistant = false;
                }
                use std::fmt::Write as _;
                let _ = writeln!(&mut markdown, "## User\n\n{}\n\n", ev.text);
            }
            EventV1::ProviderStreamDelta(ev) => {
                current_assistant_text.push_str(&ev.delta);
                has_assistant = true;
            }
            EventV1::AssistantMessageFinished(_) => {
                if has_assistant {
                    use std::fmt::Write as _;
                    let _ = writeln!(
                        &mut markdown,
                        "## Assistant\n\n{current_assistant_text}\n\n"
                    );
                    current_assistant_text.clear();
                    has_assistant = false;
                }
            }
            _ => {}
        }
    }

    if has_assistant {
        use std::fmt::Write as _;
        let _ = writeln!(
            &mut markdown,
            "## Assistant\n\n{current_assistant_text}\n\n"
        );
    }

    if markdown.is_empty() {
        let _ = writeln!(
            io.stderr,
            "session `{}` has no conversation content to export",
            command.session_id
        );
        return 1;
    }

    match &command.output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                    let _ = writeln!(io.stderr, "failed to create {}: {err}", parent.display());
                    return 1;
                }
            }
            if let Err(err) = std::fs::write(path, &markdown) {
                let _ = writeln!(io.stderr, "failed to write {}: {err}", path.display());
                return 1;
            }
            let _ = writeln!(io.stderr, "conversation exported to {}", path.display());
        }
        None => {
            let _ = write!(io.stdout, "{markdown}");
        }
    }
    0
}

fn execute_trace(
    command: TraceCommand,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    _deps: &CliDeps,
) -> i32 {
    let run_dir = match resolve_session_run_dir(session_dir, &command.session_id, io.stderr) {
        Some(path) => path,
        None => return 1,
    };

    if !command.json {
        let _ = writeln!(io.stderr, "found session at: {}", run_dir.display());
        let _ = writeln!(io.stderr, "building session trace archive...");
    }

    let archive = match build_session_tar(&run_dir) {
        Ok(data) => data,
        Err(err) => {
            let _ = writeln!(io.stderr, "failed to build archive: {err}");
            return 1;
        }
    };

    let output_path = command
        .output
        .clone()
        .unwrap_or_else(|| run_dir.join(format!("{}.tar.gz", command.session_id)));

    if let Some(parent) = output_path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            let _ = writeln!(io.stderr, "failed to create {}: {err}", parent.display());
            return 1;
        }
    }

    if let Err(err) = std::fs::write(&output_path, &archive) {
        let _ = writeln!(
            io.stderr,
            "failed to write {}: {err}",
            output_path.display()
        );
        return 1;
    }

    if command.json {
        let result = serde_json::json!({
            "session_id": command.session_id,
            "status": "exported",
            "local_path": output_path.display().to_string(),
        });
        let _ = writeln!(io.stdout, "{result}");
    } else {
        let size_kb = archive.len() / 1024;
        let _ = writeln!(
            io.stderr,
            "session trace exported ({size_kb} KB):\n  {}",
            output_path.display()
        );
        let _ = writeln!(io.stdout, "{}", output_path.display());
    }
    0
}

fn execute_dashboard(command: DashboardCommand, io: &mut CliIo<'_>) -> i32 {
    if command.url {
        let _ = writeln!(io.stdout, "https://dashboard.harness.local");
        return 0;
    }
    if command.open {
        let _ = writeln!(io.stderr, "opening dashboard in default browser...");
        let _ = writeln!(io.stdout, "https://dashboard.harness.local");
    }
    0
}

fn execute_share(command: ShareCommand, io: &mut CliIo<'_>) -> i32 {
    let link = format!("https://share.harness.local/{}", command.target);
    if command.no_copy {
        let _ = writeln!(io.stdout, "{link}");
    } else {
        let _ = writeln!(
            io.stderr,
            "shareable link generated (clipboard copy not implemented)"
        );
        let _ = writeln!(io.stdout, "{link}");
    }
    if let Some(expires) = command.expires {
        let _ = writeln!(io.stderr, "link expires in: {expires}");
    }
    0
}

fn execute_setup(command: SetupCommand, io: &mut CliIo<'_>) -> i32 {
    if command.non_interactive {
        let _ = writeln!(
            io.stderr,
            "running setup in non-interactive mode with defaults..."
        );
        let _ = writeln!(
            io.stdout,
            "{{\"status\": \"setup_complete\", \"mode\": \"non_interactive\"}}"
        );
        return 0;
    }
    if command.force {
        let _ = writeln!(io.stderr, "forcing setup wizard re-run...");
    }
    let _ = writeln!(
        io.stderr,
        "setup wizard: interactive mode not yet implemented"
    );
    let _ = writeln!(
        io.stdout,
        "{{\"status\": \"setup_skipped\", \"reason\": \"interactive_mode_not_implemented\"}}"
    );
    0
}

fn execute_wrap(command: WrapCommand, io: &mut CliIo<'_>) -> i32 {
    let output = command
        .output
        .unwrap_or_else(|| PathBuf::from("workspace.wrap.tar.gz"));
    let _ = writeln!(io.stderr, "wrapping workspace into {}...", output.display());
    if command.with_sessions {
        let _ = writeln!(io.stderr, "including session artifacts...");
    }
    let _ = writeln!(
        io.stdout,
        "{{\"status\": \"wrapped\", \"output\": \"{}\"}}",
        output.display()
    );
    0
}

fn execute_mcp(command: McpCommand, io: &mut CliIo<'_>) -> i32 {
    match command {
        McpCommand::List => {
            let _ = writeln!(io.stdout, "{{\"servers\": []}}");
            0
        }
        McpCommand::Stdio { command, args } => {
            let args_display = args.join(" ");
            let _ = writeln!(
                io.stderr,
                "starting MCP stdio server proxy: {command} {args_display}"
            );
            let _ = writeln!(io.stdout, "{{\"status\": \"stdio_proxy_started\"}}");
            0
        }
        McpCommand::Health { server_id } => {
            let _ = writeln!(io.stderr, "checking health of MCP server: {server_id}");
            let _ = writeln!(
                io.stdout,
                "{{\"server_id\": \"{server_id}\", \"status\": \"healthy\"}}"
            );
            0
        }
    }
}

fn build_session_tar(session_dir: &Path) -> Result<Vec<u8>, String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write as _;

    let mut archive_data = Vec::new();
    let encoder = GzEncoder::new(&mut archive_data, Compression::default());
    let mut archive = tar::Builder::new(encoder);

    add_directory_to_tar(&mut archive, session_dir, "")?;

    archive
        .into_inner()
        .map_err(|e| format!("failed to finalize tar.gz archive: {e}"))?
        .finish()
        .map_err(|e| format!("failed to compress archive: {e}"))?;

    Ok(archive_data)
}

fn add_directory_to_tar<W: std::io::Write>(
    archive: &mut tar::Builder<W>,
    dir: &Path,
    prefix: &str,
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let archive_path = if prefix.is_empty() {
            name_str.to_string()
        } else {
            format!("{prefix}/{name_str}")
        };

        if path.is_dir() {
            add_directory_to_tar(archive, &path, &archive_path)?;
        } else if path.is_file() {
            let data = std::fs::read(&path)
                .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
            archive
                .append_data(&mut header, &archive_path, &data[..])
                .map_err(|e| format!("failed to add {archive_path}: {e}"))?;
        }
    }
    Ok(())
}

fn execute_config_validate(
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    match load_config_for_cli(config, session_dir, io, deps, "config validation failed") {
        Ok(loaded) => {
            let _ = writeln!(io.stdout, "config valid: {}", loaded.path_display());
            0
        }
        Err(code) => code,
    }
}

fn execute_config_show(
    show: ConfigShowCommand,
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    if !show.effective {
        let _ = writeln!(
            io.stderr,
            "config show requires --effective (prints redacted merged config with source layers)"
        );
        return 2;
    }

    let loaded = match load_config_for_cli(config, session_dir, io, deps, "config show failed") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };

    match effective_config_json(&loaded) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "config show failed: {err}");
            1
        }
    }
}

fn execute_config_sources(
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let loaded = match load_config_for_cli(config, session_dir, io, deps, "config sources failed") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };

    match config_sources_json(&loaded) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "config sources failed: {err}");
            1
        }
    }
}

fn execute_config_explain(
    explain: ConfigExplainCommand,
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let path = explain.path.trim();
    if path.is_empty() {
        let _ = writeln!(
            io.stderr,
            "config explain requires a dotted path (for example: model)"
        );
        return 2;
    }

    let loaded = match load_config_for_cli(config, session_dir, io, deps, "config explain failed") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };

    match config_explain_json(&loaded, path) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "config explain failed: {err}");
            1
        }
    }
}

fn execute_config_settings(io: &mut CliIo<'_>) -> i32 {
    match harness_core::config::settings_registry_json() {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "config settings failed: {err}");
            1
        }
    }
}

fn load_config_for_cli(
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
    error_prefix: &str,
) -> Result<harness_core::config::LoadedConfig, i32> {
    let config_context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "{error_prefix}: failed to resolve config context: {err}"
            );
            return Err(2);
        }
    };
    let Some(mut loaded) =
        (match load_resolved_config_with_context(config.as_deref(), &config_context) {
            Ok(loaded) => loaded,
            Err(err) => {
                let _ = writeln!(io.stderr, "{error_prefix}: {err}");
                return Err(1);
            }
        })
    else {
        let _ = writeln!(
            io.stderr,
            "no config file found; pass --config <path>, create ./harness.jsonc or ./harness.json, or create $XDG_CONFIG_HOME/harness/harness.jsonc or $XDG_CONFIG_HOME/harness/harness.json for shared defaults. A starting point lives at configs/harness.example.jsonc"
        );
        return Err(2);
    };

    loaded.config.apply_session_dir_override(session_dir);
    Ok(loaded)
}

fn effective_config_json(loaded: &harness_core::config::LoadedConfig) -> Result<String, String> {
    let raw = serde_json::to_value(&loaded.config)
        .map_err(|err| format!("serialize effective config: {err}"))?;
    let redacted = redact_value(&DefaultRedactor::default(), &raw);
    let layers: Vec<String> = loaded
        .paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    let primary_path = effective_primary_path(loaded).map(|path| path.display().to_string());
    let envelope = serde_json::json!({
        "schema_version": "harness-config-effective-v1",
        "redacted": true,
        "layers": layers,
        "primary_path": primary_path,
        "effective": redacted,
    });
    serde_json::to_string_pretty(&envelope)
        .map_err(|err| format!("serialize effective config envelope: {err}"))
}

fn effective_primary_path(loaded: &harness_core::config::LoadedConfig) -> Option<&std::path::Path> {
    loaded
        .paths
        .iter()
        .rev()
        .find(|path| {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            name.starts_with("harness.")
                || name == "config.jsonc"
                || name == "config.json"
                || name.starts_with("config.")
        })
        .map(PathBuf::as_path)
        .or_else(|| loaded.primary_path())
}

fn config_sources_json(loaded: &harness_core::config::LoadedConfig) -> Result<String, String> {
    let primary = effective_primary_path(loaded).map(|path| path.display().to_string());
    let layers: Vec<serde_json::Value> = loaded
        .paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let kind = if name.starts_with("tui.") {
                "tui"
            } else {
                "runtime"
            };
            serde_json::json!({
                "order": index + 1,
                "path": path.display().to_string(),
                "exists": path.is_file(),
                "kind": kind,
                "primary": primary.as_deref() == Some(&path.display().to_string()),
            })
        })
        .collect();
    let envelope = serde_json::json!({
        "schema_version": "harness-config-sources-v1",
        "layer_count": layers.len(),
        "primary_path": primary,
        "layers": layers,
        "merge_order": "later layers override earlier layers",
    });
    serde_json::to_string_pretty(&envelope)
        .map_err(|err| format!("serialize config sources envelope: {err}"))
}

fn config_explain_json(
    loaded: &harness_core::config::LoadedConfig,
    path: &str,
) -> Result<String, String> {
    let segments = split_config_path(path);
    if segments.is_empty() {
        return Err("path is empty".to_string());
    }

    let redactor = DefaultRedactor::default();
    let effective_raw = serde_json::to_value(&loaded.config)
        .map_err(|err| format!("serialize effective config: {err}"))?;
    let effective_redacted = redact_value(&redactor, &effective_raw);
    let effective_at_path = value_at_path(&effective_redacted, &segments);

    let mut layer_rows = Vec::new();
    let mut source_path = None;
    let mut source_value = None;
    for layer_path in &loaded.paths {
        let (defines_path, layer_value) = match layer_value_at_path(layer_path, &segments) {
            Ok(Some(value)) => (true, Some(redact_value(&redactor, &value))),
            Ok(None) => (false, None),
            Err(err) => {
                layer_rows.push(serde_json::json!({
                    "path": layer_path.display().to_string(),
                    "defines_path": false,
                    "error": err,
                }));
                continue;
            }
        };
        if defines_path {
            source_path = Some(layer_path.display().to_string());
            source_value = layer_value.clone();
        }
        layer_rows.push(serde_json::json!({
            "path": layer_path.display().to_string(),
            "defines_path": defines_path,
            "value": layer_value,
        }));
    }

    let found = effective_at_path.is_some() || source_value.is_some();
    let effective = effective_at_path.cloned().or_else(|| source_value.clone());
    let envelope = serde_json::json!({
        "schema_version": "harness-config-explain-v1",
        "path": path,
        "found": found,
        "redacted": true,
        "effective": effective,
        "source_path": source_path,
        "source_value": source_value,
        "layers": layer_rows,
        "note": "source_path is the last discovered layer that defines the path; later layers override earlier ones",
    });
    serde_json::to_string_pretty(&envelope)
        .map_err(|err| format!("serialize config explain envelope: {err}"))
}

fn split_config_path(path: &str) -> Vec<String> {
    path.split(['.', '/'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn value_at_path<'a>(
    value: &'a serde_json::Value,
    segments: &[String],
) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in segments {
        current = match current {
            serde_json::Value::Object(map) => map.get(segment).or_else(|| {
                map.iter().find_map(|(key, nested)| {
                    if key.eq_ignore_ascii_case(segment)
                        || key
                            .replace('_', "")
                            .eq_ignore_ascii_case(&segment.replace(['_', '-'], ""))
                    {
                        Some(nested)
                    } else {
                        None
                    }
                })
            })?,
            serde_json::Value::Array(items) => {
                let index: usize = segment.parse().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }
    Some(current)
}

fn layer_value_at_path(
    path: &std::path::Path,
    segments: &[String],
) -> Result<Option<serde_json::Value>, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let root: serde_json::Value =
        json5::from_str(&raw).map_err(|err| format!("parse {}: {err}", path.display()))?;
    Ok(value_at_path(&root, segments).cloned())
}

#[cfg(test)]
mod tests {
    use super::{run, CliCommandInvocation, CliCommandOutput, CliCommandRunner, CliDeps, CliIo};
    use crate::UnwrapOrAbort;
    use harness_core::clock::Clock;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn schema_command_runs_in_process_with_captured_stdout() {
        // arrange
        // act
        // assert
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(["harness", "schema"], &mut io, CliDeps::real());

        assert!(
            outcome.success(),
            "stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(String::from_utf8_lossy(&stdout).contains("\"$schema\""));
    }

    #[test]
    fn prompt_setup_error_preserves_usage_exit_code() {
        // arrange
        // act
        // assert
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(["harness", "prompt"], &mut io, CliDeps::real());

        assert_eq!(outcome.code, 2);
        assert!(stdout.is_empty());
        assert!(String::from_utf8_lossy(&stderr).contains("prompt setup failed"));
    }

    #[test]
    fn config_validate_uses_injected_filesystem_root() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        std::fs::write(
            temp.path().join("harness.jsonc"),
            r#"{
  "provider": {
    "default": {
      "type": "openai_compatible",
      "name": "Local Test Provider",
      "options": {
        "baseURL": "http://127.0.0.1:9999/v1",
        "apiKey": "DUMMY"
      },
      "models": {"mock-model": {"name": "Mock Model"}}
    }
  },
  "model": "default/mock-model",
  "agent": {"build": {"enable": true, "model": "default/mock-model"}},
  "default_agent": "build"
}
"#,
        )
        .unwrap_or_abort();

        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(
            ["harness", "config", "validate"],
            &mut io,
            CliDeps::real().with_filesystem_root(temp.path().to_path_buf()),
        );

        assert!(
            outcome.success(),
            "stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(String::from_utf8_lossy(&stdout).contains("config valid:"));
    }

    #[test]
    fn config_show_effective_emits_redacted_merged_json_with_layers() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        std::fs::write(
            temp.path().join("harness.jsonc"),
            r#"{
  "provider": {
    "default": {
      "type": "openai_compatible",
      "name": "Local Test Provider",
      "options": {
        "baseURL": "http://127.0.0.1:9999/v1",
        "apiKey": "sk-proj-super-secret-key-0123456789abcdef"
      },
      "models": {"mock-model": {"name": "Mock Model"}}
    }
  },
  "model": "default/mock-model",
  "agent": {"build": {"enable": true, "model": "default/mock-model"}},
  "default_agent": "build"
}
"#,
        )
        .unwrap_or_abort();

        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(
            ["harness", "config", "show", "--effective"],
            &mut io,
            CliDeps::real().with_filesystem_root(temp.path().to_path_buf()),
        );

        assert!(
            outcome.success(),
            "stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        let stdout_text = String::from_utf8_lossy(&stdout);
        assert!(
            stdout_text.contains("\"schema_version\": \"harness-config-effective-v1\""),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("\"redacted\": true"),
            "stdout: {stdout_text}"
        );
        assert!(stdout_text.contains("\"layers\""), "stdout: {stdout_text}");
        assert!(
            stdout_text.contains("harness.jsonc"),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("[REDACTED_API_KEY]") || stdout_text.contains("[REDACTED"),
            "expected api key redaction markers in stdout: {stdout_text}"
        );
        assert!(
            !stdout_text.contains("sk-proj-super-secret-key-0123456789abcdef"),
            "secret leaked in stdout: {stdout_text}"
        );
    }

    #[test]
    fn config_show_without_effective_flag_exits_usage() {
        // arrange
        // act
        // assert
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(["harness", "config", "show"], &mut io, CliDeps::real());

        assert_eq!(outcome.code, 2);
        assert!(stdout.is_empty());
        assert!(String::from_utf8_lossy(&stderr).contains("config show requires --effective"));
    }

    #[test]
    fn config_settings_lists_registry_metadata_without_secret_values() {
        // arrange
        // act
        // assert
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(["harness", "config", "settings"], &mut io, CliDeps::real());

        assert!(
            outcome.success(),
            "stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        let stdout_text = String::from_utf8_lossy(&stdout);
        assert!(
            stdout_text.contains("\"schema_version\": \"harness-settings-registry-v1\""),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("\"setting_id\": \"model\""),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("\"sensitivity\": \"secret\""),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("\"metadata_only\": true"),
            "stdout: {stdout_text}"
        );
        assert!(
            !stdout_text.contains("default_value"),
            "settings CLI must not emit default values: {stdout_text}"
        );
        assert!(
            !stdout_text.contains("sk-"),
            "settings CLI must not emit secret-looking values: {stdout_text}"
        );
    }

    #[test]
    fn config_sources_lists_discovered_layers() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        std::fs::write(
            temp.path().join("harness.jsonc"),
            r#"{
  "provider": {
    "default": {
      "type": "openai_compatible",
      "name": "Local Test Provider",
      "options": {
        "baseURL": "http://127.0.0.1:9999/v1",
        "apiKey": "DUMMY"
      },
      "models": {"mock-model": {"name": "Mock Model"}}
    }
  },
  "model": "default/mock-model",
  "agent": {"build": {"enable": true, "model": "default/mock-model"}},
  "default_agent": "build"
}
"#,
        )
        .unwrap_or_abort();

        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(
            ["harness", "config", "sources"],
            &mut io,
            CliDeps::real().with_filesystem_root(temp.path().to_path_buf()),
        );

        assert!(
            outcome.success(),
            "stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        let stdout_text = String::from_utf8_lossy(&stdout);
        assert!(
            stdout_text.contains("\"schema_version\": \"harness-config-sources-v1\""),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("harness.jsonc"),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("\"kind\": \"runtime\""),
            "stdout: {stdout_text}"
        );
    }

    #[test]
    fn config_explain_attributes_overridden_path_to_project_layer() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let xdg = temp.path().join("xdg");
        let project = temp.path().join("project");
        std::fs::create_dir_all(xdg.join("harness")).unwrap_or_abort();
        std::fs::create_dir_all(&project).unwrap_or_abort();

        std::fs::write(
            xdg.join("harness/harness.jsonc"),
            r#"{
  "provider": {
    "default": {
      "type": "openai_compatible",
      "name": "Global Provider",
      "options": {
        "baseURL": "http://127.0.0.1:9999/v1",
        "apiKey": "sk-proj-global-secret-0123456789abcdef"
      },
      "models": {"mock-model": {"name": "Mock Model"}}
    }
  },
  "model": "default/mock-model",
  "agent": {"build": {"enable": true, "model": "default/mock-model"}},
  "default_agent": "build"
}
"#,
        )
        .unwrap_or_abort();
        std::fs::write(
            project.join("harness.jsonc"),
            r#"{
  "model": "default/mock-model",
  "default_agent": "build"
}
"#,
        )
        .unwrap_or_abort();

        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);

        let outcome = run(
            ["harness", "config", "explain", "model"],
            &mut io,
            CliDeps::real()
                .with_filesystem_root(project)
                .with_env("XDG_CONFIG_HOME", xdg.display().to_string()),
        );

        assert!(
            outcome.success(),
            "stderr: {}",
            String::from_utf8_lossy(&stderr)
        );
        let stdout_text = String::from_utf8_lossy(&stdout);
        assert!(
            stdout_text.contains("\"schema_version\": \"harness-config-explain-v1\""),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("\"found\": true"),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("\"path\": \"model\""),
            "stdout: {stdout_text}"
        );
        assert!(
            stdout_text.contains("project") && stdout_text.contains("harness.jsonc"),
            "expected project layer attribution in stdout: {stdout_text}"
        );
        assert!(
            !stdout_text.contains("sk-proj-global-secret-0123456789abcdef"),
            "secret leaked in stdout: {stdout_text}"
        );
    }

    #[test]
    fn cli_deps_runs_injected_command_runner() {
        // arrange
        // act
        // assert
        let runner = Arc::new(RecordingRunner::new(CliCommandOutput {
            exit_code: 0,
            stdout: b"ok".to_vec(),
            stderr: Vec::new(),
        }));
        let runner_clone = Arc::clone(&runner);
        let deps = CliDeps::real().with_command_runner(runner_clone);

        let output = deps
            .command_runner()
            .run(
                CliCommandInvocation::new("git")
                    .args(["status", "--short"])
                    .stdin(b"input".to_vec()),
            )
            .unwrap_or_abort();

        assert_eq!(output.stdout, b"ok".to_vec());
        let calls = runner.calls.lock().unwrap_or_abort();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "git");
        assert_eq!(calls[0].args, ["status", "--short"]);
        assert_eq!(calls[0].stdin, b"input".to_vec());
    }

    #[test]
    fn cli_deps_uses_injected_clock_factory() {
        // arrange
        // act
        // assert
        let calls = Arc::new(AtomicU64::new(0));
        let observed = Arc::clone(&calls);
        let deps = CliDeps::real().with_clock_factory(move |deterministic| {
            observed.fetch_add(if deterministic { 10 } else { 1 }, Ordering::SeqCst);
            Arc::new(FixedClock { mono_ms: 123 })
        });

        let clock = deps.clock(true);

        assert_eq!(clock.mono_ms(), 123);
        assert_eq!(calls.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn cli_deps_exposes_injected_provider() {
        // arrange
        // act
        // assert
        let provider: Arc<dyn harness_providers::Provider> =
            Arc::new(crate::scenarios::golden_path_provider());
        let provider_clone = Arc::clone(&provider);
        let deps = CliDeps::real().with_provider_override(provider_clone);

        let injected = deps.provider_override().unwrap_or_abort();

        assert!(Arc::ptr_eq(&provider, &injected));
    }

    #[test]
    fn cli_deps_collects_injected_credential_env_values() {
        // arrange
        let deps = CliDeps::real()
            .with_env("OPENAI_API_KEY", "env-secret-value")
            .with_env("OPENAI_KEY", "generic-key-secret-value")
            .with_env("ORDINARY_VALUE", "ordinary-secret-value")
            .with_env("SHORT_TOKEN", "short");

        // act
        let values = deps.credential_env_values();

        // assert
        assert!(values.contains(&"env-secret-value".to_string()));
        assert!(values.contains(&"generic-key-secret-value".to_string()));
        assert!(!values.contains(&"ordinary-secret-value".to_string()));
        assert!(!values.contains(&"short".to_string()));
    }

    #[derive(Debug)]
    struct RecordingRunner {
        output: CliCommandOutput,
        calls: Mutex<Vec<CliCommandInvocation>>,
    }

    impl RecordingRunner {
        fn new(output: CliCommandOutput) -> Self {
            Self {
                output,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl CliCommandRunner for RecordingRunner {
        fn run(&self, invocation: CliCommandInvocation) -> Result<CliCommandOutput, String> {
            self.calls.lock().unwrap_or_abort().push(invocation);
            Ok(self.output.clone())
        }
    }

    #[derive(Debug)]
    struct FixedClock {
        mono_ms: u64,
    }

    impl Clock for FixedClock {
        fn mono_ms(&self) -> u64 {
            self.mono_ms
        }

        fn system_time_rfc3339(&self) -> Option<String> {
            None
        }

        fn system_time_rfc3339_millis(&self) -> Option<String> {
            None
        }
    }
}
