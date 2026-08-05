mod process;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct CommandSpec {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    env: BTreeMap<OsString, OsString>,
}

impl CommandSpec {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
        self
    }

    pub fn cwd(mut self, path: impl AsRef<Path>) -> Self {
        self.cwd = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .insert(key.as_ref().to_owned(), value.as_ref().to_owned());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    memory_bytes: Option<u64>,
    process_count: Option<u32>,
}

impl ResourceLimits {
    pub const fn verification_default() -> Self {
        Self {
            memory_bytes: Some(16 * 1024 * 1024 * 1024),
            process_count: Some(512),
        }
    }

    pub const fn unrestricted() -> Self {
        Self {
            memory_bytes: None,
            process_count: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct InterruptFlag(Arc<AtomicBool>);

impl InterruptFlag {
    pub fn install() -> Result<Self, DeadlineError> {
        if let Some(flag) = INTERRUPTED.get() {
            return Ok(Self(Arc::clone(flag)));
        }
        let flag = Arc::new(AtomicBool::new(false));
        let handler_flag = Arc::clone(&flag);
        ctrlc::set_handler(move || handler_flag.store(true, Ordering::SeqCst))
            .map_err(|error| DeadlineError::Signal(error.to_string()))?;
        let _ = INTERRUPTED.set(Arc::clone(&flag));
        Ok(Self(flag))
    }

    pub fn new_for_test() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn interrupt(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_interrupted(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Passed,
    Failed,
    TimedOut,
    Interrupted,
    CleanupFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessCleanup {
    pub forced_termination: bool,
    pub detected_child_pids: Vec<u32>,
    pub surviving_pids: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub status: CommandStatus,
    pub duration_millis: u128,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub cleanup: ProcessCleanup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeadlineError {
    Spawn(String),
    Wait(String),
    Signal(String),
}

impl std::fmt::Display for DeadlineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(detail) => write!(formatter, "command spawn: {detail}"),
            Self::Wait(detail) => write!(formatter, "command wait: {detail}"),
            Self::Signal(detail) => write!(formatter, "signal handler: {detail}"),
        }
    }
}

impl std::error::Error for DeadlineError {}

pub struct DeadlineRunner {
    timeout: Duration,
    cleanup_timeout: Duration,
    limits: ResourceLimits,
    interrupt: InterruptFlag,
}

impl DeadlineRunner {
    pub fn new(
        timeout: Duration,
        cleanup_timeout: Duration,
        limits: ResourceLimits,
        interrupt: InterruptFlag,
    ) -> Self {
        Self {
            timeout,
            cleanup_timeout,
            limits,
            interrupt,
        }
    }

    pub fn run(&self, spec: &CommandSpec) -> Result<CommandReceipt, DeadlineError> {
        process::run(
            self.command(spec),
            self.timeout,
            self.cleanup_timeout,
            &self.interrupt,
        )
    }

    fn command(&self, spec: &CommandSpec) -> Command {
        let mut command = if self.limits == ResourceLimits::unrestricted() {
            Command::new(&spec.program)
        } else {
            let mut command = Command::new("prlimit");
            if let Some(memory) = self.limits.memory_bytes {
                command.arg(format!("--as={memory}"));
            }
            if let Some(processes) = self.limits.process_count {
                command.arg(format!("--nproc={processes}"));
            }
            command.arg("--").arg(&spec.program);
            command
        };
        command.args(&spec.args).envs(&spec.env);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        command
    }
}
