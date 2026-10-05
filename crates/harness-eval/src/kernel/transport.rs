use super::Process;
use crate::{Result, SessionOptions};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

enum Command {
    Request(Value, mpsc::Sender<Value>),
    Send(Value),
    Interrupt(String),
}

#[derive(Clone)]
pub(crate) struct Control {
    sender: mpsc::Sender<Command>,
    cancellation: CancellationToken,
}

impl Control {
    pub async fn request(&self, value: Value) -> Result<mpsc::Receiver<Value>> {
        let (sender, receiver) = mpsc::channel(128);
        self.sender
            .send(Command::Request(value, sender))
            .await
            .map_err(|_| "eval kernel is unavailable")?;
        Ok(receiver)
    }

    pub async fn send(&self, value: Value) -> Result<()> {
        self.sender
            .send(Command::Send(value))
            .await
            .map_err(|_| "eval kernel is unavailable".into())
    }

    pub async fn interrupt(&self, id: &str) -> Result<()> {
        self.sender
            .send(Command::Interrupt(id.into()))
            .await
            .map_err(|_| "eval kernel is unavailable".into())
    }

    pub fn terminate(&self) {
        self.cancellation.cancel();
    }
}

pub(crate) struct Kernel {
    pub control: Control,
    pub runtime: Value,
    events: Option<mpsc::Receiver<Value>>,
    task: Option<tokio::task::JoinHandle<()>>,
    pid: Option<u32>,
}

impl Kernel {
    pub async fn start(options: &SessionOptions, language: &str) -> Result<Self> {
        let process = Process::start(options, language).await?;
        let (sender, commands) = mpsc::channel(32);
        let cancellation = CancellationToken::new();
        let control = Control {
            sender,
            cancellation: cancellation.clone(),
        };
        let pid = process.child.id();
        let runtime = process.runtime.clone();
        let task = tokio::spawn(pump(process, commands, cancellation));
        Ok(Self {
            control,
            runtime,
            pid,
            task: Some(task),
            events: None,
        })
    }

    pub fn is_alive(&self) -> bool {
        !self.control.cancellation.is_cancelled()
    }

    pub async fn send(&mut self, value: Value) -> Result<()> {
        if value["type"] == "run" {
            self.events = Some(self.control.request(value).await?);
        } else {
            self.control.send(value).await?;
        }
        Ok(())
    }

    pub async fn receive(&mut self) -> Result<Value> {
        self.events
            .as_mut()
            .ok_or("eval kernel has no active cell")?
            .recv()
            .await
            .ok_or_else(|| "eval worker exited before the cell completed".into())
    }

    pub async fn interrupt(&self, id: &str) -> Result<()> {
        self.control.interrupt(id).await
    }

    pub async fn footprint(&self) -> Option<Value> {
        crate::memory::footprint(self.pid).await
    }

    pub async fn stop(&mut self) {
        self.control.terminate();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        self.control.terminate();
    }
}

async fn pump(
    mut process: Process,
    mut commands: mpsc::Receiver<Command>,
    cancellation: CancellationToken,
) {
    let mut routes = BTreeMap::<String, mpsc::Sender<Value>>::new();
    let outcome: Result<()> = async {
        loop {
            routes.retain(|_, sender| !sender.is_closed());
            tokio::select! {
                () = cancellation.cancelled() => return Ok(()),
                command = commands.recv() => match command {
                    Some(Command::Request(value, sender)) => {
                        let id = value["id"].as_str().ok_or("kernel request id is required")?.to_owned();
                        routes.insert(id, sender);
                        process.send(value).await?;
                    }
                    Some(Command::Send(value)) => process.send(value).await?,
                    Some(Command::Interrupt(id)) => { if routes.contains_key(&id) { process.interrupt(&id).await?; } }
                    None => return Ok(()),
                },
                event = process.receive() => {
                    let event = event?;
                    let id = event["cellId"].as_str().unwrap_or_default().to_owned();
                    let terminal = event["type"] == "result";
                    if let Some(sender) = routes.get(&id) {
                        tokio::select! {
                            () = cancellation.cancelled() => return Ok(()),
                            _ = sender.send(event) => {},
                        }
                    }
                    if terminal { routes.remove(&id); }
                }
            }
        }
    }.await;
    if let Err(error) = outcome {
        for (id, sender) in routes {
            let _ = sender.try_send(
                serde_json::json!({"type":"transport-error","cellId":id,"error":error.to_string()}),
            );
        }
    }
    cancellation.cancel();
    process.stop().await;
}
