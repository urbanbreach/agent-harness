use harness_core::event::{EventEnvelopeV1, EventV1, ToolCallStatus};
pub(crate) use harness_core::proj::load_run_metadata;
use std::io::{BufRead, Write};
use std::{path::Path, time::Duration};
pub(crate) const DEFAULT_EVENT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) enum ToolFinishTerminalEvents {
    Ignore,
    Error,
}
pub fn load_events_from_run_dir(directory: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    harness_core::store::read_events(&directory.join("events.jsonl")).map_err(|e| e.to_string())
}
pub async fn wait_for_permission_id(
    path: &Path,
    tool: &str,
    timeout: Duration,
) -> Result<String, String> {
    wait_event(path, Some(timeout), |event| match &event.payload {
        EventV1::PermissionRequested(permission)
            if permission
                .tool_call_id
                .as_ref()
                .is_some_and(|id| id.as_str() == tool) =>
        {
            Some(Ok(permission.permission_id.clone()))
        }
        EventV1::RunFinished(_) | EventV1::RunFailed(_) => {
            Some(Err("run ended before requesting approval".into()))
        }
        _ => None,
    })
    .await
}
pub(crate) async fn wait_for_tool_finished(
    path: &Path,
    tool: &str,
    timeout: Option<Duration>,
    terminal: ToolFinishTerminalEvents,
) -> Result<ToolCallStatus, String> {
    wait_event(path, timeout, |event| match &event.payload {
        EventV1::ToolCallFinished(finished) if finished.tool_call_id.as_str() == tool => {
            Some(Ok(finished.status))
        }
        EventV1::RunFinished(_) | EventV1::RunFailed(_)
            if matches!(terminal, ToolFinishTerminalEvents::Error) =>
        {
            Some(Err("run ended before the tool finished".into()))
        }
        _ => None,
    })
    .await
}
async fn wait_event<T>(
    path: &Path,
    timeout: Option<Duration>,
    select: impl Fn(&EventEnvelopeV1) -> Option<Result<T, String>>,
) -> Result<T, String> {
    let deadline = timeout.and_then(|timeout| tokio::time::Instant::now().checked_add(timeout));
    // ponytail: only demo scenarios poll this small journal; live execution uses subscriptions.
    loop {
        let length = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        for event in
            harness_core::store::JournalReader::open(path, length).map_err(|e| e.to_string())?
        {
            match event {
                Ok(event) => {
                    if let Some(result) = select(&event) {
                        return result;
                    }
                }
                Err(harness_core::store::EventStoreError::Json(error)) if error.is_eof() => break,
                Err(error) => return Err(error.to_string()),
            }
        }
        if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            return Err("timed out waiting for a session event".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub struct CliIo<'a> {
    pub stdin: &'a mut dyn BufRead,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    pub(crate) stdin_is_terminal: bool,
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
            stdin_is_terminal: false,
        }
    }
    pub fn with_stdin_terminal(mut self, terminal: bool) -> Self {
        self.stdin_is_terminal = terminal;
        self
    }
}

// Keep borrowed, potentially non-Send output on the caller's thread. Each write
// waits for its result, so an output failure reaches session cleanup immediately.
pub(crate) fn with_output_worker(
    io: &mut CliIo<'_>,
    work: impl FnOnce(&mut CliIo<'_>) -> Result<(), String> + Send,
) -> Result<(), String> {
    if tokio::runtime::Handle::try_current().is_err() {
        return work(io);
    }
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    type Output = (bool, Option<Vec<u8>>);
    struct Forward<'a> {
        stderr: bool,
        output: &'a SyncSender<Output>,
        replies: &'a Receiver<std::io::Result<()>>,
    }
    impl Forward<'_> {
        fn send(&self, bytes: Option<Vec<u8>>) -> std::io::Result<()> {
            self.output
                .send((self.stderr, bytes))
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
            self.replies
                .recv()
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?
        }
    }
    impl Write for Forward<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let size = bytes.len().min(64 * 1024);
            self.send(Some(bytes[..size].to_vec()))?;
            Ok(size)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.send(None)
        }
    }
    let (output, pending) = sync_channel::<Output>(0);
    let (reply, replies) = sync_channel(0);
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("harness-cli".into())
            .spawn_scoped(scope, move || {
                let mut stdout = Forward {
                    stderr: false,
                    output: &output,
                    replies: &replies,
                };
                let mut stderr = Forward {
                    stderr: true,
                    output: &output,
                    replies: &replies,
                };
                work(&mut CliIo::new(
                    &mut std::io::empty(),
                    &mut stdout,
                    &mut stderr,
                ))
            })
            .map_err(|e| e.to_string())?;
        while let Ok((stderr, bytes)) = pending.recv() {
            let writer = if stderr {
                &mut io.stderr
            } else {
                &mut io.stdout
            };
            let result = match bytes {
                Some(bytes) => writer.write_all(&bytes),
                None => writer.flush(),
            };
            if reply.send(result).is_err() {
                break;
            }
        }
        worker.join().map_err(|_| "CLI worker stopped".to_owned())?
    })
}
