//! Run-owned, bounded LSP workers. Only a worker owns its protocol connection.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::tool::ToolError;
use harness_core::ToolResultExt;
use tokio::sync::oneshot;

use super::session::{LspSession, SessionControl, REQUEST_TIMEOUT};
use super::LspServerSpec;

const MAX_SERVERS: usize = 6;
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

type Operation = Box<dyn FnOnce(Result<&mut LspSession, ToolError>) -> bool + Send>;

struct Job {
    run: Operation,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    _pending: Pending,
}

struct Pending(Arc<AtomicUsize>);
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Worker {
    root: PathBuf,
    spec: LspServerSpec,
    sender: mpsc::SyncSender<Job>,
    shutdown: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
    last_used: Instant,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
pub(super) struct LspPool {
    workers: Mutex<Vec<Worker>>,
}

impl Drop for LspPool {
    fn drop(&mut self) {
        // Stop all workers before joining any of them, so shutdown runs concurrently.
        for worker in self
            .workers
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            worker.shutdown.store(true, Ordering::Release);
        }
    }
}

// Dropping an awaiting tool future cancels even a blocked initialize/request.
struct CancelOnDrop(Option<Arc<AtomicBool>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(flag) = &self.0 {
            flag.store(true, Ordering::Release);
        }
    }
}

impl LspPool {
    pub(super) async fn execute<T: Send + 'static>(
        &self,
        spec: LspServerSpec,
        root: PathBuf,
        operation: impl FnOnce(&mut LspSession) -> Result<T, ToolError> + Send + 'static,
    ) -> Result<T, ToolError> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut cancel = CancelOnDrop(Some(Arc::clone(&cancelled)));
        let (reply, response) = oneshot::channel();
        {
            let mut workers = self.workers.lock().map_err(|_| {
                ToolError::Execution("language server pool lock poisoned".to_string())
            })?;
            workers.retain(|worker| {
                !(worker.shutdown.load(Ordering::Acquire)
                    || worker.pending.load(Ordering::Acquire) == 0
                        && worker.last_used.elapsed() >= IDLE_TIMEOUT)
            });
            let index = match workers
                .iter()
                .position(|worker| worker.root == root && worker.spec == spec)
            {
                Some(index) => index,
                None => {
                    if workers.len() == MAX_SERVERS {
                        let oldest = workers.iter().enumerate()
                            .filter(|(_, worker)| worker.pending.load(Ordering::Acquire) == 0)
                            .min_by_key(|(_, worker)| worker.last_used)
                            .map(|(index, _)| index)
                            .ok_or_else(|| ToolError::Execution("all six language servers are busy; retry after a check completes".to_string()))?;
                        workers.remove(oldest);
                    }
                    workers.push(Worker::start(root, spec)?);
                    workers.len() - 1
                }
            };
            let worker = &mut workers[index];
            worker.last_used = Instant::now();
            worker.pending.fetch_add(1, Ordering::AcqRel);
            worker
                .sender
                .try_send(Job {
                    cancelled,
                    deadline: Instant::now() + REQUEST_TIMEOUT,
                    _pending: Pending(Arc::clone(&worker.pending)),
                    run: Box::new(move |session| {
                        let result = session.and_then(operation);
                        let healthy = result.is_ok();
                        let _ = reply.send(result);
                        healthy
                    }),
                })
                .map_err(|_| {
                    ToolError::Execution(
                        "language server queue is full or closed; retry the check".to_string(),
                    )
                })?;
        }
        let result = response.await.tool_err("language server worker stopped")?;
        cancel.0 = None;
        result
    }
}

impl Worker {
    fn start(root: PathBuf, spec: LspServerSpec) -> Result<Self, ToolError> {
        let (sender, commands) = mpsc::sync_channel::<Job>(32);
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut worker = Self {
            root: root.clone(),
            spec: spec.clone(),
            sender,
            shutdown: Arc::clone(&shutdown),
            pending: Arc::new(AtomicUsize::new(0)),
            last_used: Instant::now(),
            thread: None,
        };
        let thread = thread::Builder::new()
            .name("harness-lsp-session".to_string())
            .spawn(move || run_worker(root, spec, commands, shutdown))
            .tool_err("failed to start language server worker")?;
        worker.thread = Some(thread);
        Ok(worker)
    }
}

fn run_worker(
    root: PathBuf,
    spec: LspServerSpec,
    commands: mpsc::Receiver<Job>,
    shutdown: Arc<AtomicBool>,
) {
    let _closed = CancelOnDrop(Some(Arc::clone(&shutdown)));
    let mut session: Option<LspSession> = None;
    let mut last_used = Instant::now();
    // ponytail: one request at a time per server; independent servers run in parallel.
    // Add multiplexing only if measured queue time outweighs language-server work.
    while !shutdown.load(Ordering::Acquire) && last_used.elapsed() < IDLE_TIMEOUT {
        match commands.recv_timeout(POLL_INTERVAL) {
            Ok(job) => {
                if job.cancelled.load(Ordering::Acquire) {
                    continue;
                }
                let control = SessionControl {
                    cancelled: job.cancelled,
                    shutdown: Arc::clone(&shutdown),
                };
                let result = if Instant::now() >= job.deadline {
                    Err(ToolError::Execution(
                        "language server request expired in queue".to_string(),
                    ))
                } else if let Some(session) = session.as_mut() {
                    session.begin_operation(control, job.deadline);
                    session.sync_documents(&spec.name)
                } else {
                    LspSession::start(&spec, &root, control, job.deadline)
                        .map(|started| session = Some(started))
                };
                let result = result.and_then(|()| {
                    session.as_mut().ok_or_else(|| {
                        ToolError::Execution("language server unavailable".to_string())
                    })
                });
                if !(job.run)(result) {
                    // Discard uncertain protocol/document state; the next call starts fresh.
                    session = None;
                }
                last_used = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if session
                    .as_mut()
                    .is_some_and(|session| session.drain_messages().is_err())
                {
                    session = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(session);
}
