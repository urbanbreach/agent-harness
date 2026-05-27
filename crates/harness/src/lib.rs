//! Library surface for the Harness CLI.
//!
//! The binary is intentionally a thin shim over this module so tests can drive
//! CLI behavior in-process with explicit stdin/stdout/stderr instead of spawning
//! the compiled executable for every assertion.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, load_resolved_config_with_context,
};

mod bootstrap;
mod cli_config;
mod cli_io;
mod cli_labels;
mod defaults;
mod doctor;
mod dynamic_prompt;
mod generated_model_catalog;
mod logging;
mod model_probe;
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
use doctor::DoctorCommand;
use models::ModelsCommand;
use replay::ReplayCommand;
use run::RunCommand;
use sessions::SessionsCommand;

#[derive(Debug, Parser)]
#[command(name = "harness")]
#[command(version)]
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
    Doctor(DoctorCommand),
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

/// Explicit standard I/O for in-process CLI tests.
pub struct CliIo<'a> {
    pub stdin: &'a mut dyn BufRead,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
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
        }
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

    fn current_dir(&self) -> Result<PathBuf, io::Error> {
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
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let outcome = run(std::env::args_os(), &mut io, CliDeps::real());
    ExitCode::from(outcome.code.clamp(0, u8::MAX as i32) as u8)
}

fn execute_cli(cli: Cli, io: &mut CliIo<'_>, deps: CliDeps) -> i32 {
    let Cli {
        config,
        session_dir,
        interactive,
        command,
    } = cli;

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
        Commands::Run(command) => run::execute_with_io(command, config, session_dir, io, &deps),
        Commands::Doctor(command) => {
            doctor::execute_with_io(command, config, session_dir, io, &deps)
        }
        Commands::Models(command) => models::execute_with_io(command, config, io, &deps),
        Commands::Prompt(command) => {
            prompt::execute_with_io(command, config, session_dir, io, &deps)
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
        },
    }
}

fn execute_config_validate(
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let config_context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "config validation failed: failed to resolve config context: {err}"
            );
            return 2;
        }
    };
    let Some(loaded) = (match load_resolved_config_with_context(config.as_deref(), &config_context)
    {
        Ok(loaded) => loaded,
        Err(err) => {
            let _ = writeln!(io.stderr, "config validation failed: {err}");
            return 1;
        }
    }) else {
        let _ = writeln!(
            io.stderr,
            "no config file found; pass --config <path>, create ./harness.jsonc or ./harness.json, or create $XDG_CONFIG_HOME/harness/harness.jsonc or $XDG_CONFIG_HOME/harness/harness.json for shared defaults. A starting point lives at configs/harness.example.jsonc"
        );
        return 2;
    };

    let path_display = loaded.path_display();
    let mut config = loaded.config;
    config.apply_session_dir_override(session_dir);
    let _ = writeln!(io.stdout, "config valid: {path_display}");
    0
}

#[cfg(test)]
mod tests {
    use super::{run, CliCommandInvocation, CliCommandOutput, CliCommandRunner, CliDeps, CliIo};
    use harness_core::clock::Clock;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn schema_command_runs_in_process_with_captured_stdout() {
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
        let temp = tempfile::tempdir().expect("tempdir");
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
        .expect("write config");

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
    fn cli_deps_runs_injected_command_runner() {
        let runner = Arc::new(RecordingRunner::new(CliCommandOutput {
            exit_code: 0,
            stdout: b"ok".to_vec(),
            stderr: Vec::new(),
        }));
        let deps = CliDeps::real().with_command_runner(runner.clone());

        let output = deps
            .command_runner()
            .run(
                CliCommandInvocation::new("git")
                    .args(["status", "--short"])
                    .stdin(b"input".to_vec()),
            )
            .expect("command output");

        assert_eq!(output.stdout, b"ok".to_vec());
        let calls = runner.calls.lock().expect("calls lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "git");
        assert_eq!(calls[0].args, ["status", "--short"]);
        assert_eq!(calls[0].stdin, b"input".to_vec());
    }

    #[test]
    fn cli_deps_uses_injected_clock_factory() {
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
        let provider: Arc<dyn harness_providers::Provider> =
            Arc::new(crate::scenarios::golden_path_provider());
        let deps = CliDeps::real().with_provider_override(provider.clone());

        let injected = deps.provider_override().expect("provider override");

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
            self.calls.lock().expect("calls lock").push(invocation);
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
