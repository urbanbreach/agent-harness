use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use harness_core::sandbox::SandboxChildPlan;
use rustix::io::{fcntl_setfd, FdFlags};
use serde::{Deserialize, Serialize};

use super::{ShellProcessOutput, ToolError};

const HELPER_NAME: &str = "harness-sandbox-helper";
const SETUP_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_LIMIT_BYTES: u64 = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SandboxSetupFrame {
    Ready,
    Error {
        code: SandboxSetupErrorCode,
        message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SandboxSetupErrorCode {
    InvalidRequest,
    UnsafeStandardIo,
    FdClosure,
    Restriction,
    Command,
}

impl SandboxSetupErrorCode {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::UnsafeStandardIo => "unsafe_standard_io",
            Self::FdClosure => "fd_closure",
            Self::Restriction => "restriction",
            Self::Command => "command",
        }
    }
}

#[derive(Serialize)]
struct SandboxSetupRequest<'a> {
    plan: &'a SandboxChildPlan,
}

pub(super) async fn run(
    command: tokio::process::Command,
    plan: SandboxChildPlan,
    timeout_ms: u64,
) -> Result<ShellProcessOutput, ToolError> {
    let helper = helper_path()?;
    let (mut parent_control, child_control) = UnixStream::pair().map_err(setup_error)?;
    fcntl_setfd(&child_control, FdFlags::empty()).map_err(setup_error)?;

    let command = command.as_std();
    let program = command.get_program().to_string_lossy().into_owned();
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let cwd = command.get_current_dir().map(PathBuf::from);
    let mut helper_command = tokio::process::Command::new(helper);
    helper_command
        .arg("--control-fd")
        .arg(child_control.as_raw_fd().to_string())
        .arg("--")
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        helper_command.current_dir(cwd);
    }
    super::apply_sanitized_shell_environment(&mut helper_command);
    // Make the helper (which execs into the real command) its own process-group
    // leader and kill-on-drop, so a timeout or cancellation can terminate the
    // whole confined tree instead of orphaning it past the timeout.
    helper_command.kill_on_drop(true);
    super::configure_shell_process_group(&mut helper_command);
    let child = helper_command.spawn().map_err(setup_error)?;
    drop(child_control);

    serde_json::to_writer(&mut parent_control, &SandboxSetupRequest { plan: &plan })
        .map_err(setup_error)?;
    parent_control.write_all(b"\n").map_err(setup_error)?;
    parent_control
        .shutdown(std::net::Shutdown::Write)
        .map_err(setup_error)?;
    let setup = read_setup_frame(&mut parent_control)?;

    match setup {
        SandboxSetupFrame::Ready => super::await_child_output(child, timeout_ms).await,
        SandboxSetupFrame::Error { code, message } => Err(ToolError::Execution(format!(
            "sandbox child setup failed ({}) via control pipe: {message}",
            code.as_str()
        ))),
    }
}

fn helper_path() -> Result<PathBuf, ToolError> {
    let executable = std::env::current_exe().map_err(setup_error)?;
    let directory = executable.parent().ok_or_else(|| {
        ToolError::Execution(
            "cannot resolve sandbox helper directory from current executable".into(),
        )
    })?;
    let sibling = directory.join(HELPER_NAME);
    if sibling.is_file() {
        return Ok(sibling);
    }
    let development_sibling = directory
        .parent()
        .map(|parent| parent.join(HELPER_NAME))
        .filter(|path| path.is_file());
    development_sibling.ok_or_else(|| {
        ToolError::Execution(format!(
            "Linux sandbox helper `{HELPER_NAME}` is missing beside the running binary; refusing sandboxed spawn"
        ))
    })
}

fn read_setup_frame(control: &mut UnixStream) -> Result<SandboxSetupFrame, ToolError> {
    control
        .set_read_timeout(Some(SETUP_TIMEOUT))
        .map_err(setup_error)?;
    let mut frame = String::new();
    BufReader::new(control)
        .take(FRAME_LIMIT_BYTES)
        .read_line(&mut frame)
        .map_err(setup_error)?;
    if frame.is_empty() {
        return Err(ToolError::Execution(
            "sandbox helper closed the control pipe before READY".into(),
        ));
    }
    serde_json::from_str(&frame).map_err(|error| {
        ToolError::Execution(format!("invalid sandbox helper control frame: {error}"))
    })
}

fn setup_error(error: impl std::fmt::Display) -> ToolError {
    ToolError::Execution(format!("sandbox helper setup failed: {error}"))
}
