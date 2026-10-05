use crate::{cell::Cell, kernel::Control, session::Inner, Result, Session};
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct KernelToolError {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

impl KernelToolError {
    fn cancelled() -> Self {
        Self {
            code: "kernel_tool_stale".into(),
            message: "Kernel tool call cancelled".into(),
            details: None,
        }
    }

    fn unavailable() -> Self {
        Self {
            code: "tools_unavailable".into(),
            message: "Kernel tools require a live JavaScript worker context".into(),
            details: None,
        }
    }
}

impl Session {
    /// Kernel-defined tools support a per-invocation host-tool allow/deny scope.
    pub fn kernel_tools_capabilities(&self) -> Value {
        json!({"invokeScope":true})
    }

    pub async fn describe_kernel_tools(&self, names: &[String]) -> Result<Value> {
        let control = self
            .inner
            .controls
            .lock()
            .await
            .get("js")
            .cloned()
            .ok_or_else(KernelToolError::unavailable)?;
        let id = format!(
            "describe-{}",
            self.inner.sequence.fetch_add(1, Ordering::Relaxed)
        );
        let mut events = control
            .request(json!({"type":"describe","id":id,"names":names}))
            .await?;
        let event = match tokio::time::timeout(Duration::from_secs(10), events.recv()).await {
            Ok(event) => event.ok_or_else(KernelToolError::unavailable)?,
            Err(error) => {
                control.interrupt(&id).await?;
                return Err(error.into());
            }
        };
        response(event)
    }

    /// Invoke a saved descriptor, including while its defining cell awaits a host
    /// call. Answer emitted host calls with send(), just as for execute().
    pub async fn invoke_kernel_tool(
        &self,
        request: Value,
        scope: Option<Value>,
        tools: Value,
        cancellation: CancellationToken,
    ) -> Result<mpsc::Receiver<Value>> {
        if cancellation.is_cancelled() {
            return Err(KernelToolError::cancelled().into());
        }
        if request["name"].as_str().is_none_or(str::is_empty)
            || request["call_id"].as_str().is_none_or(str::is_empty)
            || request["kernel_generation"].as_u64().is_none()
            || request["definition_revision"].as_u64().is_none()
        {
            return Err("invalid kernel tool invocation descriptor".into());
        }
        let control = self
            .inner
            .controls
            .lock()
            .await
            .get("js")
            .cloned()
            .ok_or_else(KernelToolError::unavailable)?;
        let sequence = self.inner.sequence.fetch_add(1, Ordering::Relaxed);
        let id = format!("invoke-{sequence}");
        let (sender, receiver) = mpsc::channel(32);
        let cell = Cell::new(
            id.clone(),
            json!({"language":"js","summary":request["name"],"code":""}),
            tools,
            sender,
            &self.inner.options,
            Vec::new(),
            false,
            self.inner.cancel.child_token(),
            sequence,
        );
        let inner = Arc::clone(&self.inner);
        let events = control
            .request(json!({"type":"invoke","id":id,"request":request,"scope":scope}))
            .await?;
        let task = tokio::spawn(async move {
            let outcome = invoke(&inner, &cell, control, events, cancellation).await;
            inner.retire_calls(&cell).await;
            let event = match outcome {
                Ok(value) => json!({"type":"result","result":value}),
                Err(error) => {
                    let fields = error.downcast_ref::<KernelToolError>().map_or_else(|| json!({"code":"kernel_tool_failed","message":error.to_string()}), |error| json!({"code":error.code,"message":error.message,"details":error.details}));
                    json!({"type":"error","error":fields})
                }
            };
            let _ = cell.event(event).await;
        });
        let mut tasks = self.inner.tasks.lock().await;
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
        Ok(receiver)
    }
}

fn response(event: Value) -> Result<Value> {
    if event["ok"] == true {
        return Ok(event["value"].clone());
    }
    Err(KernelToolError {
        code: event["error"]["code"]
            .as_str()
            .unwrap_or("kernel_tool_failed")
            .into(),
        message: event["error"]["message"]
            .as_str()
            .unwrap_or("kernel tool failed")
            .into(),
        details: event["error"].get("details").cloned(),
    }
    .into())
}

async fn invoke(
    session: &Arc<Inner>,
    cell: &Arc<Cell>,
    control: Control,
    mut events: mpsc::Receiver<Value>,
    cancellation: CancellationToken,
) -> Result<Value> {
    let mut calls = tokio::task::JoinSet::new();
    let mut interrupted = false;
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(86400);
    let outcome = async { loop {
        tokio::select! {
            () = async { tokio::select! { () = cell.cancel.cancelled() => {}, () = cancellation.cancelled() => {} } }, if !interrupted => {
                interrupted = true;
                cell.cancel.cancel();
                control.interrupt(&cell.id).await?;
                deadline = tokio::time::Instant::now() + Duration::from_secs(2);
            }
            () = tokio::time::sleep_until(deadline), if interrupted => {
                control.terminate();
                return Err("kernel tool did not acknowledge cancellation; kernel reset".into());
            }
            event = events.recv() => {
                let event = event.ok_or_else(KernelToolError::unavailable)?;
                match event["type"].as_str() {
                    Some("result") => return if interrupted { Err(KernelToolError::cancelled().into()) } else { response(event) },
                    Some("transport-error") => return Err(event["error"].as_str().unwrap_or("kernel transport failed").to_owned().into()),
                    Some("call") => {
                        let (session, cell) = (Arc::clone(session), Arc::clone(cell));
                        calls.spawn(async move { crate::helpers::reply(&session, &cell, &event).await });
                    }
                    _ => cell.accept(event, session.options.settings.status_events).await?,
                }
            }
            reply = calls.join_next(), if !calls.is_empty() => control.send(reply.ok_or("kernel helper stopped")??).await?,
        }
    }}.await;
    calls.abort_all();
    while calls.join_next().await.is_some() {}
    outcome
}
