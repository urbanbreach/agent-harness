use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::cleanup::ChildCleanup;
use super::{PagerError, TranscriptSnapshot};

#[derive(Clone, Debug)]
pub struct PagerCommand {
    program: OsString,
    args: Vec<OsString>,
}

impl PagerCommand {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, argument: impl AsRef<OsStr>) -> Self {
        self.args.push(argument.as_ref().to_os_string());
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PagerStdio {
    capture_output: bool,
    timeout: Duration,
}

impl PagerStdio {
    pub const fn inherit() -> Self {
        Self {
            capture_output: false,
            timeout: Duration::from_secs(300),
        }
    }

    pub const fn capture() -> Self {
        Self {
            capture_output: true,
            timeout: Duration::from_secs(300),
        }
    }

    pub const fn capture_with_timeout(timeout: Duration) -> Self {
        Self {
            capture_output: true,
            timeout,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagerExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl PagerExit {
    pub const fn code(code: i32, stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
        Self {
            code: Some(code),
            signal: None,
            stdout,
            stderr,
        }
    }
}

pub fn launch_pager(
    snapshot: &TranscriptSnapshot,
    pager_cmd: &PagerCommand,
    stdio: PagerStdio,
) -> Result<PagerExit, PagerError> {
    let mut command = Command::new(&pager_cmd.program);
    command.args(&pager_cmd.args);
    command.stdin(Stdio::piped());
    if stdio.capture_output {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }
    configure_process_group(&mut command);
    let mut child = command.spawn().map_err(|error| PagerError::Spawn {
        command: pager_cmd.program.to_string_lossy().into_owned(),
        detail: error.to_string(),
    })?;
    let stdout = child.stdout.take().map(spawn_reader);
    let stderr = child.stderr.take().map(spawn_reader);
    let mut cleanup = ChildCleanup::new(child, stdio.timeout);
    let mut stdin = cleanup
        .child()
        .and_then(|child| child.stdin.take())
        .ok_or_else(|| PagerError::Write {
            detail: "pager stdin was not available".to_owned(),
        })?;
    stdin
        .write_all(snapshot.as_bytes())
        .map_err(|error| PagerError::Write {
            detail: error.to_string(),
        })?;
    drop(stdin);

    let deadline = Instant::now() + stdio.timeout;
    let status = loop {
        cleanup.observe();
        match cleanup.child().map(Child::try_wait) {
            Some(Ok(Some(status))) => break status,
            Some(Ok(None)) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Some(Ok(None)) => {
                let report = cleanup.terminate_and_reap();
                join_readers(stdout, stderr)?;
                return Err(PagerError::Timeout { cleanup: report });
            }
            Some(Err(error)) => {
                let _ = cleanup.terminate_and_reap();
                return Err(PagerError::Wait {
                    detail: error.to_string(),
                });
            }
            None => {
                return Err(PagerError::Wait {
                    detail: "pager child was already reaped".to_owned(),
                });
            }
        }
    };
    let report = cleanup.finish();
    if !report.surviving_pids.is_empty() {
        return Err(PagerError::Cleanup { cleanup: report });
    }
    let (stdout, stderr) = join_readers(stdout, stderr)?;
    Ok(exit_from_status(status, stdout, stderr))
}

fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_readers(
    stdout: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    stderr: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
) -> Result<(Vec<u8>, Vec<u8>), PagerError> {
    Ok((
        join_reader(stdout, "stdout")?,
        join_reader(stderr, "stderr")?,
    ))
}

fn join_reader(
    reader: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    stream: &'static str,
) -> Result<Vec<u8>, PagerError> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    reader
        .join()
        .map_err(|_| PagerError::Output {
            stream,
            detail: "reader thread panicked".to_owned(),
        })?
        .map_err(|error| PagerError::Output {
            stream,
            detail: error.to_string(),
        })
}

fn exit_from_status(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> PagerExit {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        return PagerExit {
            code: status.code(),
            signal: status.signal(),
            stdout,
            stderr,
        };
    }
    #[cfg(not(unix))]
    PagerExit {
        code: status.code(),
        signal: None,
        stdout,
        stderr,
    }
}
