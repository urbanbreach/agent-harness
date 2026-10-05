use deno_core::{op2, OpState};
use deno_error::JsErrorBox;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{BufRead, Write},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub(super) struct Protocol {
    output: Mutex<std::io::Stdout>,
    capture: Mutex<Option<super::stdio::Capture>>,
    active: Mutex<String>,
    contexts: Mutex<BTreeMap<String, CancellationToken>>,
    commands: mpsc::Sender<Value>,
    command_input: tokio::sync::Mutex<mpsc::Receiver<Value>>,
    sequence: AtomicU64,
    pending: Mutex<BTreeMap<String, (String, oneshot::Sender<Value>)>>,
}

impl Protocol {
    pub fn start() -> (Arc<Self>, mpsc::Receiver<Value>) {
        let (commands, command_input) = mpsc::channel(32);
        let protocol = Arc::new(Self {
            output: Mutex::new(std::io::stdout()),
            capture: Mutex::new(None),
            active: Mutex::default(),
            contexts: Mutex::default(),
            commands,
            command_input: tokio::sync::Mutex::new(command_input),
            sequence: AtomicU64::new(0),
            pending: Mutex::default(),
        });
        let (sender, receiver) = mpsc::channel(16);
        let reader = Arc::clone(&protocol);
        std::thread::spawn(move || reader.read_input(sender));
        (protocol, receiver)
    }

    fn read_input(&self, sender: mpsc::Sender<Value>) {
        let mut input = std::io::stdin().lock();
        loop {
            let mut line = Vec::new();
            let mut frame = std::io::Read::take(&mut input, 32 * 1024 * 1024 + 1);
            if !matches!(frame.read_until(b'\n', &mut line), Ok(n) if n > 0 && n <= 32 * 1024 * 1024)
            {
                break;
            }
            let Ok(message) = serde_json::from_slice::<Value>(&line) else {
                break;
            };
            match message["type"].as_str() {
                Some("reply") => {
                    if let Some((_, reply)) = message["id"]
                        .as_str()
                        .and_then(|id| self.pending.lock().ok()?.remove(id))
                    {
                        let _ = reply.send(message);
                    }
                }
                Some("cancel") => {
                    let id = message["id"].as_str().unwrap_or_default();
                    self.cancel(id);
                }
                Some("describe" | "invoke") => {
                    if self
                        .enter(message["id"].as_str().unwrap_or_default())
                        .is_err()
                        || self.commands.blocking_send(message).is_err()
                    {
                        break;
                    }
                }
                Some("shutdown") => break,
                _ => {
                    if sender.blocking_send(message).is_err() {
                        break;
                    }
                }
            }
        }
        if let Ok(contexts) = self.contexts.lock() {
            for token in contexts.values() {
                token.cancel();
            }
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        // The input pipe is the owner's lifetime, including when V8 is stuck in user code.
        #[cfg(unix)]
        if rustix::process::getpgrp() == rustix::process::getpid() {
            let _ = rustix::process::kill_process_group(
                rustix::process::getpgrp(),
                rustix::process::Signal::KILL,
            );
        }
    }

    pub fn begin(&self, id: &str) -> Result<(), JsErrorBox> {
        if let Some(capture) = self.capture.lock().map_err(poison)?.as_mut() {
            capture.clear().map_err(JsErrorBox::from_err)?;
        }
        *self.active.lock().map_err(poison)? = id.to_owned();
        self.enter(id)?;
        Ok(())
    }

    fn cancel(&self, id: &str) {
        let primary = self.active.lock().is_ok_and(|active| *active == id);
        if let Ok(contexts) = self.contexts.lock() {
            for (_, token) in contexts
                .iter()
                .filter(|(context, _)| primary || *context == id)
            {
                token.cancel();
            }
        }
    }

    pub fn end(&self) -> Result<(), JsErrorBox> {
        let id = std::mem::take(&mut *self.active.lock().map_err(poison)?);
        self.leave(&id)
    }

    fn enter(&self, id: &str) -> Result<(), JsErrorBox> {
        self.contexts
            .lock()
            .map_err(poison)?
            .insert(id.into(), CancellationToken::new());
        Ok(())
    }

    fn leave(&self, id: &str) -> Result<(), JsErrorBox> {
        if let Some(token) = self.contexts.lock().map_err(poison)?.remove(id) {
            token.cancel();
        }
        self.pending
            .lock()
            .map_err(poison)?
            .retain(|_, (parent, _)| parent != id);
        Ok(())
    }

    pub fn stdio(&self, root: &std::path::Path) -> std::io::Result<deno_runtime::deno_io::Stdio> {
        let (capture, stdio) = super::stdio::Capture::new(root)?;
        *self
            .capture
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))? = Some(capture);
        Ok(stdio)
    }

    pub fn flush_native(&self) -> Result<(), JsErrorBox> {
        let cell = self.active.lock().map_err(poison)?.clone();
        if let Some(capture) = self.capture.lock().map_err(poison)?.as_mut() {
            capture
                .drain(|stream, data| {
                    if cell.is_empty() {
                        return Ok(());
                    }
                    self.write(&json!({"type":"text","cellId":cell,"stream":stream,"data":data}))
                })
                .map_err(JsErrorBox::from_err)?;
        }
        Ok(())
    }

    pub fn emit(&self, mut value: Value) -> Result<(), JsErrorBox> {
        self.flush_native()?;
        if let Some(cell) = value.get("cellId") {
            if !self
                .contexts
                .lock()
                .map_err(poison)?
                .contains_key(cell.as_str().unwrap_or_default())
            {
                return Ok(());
            }
        } else {
            value["cellId"] = self.active.lock().map_err(poison)?.clone().into();
        }
        self.write(&value).map_err(JsErrorBox::from_err)
    }

    fn write(&self, value: &Value) -> std::io::Result<()> {
        let mut output = self
            .output
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        // Serialize before touching stdout, avoiding its lock for every JSON token.
        output.write_all(&serde_json::to_vec(value)?)?;
        output.write_all(b"\n")?;
        output.flush()
    }

    async fn call(&self, operation: String, mut args: Value) -> Result<Value, JsErrorBox> {
        let parent = args["cellId"].as_str().unwrap_or_default().to_owned();
        let cancellation = self
            .contexts
            .lock()
            .map_err(poison)?
            .get(&parent)
            .cloned()
            .ok_or_else(|| JsErrorBox::generic("eval cell is no longer active"))?;
        if cancellation.is_cancelled() {
            return Ok(json!({"cancelled":true}));
        }
        if let Some(args) = args.as_object_mut() {
            args.remove("cellId");
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed).to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .map_err(poison)?
            .insert(id.clone(), (parent.clone(), sender));
        self.emit(
            json!({"type":"call","id":id,"cellId":parent,"operation":operation,"args":args}),
        )?;
        let reply = tokio::select! {
            () = cancellation.cancelled() => {
                self.pending.lock().map_err(poison)?.remove(&id);
                return Ok(json!({"cancelled":true}));
            }
            reply = receiver => reply.map_err(|_| JsErrorBox::generic("eval host disconnected"))?,
        };
        Ok(reply)
    }
}

