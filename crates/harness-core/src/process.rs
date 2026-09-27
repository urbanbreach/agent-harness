use crate::tool::ToolError;
use std::{
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

/// Runs a bounded native command from a synchronous adapter, including one inside a Tokio host.
pub fn run_blocking(command: Command, timeout: Duration) -> Result<Output, ToolError> {
    std::thread::Builder::new()
        .name("harness-native-command".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(run(command, timeout, &CancellationToken::new()))
        })?
        .join()
        .map_err(|_| ToolError::Execution("native command worker stopped".into()))?
}

#[cfg(unix)]
pub struct Group(pub(crate) Option<rustix::process::Pid>);
#[cfg(unix)]
impl Group {
    fn signal(&self, signal: rustix::process::Signal) {
        if let Some(pid) = self.0 {
            let _ = rustix::process::kill_process_group(pid, signal);
        }
    }
    pub fn stop(&mut self) {
        self.signal(rustix::process::Signal::KILL);
        self.0 = None;
    }
    pub async fn terminate(
        &mut self,
        child: &mut tokio::process::Child,
    ) -> std::io::Result<ExitStatus> {
        self.signal(rustix::process::Signal::TERM);
        let result = match tokio::time::timeout(Duration::from_millis(200), child.wait()).await {
            Ok(status) => status,
            Err(_) => {
                self.signal(rustix::process::Signal::KILL);
                child.wait().await
            }
        };
        self.stop();
        result
    }
}

#[cfg(unix)]
pub fn spawn_group(mut command: Command) -> Result<(tokio::process::Child, Group), ToolError> {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
    let child = command.kill_on_drop(true).spawn()?;
    let pid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| ToolError::Execution("child PID is unavailable".into()))?;
    Ok((child, Group(Some(pid))))
}
#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(unix)]
pub async fn run(
    mut command: Command,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Output, ToolError> {
    if cancellation.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (mut child, mut group) = spawn_group(command)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::Execution("stdout pipe is unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::Execution("stderr pipe is unavailable".into()))?;
    let mut stdout = tokio::spawn(capture(stdout));
    let mut stderr = tokio::spawn(capture(stderr));
    let mut interrupted = None;
    let status = tokio::select! {
        biased;
        () = cancellation.cancelled() => { interrupted = Some(ToolError::Cancelled); None },
        () = tokio::time::sleep(timeout) => { interrupted = Some(ToolError::Execution("command timed out".into())); None },
        status = child.wait() => Some(status?),
    };
    let status = if let Some(status) = status {
        status
    } else {
        group.terminate(&mut child).await?
    };
    // A command cannot leave detached descendants holding its pipes after it completes.
    group.stop();
    let captured = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(&mut stdout, &mut stderr)
    })
    .await;
    let (stdout, stderr) = match captured {
        Ok(output) => output,
        Err(_) => {
            stdout.abort();
            stderr.abort();
            let _ = tokio::join!(stdout, stderr);
            return Err(interrupted.unwrap_or_else(|| {
                ToolError::Execution("command descendants did not close their output pipes".into())
            }));
        }
    };
    let (stdout, out_truncated) =
        stdout.map_err(|_| ToolError::Execution("stdout reader stopped".into()))??;
    let (stderr, err_truncated) =
        stderr.map_err(|_| ToolError::Execution("stderr reader stopped".into()))??;
    if let Some(error) = interrupted {
        return Err(error);
    }
    Ok(Output {
        status,
        stdout,
        stderr,
        truncated: out_truncated || err_truncated,
    })
}
#[cfg(not(unix))]
pub async fn run(_: Command, _: Duration, _: &CancellationToken) -> Result<Output, ToolError> {
    Err(ToolError::Execution(
        "process-tree control is unavailable on this platform".into(),
    ))
}
async fn capture(mut stream: impl AsyncRead + Unpin) -> Result<(Vec<u8>, bool), std::io::Error> {
    const LIMIT: usize = 512 * 1024;
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    let mut truncated = false;
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok((output, truncated));
        }
        let keep = count.min(LIMIT - output.len());
        output.extend_from_slice(&buffer[..keep]);
        truncated |= keep < count;
    }
}

/// Deliberately pass only process lookup, terminal, temporary-directory and locale settings.
pub fn environment(command: &mut Command) {
    command
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("TERM", "dumb");
    for key in [
        "PATH", "TERM", "TMPDIR", "TEMP", "TMP", "LANG", "LC_ALL", "LC_CTYPE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}
