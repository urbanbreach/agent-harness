use crate::{cell::Cell, kernel::Kernel, Result, SessionOptions};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

pub struct Session {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub options: SessionOptions,
    pub cells: Mutex<BTreeMap<String, Arc<Cell>>>,
    pub kernels: BTreeMap<String, Arc<Mutex<Option<Kernel>>>>,
    pub controls: Mutex<BTreeMap<String, crate::kernel::Control>>,
    pub memory: BTreeMap<String, Mutex<crate::memory::Policy>>,
    pending: Mutex<BTreeMap<String, (String, oneshot::Sender<Value>)>>,
    pub completed: Mutex<crate::retention::Retained>,
    pub tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    pub cancel: CancellationToken,
    pub sequence: AtomicU64,
    pub detached: AtomicUsize,
}

impl Session {
    pub fn new(options: SessionOptions) -> Result<Self> {
        options.settings.validate()?;
        std::fs::create_dir_all(&options.artifacts)?;
        std::fs::create_dir_all(&options.local_dir)?;
        if options.languages.is_empty()
            || options
                .languages
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != options.languages.len()
            || options
                .languages
                .iter()
                .any(|language| !matches!(language.as_str(), "js" | "py" | "rb" | "jl"))
        {
            return Err("eval languages must contain js, py, rb or jl".into());
        }
        let kernels: BTreeMap<_, _> = options
            .languages
            .iter()
            .map(|language| (language.clone(), Arc::new(Mutex::new(None))))
            .collect();
        let memory = kernels
            .keys()
            .map(|language| (language.clone(), Mutex::default()))
            .collect();
        Ok(Self {
            inner: Arc::new(Inner {
                options,
                cells: Mutex::default(),
                kernels,
                controls: Mutex::default(),
                memory,
                pending: Mutex::default(),
                completed: Mutex::default(),
                tasks: Mutex::default(),
                cancel: CancellationToken::new(),
                sequence: AtomicU64::new(0),
                detached: AtomicUsize::new(0),
            }),
        })
    }

    pub fn is_alive(&self) -> bool {
        !self.inner.cancel.is_cancelled()
    }
    pub fn terminate(&self) {
        self.inner.cancel.cancel();
    }

