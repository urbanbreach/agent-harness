//! Bounded newline transport for an explicitly launched peer, without an implicit ACP handshake.
use super::acp::*;
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};
const MAX_FRAME: usize = 1024 * 1024;
type Reply = mpsc::SyncSender<Result<Vec<u8>, String>>;
struct Request {
    payload: Vec<u8>,
    reply: Reply,
}
pub struct StdioAcpTransport {
    command: String,
    timeout: Duration,
    directory: Option<PathBuf>,
    worker: Option<Worker>,
}
struct Worker {
    send: mpsc::SyncSender<Request>,
    thread: thread::JoinHandle<()>,
}
impl StdioAcpTransport {
    pub fn new(command: impl Into<String>) -> Self {
        Self::with_timeout(command, Duration::from_secs(30))
    }
    pub fn with_timeout(command: impl Into<String>, timeout: Duration) -> Self {
        Self {
            command: command.into(),
            timeout: timeout.clamp(Duration::from_millis(1), Duration::from_secs(300)),
            directory: None,
            worker: None,
        }
    }
    pub fn is_connected(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|w| !w.thread.is_finished())
    }
    pub fn with_current_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }
}
impl Clone for StdioAcpTransport {
    fn clone(&self) -> Self {
        let mut cloned = Self::with_timeout(self.command.clone(), self.timeout);
        cloned.directory.clone_from(&self.directory);
        cloned
    }
}
impl std::fmt::Debug for StdioAcpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioAcpTransport")
            .field("connected", &self.is_connected())
            .finish_non_exhaustive()
    }
}
impl AcpTransport for StdioAcpTransport {
    fn connect(&mut self) -> Result<(), String> {
        if self.worker.is_some() {
            return Err("already connected".into());
        }
        if self.command.trim().is_empty()
            || self.command.len() > 16 * 1024
            || self.command.contains('\0')
        {
            return Err("invalid peer command".into());
        }
        let (send, receive) = mpsc::sync_channel(1);
        let (ready, status) = mpsc::sync_channel(1);
        let command = self.command.clone();
        let timeout = self.timeout;
        let directory = self.directory.clone();
        let worker = thread::Builder::new()
            .name("harness-stdio-peer".into())
            .spawn(move || serve(command, directory, timeout, receive, ready))
            .map_err(|_| "cannot start peer worker")?;
        match status.recv() {
            Ok(Ok(())) => {
                self.worker = Some(Worker {
                    send,
                    thread: worker,
                });
                Ok(())
            }
            result => {
                drop(send);
                let _ = worker.join();
                Err(result
                    .ok()
                    .and_then(Result::err)
                    .unwrap_or_else(|| "peer worker stopped".into()))
            }
        }
    }
    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(worker) = self.worker.take() {
            drop(worker.send);
            worker
                .thread
                .join()
                .map_err(|_| "peer worker stopped unexpectedly")?;
        }
        Ok(())
    }
    fn operate(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        let worker = self.worker.as_ref().ok_or("not connected")?;
        if payload.len() > MAX_FRAME || payload.contains(&b'\n') || payload.contains(&b'\r') {
            return Err("invalid or oversized peer frame".into());
        }
        let (reply, response) = mpsc::sync_channel(1);
        let result = worker
            .send
            .send(Request {
                payload: payload.into(),
                reply,
            })
            .map_err(|_| "peer worker stopped".to_owned())
            .and_then(|()| {
                response
                    .recv()
                    .map_err(|_| "peer worker stopped".to_owned())
            })
            .and_then(|r| r);
        if result.is_err() {
            let _ = self.disconnect();
        }
        result
    }
}
impl Drop for StdioAcpTransport {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

#[cfg(unix)]
fn serve(
    command: String,
    directory: Option<PathBuf>,
    timeout: Duration,
    requests: mpsc::Receiver<Request>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    use std::{os::unix::process::CommandExt, process::Stdio};
    use tokio::{io::BufReader, process::Command};
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            let _ = ready.send(Err("cannot start peer runtime".into()));
            return;
        }
    };
    let _entered = runtime.enter();
    let mut process = Command::new("sh");
    if let Some(directory) = directory {
        process.current_dir(directory);
    }
    process
        .args(["-c", &command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    process.as_std_mut().process_group(0);
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = ready.send(Err("cannot launch peer".into()));
            return;
        }
    };
    let pid = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(rustix::process::Pid::from_raw);
    let mut group = crate::process::Group(pid);
    let pipes = child.stdin.take().zip(child.stdout.take());
    if let Some((mut input, output)) = pipes {
        let mut output = BufReader::new(output);
        if ready.send(Ok(())).is_ok() {
            while let Ok(request) = requests.recv() {
                let result = runtime
                    .block_on(tokio::time::timeout(
                        timeout,
                        exchange(&mut input, &mut output, &request.payload),
                    ))
                    .map_err(|_| "peer operation timed out".to_owned())
                    .and_then(|r| r.map_err(String::from));
                let failed = result.is_err();
                if request.reply.send(result).is_err() || failed {
                    break;
                }
            }
        }
    } else {
        let _ = ready.send(Err("peer pipes unavailable".into()));
    }
    group.stop();
    let _ = runtime.block_on(child.wait());
}
#[cfg(unix)]
async fn exchange(
    input: &mut tokio::process::ChildStdin,
    output: &mut tokio::io::BufReader<tokio::process::ChildStdout>,
    payload: &[u8],
) -> Result<Vec<u8>, &'static str> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    input
        .write_all(payload)
        .await
        .map_err(|_| "cannot write peer frame")?;
    input
        .write_all(b"\n")
        .await
        .map_err(|_| "cannot write peer delimiter")?;
    input.flush().await.map_err(|_| "cannot flush peer input")?;
    let mut frame = Vec::new();
    output
        .take(MAX_FRAME as u64 + 2)
        .read_until(b'\n', &mut frame)
        .await
        .map_err(|_| "cannot read peer frame")?;
    if frame.last() != Some(&b'\n') || frame.len() > MAX_FRAME + 1 {
        return Err("peer closed output or exceeded frame limit");
    }
    frame.pop();
    Ok(frame)
}
#[cfg(not(unix))]
fn serve(
    _: String,
    _: Option<PathBuf>,
    _: Duration,
    _: mpsc::Receiver<Request>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    let _ = ready.send(Err(
        "stdio peer process-tree control is unavailable on this platform".into(),
    ));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StdioAcpAgentModeProduct {
    pub last_connect: AcpConnectOutcome,
    pub last_bind: AcpBindOutcome,
    pub operate_ok: bool,
    pub summary: AcpConnectionSummary,
    pub state: AcpConnectionState,
    pub session: Option<AcpSessionInfo>,
    pub command: String,
}
impl StdioAcpAgentModeProduct {
    pub fn meets_agent_mode_contract(&self) -> bool {
        self.last_connect.is_connected()
            && self.last_bind.is_bound()
            && self.operate_ok
            && self.summary.is_bound()
            && self.session.is_some()
    }
}
/// Diagnostic snapshot of a successful exchange, captured before cleanup.
pub fn run_stdio_acp_agent_mode_product(command: &str) -> StdioAcpAgentModeProduct {
    run_stdio_acp_agent_mode_product_in(command, None)
}
pub fn run_stdio_acp_agent_mode_product_in(
    command: &str,
    directory: Option<&Path>,
) -> StdioAcpAgentModeProduct {
    let mut transport = StdioAcpTransport::new(command);
    if let Some(directory) = directory {
        transport = transport.with_current_dir(directory);
    }
    let mut connection = AcpConnection::new(transport);
    let last_connect = connect_acp_outcome(&mut connection);
    let last_bind = bind_acp_session_outcome(&mut connection, "harness.probe.agent");
    let operate_ok = connection.operate(br#"{"method":"initialize"}"#).is_ok();
    StdioAcpAgentModeProduct {
        last_connect,
        last_bind,
        operate_ok,
        summary: connection.summary(),
        state: connection.state().clone(),
        session: connection.session().cloned(),
        command: command.into(),
    }
}