fn poison(error: impl std::fmt::Display) -> JsErrorBox {
    JsErrorBox::generic(error.to_string())
}

#[op2]
fn op_eval_emit(state: &mut OpState, #[serde] event: serde_json::Value) -> Result<(), JsErrorBox> {
    state.borrow::<Arc<Protocol>>().emit(event)
}

#[op2]
#[serde]
async fn op_eval_call(
    state: Rc<RefCell<OpState>>,
    #[string] operation: String,
    #[serde] args: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let protocol = Arc::clone(state.borrow().borrow::<Arc<Protocol>>());
    protocol.call(operation, args).await
}

#[op2]
#[serde]
async fn op_eval_command(state: Rc<RefCell<OpState>>) -> Option<serde_json::Value> {
    let protocol = Arc::clone(state.borrow().borrow::<Arc<Protocol>>());
    let command = protocol.command_input.lock().await.recv().await;
    command
}

#[op2(fast)]
fn op_eval_leave(state: &mut OpState, #[string] id: String) -> Result<(), JsErrorBox> {
    state.borrow::<Arc<Protocol>>().leave(&id)
}

#[op2]
#[serde]
fn op_eval_function(#[string] source: String) -> Result<serde_json::Value, JsErrorBox> {
    super::tools::parse(&source).map_err(JsErrorBox::generic)
}

deno_core::extension!(harness_eval,
    ops = [op_eval_emit, op_eval_call, op_eval_command, op_eval_leave, op_eval_function],
    js = [dir "src/javascript", "ops.js"],
    options = { protocol: Arc<Protocol> },
    state = |state, options| state.put(options.protocol),
);