    /// Submit a cell or a control action. Call events must be answered with send().
    pub async fn execute(
        &self,
        id: &str,
        args: Value,
        tools: Value,
        interactive: bool,
    ) -> Result<mpsc::Receiver<Value>> {
        if !self.is_alive() {
            return Err("eval session has closed".into());
        }
        let args = crate::normalize_request(args, &self.inner.options.languages)?;
        if args["isolate"] == true && !self.inner.options.settings.sandbox.enabled {
            return Err("isolated eval is disabled; enable eval.sandbox.enabled".into());
        }
        let (events, receiver) = mpsc::channel(32);
        match args["action"].as_str().unwrap_or("run") {
            "list" => {
                let result = self.inner.list().await;
                events
                    .send(json!({"type":"result","id":id,"result":result}))
                    .await?;
            }
            "peek" | "stop" => {
                let key = args["cell_id"].as_str().ok_or("cell_id is required")?;
                let cell = self
                    .inner
                    .cells
                    .lock()
                    .await
                    .get(key)
                    .cloned()
                    .ok_or("eval cell is not retained")?;
                if args["action"] == "stop" {
                    cell.cancel.cancel();
                    tokio::time::timeout(Duration::from_secs(5), cell.wait_terminal()).await?;
                }
                let live = !cell.state.lock().await.completed;
                let mut result = cell.result(live).await;
                let status = result["details"]["cells"][0]["status"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned();
                let shown_status = match status.as_str() {
                    "complete" => "completed",
                    "error" => "failed",
                    other => other,
                };
                let output = result["content"][0]["text"]
                    .as_str()
                    .unwrap_or("(no buffered output)");
                result["content"][0]["text"] = format!(
                    "Eval cell {key} ({}) is {shown_status}.\n{output}",
                    cell.language
                )
                .into();
                if args["action"] == "stop"
                    && status == "cancelled"
                    && let Some(details) = result["details"].as_object_mut()
                {
                    details.remove("isError");
                }
                events
                    .send(json!({"type":"result","id":id,"result":result}))
                    .await?;
            }
            "run" => {
                let language = args["language"].as_str().ok_or("language is required")?;
                let mut cells = self.inner.cells.lock().await;
                if cells.contains_key(id) {
                    return Err("eval cell id is already in use".into());
                }
                let mut previous = Vec::new();
                for cell in cells.values().filter(|cell| cell.language == language) {
                    if !cell.state.lock().await.completed {
                        previous.push(Arc::clone(cell));
                    }
                }
                previous.sort_by_key(|cell| cell.created);
                let queued = previous
                    .iter()
                    .map(|cell| cell.id.clone())
                    .collect::<Vec<_>>();
                let predecessor = previous.pop();
                if args["reset"] == true && !queued.is_empty() {
                    return Err("cannot reset a kernel while it has live cells".into());
                }
                let sequence = self.inner.sequence.fetch_add(1, Ordering::Relaxed);
                let cell = Cell::new(
                    id.into(),
                    args,
                    tools,
                    events,
                    &self.inner.options,
                    queued,
                    interactive,
                    self.inner.cancel.child_token(),
                    sequence,
                );
                cells.insert(id.into(), Arc::clone(&cell));
                drop(cells);
                let inner = Arc::clone(&self.inner);
                let task = tokio::spawn(async move {
                    crate::execution::execute(inner, cell, predecessor).await;
                });
                let mut tasks = self.inner.tasks.lock().await;
                tasks.retain(|task| !task.is_finished());
                tasks.push(task);
            }
            _ => return Err("unknown eval action".into()),
        }
        Ok(receiver)
    }

    pub async fn send(&self, message: Value) -> Result<()> {
        let id = message["id"].as_str().ok_or("eval message id is missing")?;
        match message["type"].as_str() {
            Some("reply") => {
                if let Some((_, reply)) = self.inner.pending.lock().await.remove(id) {
                    let _ = reply.send(message);
                }
            }
            Some("cancel" | "steer") => {
                if let Some(cell) = self.inner.cells.lock().await.get(id) {
                    if message["type"] == "cancel" {
                        cell.cancel.cancel();
                    } else {
                        cell.steer.cancel();
                    }
                }
            }
            _ => return Err("invalid eval session message".into()),
        }
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        self.terminate();
        for mut task in std::mem::take(&mut *self.inner.tasks.lock().await) {
            if tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
        for kernel in self.inner.kernels.values() {
            if let Some(mut kernel) = kernel.lock().await.take() {
                kernel.stop().await;
            }
        }
        self.inner.pending.lock().await.clear();
        *self.inner.completed.lock().await = Default::default();
        self.inner.cells.lock().await.clear();
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl Inner {
    async fn list(&self) -> Value {
        let cells: Vec<_> = self.cells.lock().await.values().cloned().collect();
        let mut rows = Vec::new();
        for cell in cells {
            let state = cell.state.lock().await;
            let duration = if state.completed {
                state.duration
            } else {
                state
                    .running_since
                    .map_or(Duration::ZERO, |start| start.elapsed())
            };
            rows.push((
                state.completed,
                cell.created,
                duration.as_secs(),
                json!({"cellId":cell.id,"language":cell.language,"summary":cell.args["summary"],
        "state":match state.status {"complete"=>"completed","error"=>"failed", status=>status},
        "startedAtMs":state.started_at,"queuedBehind":state.queued}),
            ));
        }
        rows.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| if a.0 { b.1.cmp(&a.1) } else { a.1.cmp(&b.1) })
        });
        let live: std::collections::BTreeSet<_> = rows
            .iter()
            .filter(|row| !row.0)
            .map(|row| row.3["cellId"].as_str().unwrap_or_default().to_owned())
            .collect();
        for (_, _, _, cell) in &mut rows {
            if let Some(queued) = cell["queuedBehind"].as_array_mut() {
                queued.retain(|id| id.as_str().is_some_and(|id| live.contains(id)));
            }
        }
        let text = rows
            .iter()
            .map(|(_, _, elapsed, cell)| {
                let queued: Vec<_> = cell["queuedBehind"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                format!(
                    "{} {} {} {elapsed}s{} - {}",
                    cell["cellId"].as_str().unwrap_or_default(),
                    cell["language"].as_str().unwrap_or_default(),
                    cell["state"].as_str().unwrap_or_default(),
                    if queued.is_empty() {
                        String::new()
                    } else {
                        format!(" queued behind {}", queued.join(", "))
                    },
                    cell["summary"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let listed: Vec<_> = rows.into_iter().map(|row| row.3).collect();
        json!({"content":[{"type":"text","text":if text.is_empty() {"No eval cells are live; recent: none"} else {&text}}],"details":{"action":"list","cells":listed}})
    }

    pub async fn call(&self, cell: &Cell, operation: &str, args: Value) -> Result<Value> {
        if cell.cancel.is_cancelled() {
            return Err("eval cell cancelled".into());
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed).to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .await
            .insert(id.clone(), (cell.id.clone(), sender));
        {
            let mut state = cell.state.lock().await;
            if state.busy == 0 {
                state.blocked_since = Some(tokio::time::Instant::now());
            }
            state.busy += 1;
        }
        let result = async {
            cell.event(json!({"type":"call","parent":cell.id,"id":id,"operation":operation,"args":args})).await?;
            let reply = tokio::select! {
                () = cell.cancel.cancelled() => {
                    let _ = cell.events.send(json!({"type":"cancel_call","parent":cell.id,"id":id})).await;
                    return Err("eval cell cancelled".into());
                }
                reply = receiver => reply?,
            };
            if reply["tools"].is_array() {
                *cell.tools.lock().await = reply["tools"].clone();
                cell.tools_changed.store(true, Ordering::Release);
            }
            if let Some(error) = reply.get("error") { return Err(error.as_str().unwrap_or("eval host call failed").to_owned().into()); }
            Ok(reply["result"].clone())
        }.await;
        {
            let mut state = cell.state.lock().await;
            state.busy -= 1;
            if state.busy == 0
                && let Some(start) = state.blocked_since.take()
            {
                state.blocked_time += start.elapsed();
            }
        }
        self.pending.lock().await.remove(&id);
        result
    }

    pub async fn retire_calls(&self, cell: &Cell) {
        let calls = {
            let mut pending = self.pending.lock().await;
            pending
                .extract_if(.., |_, (parent, _)| parent == &cell.id)
                .map(|(id, _)| id)
                .collect::<Vec<_>>()
        };
        for id in calls {
            let _ = cell
                .events
                .send(json!({"type":"cancel_call","parent":cell.id,"id":id}))
                .await;
        }
    }
}
