use std::collections::BTreeSet;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::cleanup::ProcessCleanup;
use super::process_tree::{descendants, living, terminate_group, terminate_pids, wait_for_living};

pub(super) struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(super) enum BoundedFailureKind {
    Spawn,
    Wait,
    Timeout,
    Survivors,
}

pub(super) struct BoundedFailure {
    pub kind: BoundedFailureKind,
    pub detail: String,
    pub cleanup: ProcessCleanup,
}

pub(super) fn run(
    command: &mut Command,
    timeout: Duration,
    cleanup_timeout: Duration,
) -> Result<BoundedOutput, BoundedFailure> {
    command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| BoundedFailure {
        kind: BoundedFailureKind::Spawn,
        detail: error.to_string(),
        cleanup: ProcessCleanup::default(),
    })?;
    let stdout = spawn_reader(child.stdout.take());
    let stderr = spawn_reader(child.stderr.take());
    let mut guard = ChildGuard::new(child, stdout, stderr, cleanup_timeout);
    let deadline = Instant::now() + timeout;
    loop {
        guard.observe();
        match guard.try_wait() {
            Ok(Some(status)) => return guard.finish(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let cleanup = guard.terminate_and_reap();
                return Err(BoundedFailure {
                    kind: BoundedFailureKind::Timeout,
                    detail: format!("timed out after {} ms", timeout.as_millis()),
                    cleanup,
                });
            }
            Err(detail) => {
                let cleanup = guard.terminate_and_reap();
                return Err(BoundedFailure {
                    kind: BoundedFailureKind::Wait,
                    detail,
                    cleanup,
                });
            }
        }
    }
}

struct ChildGuard {
    child: Option<Child>,
    pid: u32,
    observed: BTreeSet<u32>,
    cleanup_timeout: Duration,
    stdout: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    stderr: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
}

impl ChildGuard {
    fn new(
        child: Child,
        stdout: JoinHandle<std::io::Result<Vec<u8>>>,
        stderr: JoinHandle<std::io::Result<Vec<u8>>>,
        cleanup_timeout: Duration,
    ) -> Self {
        let pid = child.id();
        Self {
            child: Some(child),
            pid,
            observed: BTreeSet::new(),
            cleanup_timeout,
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, String> {
        self.child
            .as_mut()
            .ok_or_else(|| "bounded child was already reaped".to_owned())?
            .try_wait()
            .map_err(|error| error.to_string())
    }

    fn observe(&mut self) {
        self.observed.extend(descendants(self.pid));
    }

    fn finish(mut self, status: ExitStatus) -> Result<BoundedOutput, BoundedFailure> {
        self.child.take();
        let detected = wait_for_living(&self.observed, self.cleanup_timeout);
        if !detected.is_empty() {
            terminate_pids(&detected);
            let surviving = wait_for_living(&self.observed, self.cleanup_timeout);
            return Err(BoundedFailure {
                kind: BoundedFailureKind::Survivors,
                detail: format!("left child PIDs {detected:?}"),
                cleanup: ProcessCleanup {
                    forced_termination: true,
                    detected_child_pids: detected,
                    surviving_pids: surviving,
                },
            });
        }
        let stdout = join_reader(self.stdout.take(), "stdout")?;
        let stderr = join_reader(self.stderr.take(), "stderr")?;
        Ok(BoundedOutput {
            status,
            stdout,
            stderr,
        })
    }

    fn terminate_and_reap(&mut self) -> ProcessCleanup {
        self.observe();
        terminate_group(self.pid, self.cleanup_timeout);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let detected = living(&self.observed);
        let surviving = if detected.is_empty() {
            Vec::new()
        } else {
            terminate_pids(&detected);
            wait_for_living(&self.observed, self.cleanup_timeout)
        };
        ProcessCleanup {
            forced_termination: true,
            detected_child_pids: detected,
            surviving_pids: surviving,
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.terminate_and_reap();
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    reader: Option<R>,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut reader) = reader {
            reader.read_to_end(&mut bytes)?;
        }
        Ok(bytes)
    })
}

fn join_reader(
    handle: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    name: &str,
) -> Result<Vec<u8>, BoundedFailure> {
    handle
        .ok_or_else(|| reader_failure(name, "reader missing"))?
        .join()
        .map_err(|_| reader_failure(name, "reader panicked"))?
        .map_err(|error| reader_failure(name, &error.to_string()))
}

fn reader_failure(name: &str, detail: &str) -> BoundedFailure {
    BoundedFailure {
        kind: BoundedFailureKind::Wait,
        detail: format!("{name}: {detail}"),
        cleanup: ProcessCleanup::default(),
    }
}
