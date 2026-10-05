use crate::{output::Output, Result, SessionOptions};
use base64::Engine;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, Mutex, Notify};
use tokio_util::sync::CancellationToken;

pub(crate) struct Cell {
    pub id: String,
    pub language: String,
    pub args: Value,
    pub tools: Mutex<Value>,
    pub tools_changed: std::sync::atomic::AtomicBool,
    pub state: Mutex<State>,
    pub events: mpsc::Sender<Value>,
    pub cancel: CancellationToken,
    pub steer: CancellationToken,
    pub finished: Notify,
    pub created: tokio::time::Instant,
    pub submitted_at: u128,
    pub interactive: bool,
}

pub(crate) struct State {
    pub status: &'static str,
    pub detached: bool,
    pub completed: bool,
    pub started_at: Option<u128>,
    pub queued: Vec<String>,
    pub runtime: Value,
    pub output: Output,
    pub images: Vec<Value>,
    pub image_bytes: usize,
    pub images_elided: usize,
    pub json_elided: usize,
    pub json: Vec<Value>,
    pub markdown: bool,
    pub phase: Option<String>,
    pub statuses: Vec<Value>,
    pub tool_calls: crate::metadata::ToolCalls,
    pub duration: Duration,
    pub busy: usize,
    pub running_since: Option<tokio::time::Instant>,
    pub blocked_since: Option<tokio::time::Instant>,
    pub blocked_time: Duration,
    pub result: Option<Value>,
    pub memory: Option<Value>,
}

impl Cell {
    pub fn new(
        id: String,
        args: Value,
        tools: Value,
        events: mpsc::Sender<Value>,
        options: &SessionOptions,
        queued: Vec<String>,
        interactive: bool,
        cancel: CancellationToken,
        sequence: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            language: args["language"].as_str().unwrap_or_default().into(),
            id,
            args,
            tools: Mutex::new(tools),
            tools_changed: std::sync::atomic::AtomicBool::new(false),
            events,
            cancel,
            steer: CancellationToken::new(),
            finished: Notify::new(),
            created: tokio::time::Instant::now(),
            submitted_at: epoch_ms(),
            interactive,
            state: Mutex::new(State {
                status: "queued",
                detached: false,
                completed: false,
                started_at: None,
                queued,
                runtime: Value::Null,
                output: Output::new(
                    options.artifacts.join(format!("cell-{sequence}.txt")),
                    options.settings.output_head_bytes,
                    options.settings.output_max_columns,
                ),
                images: Vec::new(),
                image_bytes: 0,
                images_elided: 0,
                json_elided: 0,
                json: Vec::new(),
                markdown: false,
                phase: None,
                statuses: Vec::new(),
                tool_calls: crate::metadata::ToolCalls::default(),
                duration: Duration::ZERO,
                busy: 0,
                running_since: None,
                blocked_since: None,
                blocked_time: Duration::ZERO,
                result: None,
                memory: None,
            }),
        })
    }

    pub async fn wait_terminal(&self) {
        loop {
            let finished = self.finished.notified();
            if self.state.lock().await.completed {
                return;
            }
            finished.await;
        }
    }

    pub async fn event(&self, mut event: Value) -> Result<()> {
        if event.get("id").is_none() {
            event["id"] = self.id.clone().into();
        }
        self.events
            .send(event)
            .await
            .map_err(|_| "eval result receiver closed".into())
    }

    pub async fn accept(&self, event: Value, status_events: bool) -> Result<()> {
        let mut state = self.state.lock().await;
        match event["type"].as_str() {
            Some("text") => state
                .output
                .push(event["data"].as_str().unwrap_or_default(), true)?,
            Some("phase") => state.phase = event["title"].as_str().map(str::to_owned),
            Some("log") => state.output.push(
                &format!("{}\n", event["message"].as_str().unwrap_or_default()),
                true,
            )?,
            Some("status") if status_events => state.status(event["event"].clone()),
            Some("display") => state.display(&event)?,
            _ => {}
        }
        Ok(())
    }

    pub async fn result(&self, live: bool) -> Value {
        let state = self.state.lock().await;
        if let Some(result) = &state.result {
            return Self::hydrate(result.clone());
        }
        self.snapshot(&state, live)
    }

    pub fn snapshot(&self, state: &State, live: bool) -> Value {
        let (output, meta) = state.output.snapshot();
        let output = output.trim_end().to_owned();
        let error = matches!(state.status, "error" | "cancelled");
        let mut cell = json!({"index":0,"summary":self.args["summary"],"code":self.args["code"],
            "language":self.language,"runtime":state.runtime,"output":if live {state.output.preview()} else {output.clone()},
            "status":state.status,"durationMs":state.duration.as_millis()});
        if let Some(start) = state.started_at {
            cell["startedAt"] = json!(start);
        }
        if !state.queued.is_empty() {
            cell["queuedBehind"] = json!(state.queued);
        }
        if state.markdown {
            cell["hasMarkdown"] = true.into();
        }
        if !state.statuses.is_empty() {
            cell["statusEvents"] = json!(state.statuses);
        }
        let mut details = json!({"language":self.language,"languages":[self.language],"runtime":state.runtime,
            "summary":self.args["summary"],"durationMs":state.duration.as_millis(),"wallDurationMs":self.created.elapsed().as_millis(),
            "toolCallCount":state.tool_calls.count,"toolCalls":state.tool_calls.rows,"truncated":meta.as_ref().is_some_and(|meta| meta["truncatedBy"].is_string()),"cells":[cell],"cell_id":self.id});
        if error {
            details["isError"] = true.into();
        }
        if let Some(phase) = &state.phase {
            details["phase"] = phase.clone().into();
        }
        if !state.statuses.is_empty() {
            details["statusEvents"] = json!(state.statuses);
        }
        if !state.json.is_empty() {
            details["jsonOutputs"] = json!(state.json);
        }
        if let Some(memory) = &state.memory {
            details["memory"] = memory.clone();
        }
        let mut shown = if live {
            format!(
                "1/1 cells {}\n[1] {} {} {}\n{}",
                state.status,
                self.language,
                self.args["summary"].as_str().unwrap_or_default(),
                state.status,
                state.output.preview()
            )
        } else if output.is_empty() {
            if state.images.is_empty() {
                "(no output)".to_owned()
            } else {
                format!(
                    "(displayed {} image{}; no text output)",
                    state.images.len(),
                    if state.images.len() == 1 { "" } else { "s" }
                )
            }
        } else {
            output
        };
        if let Some(meta) = meta {
            if !live {
                if meta["truncatedBy"].is_string() {
                    shown.push_str(&format!(
                        "\n[Output truncated: kept {} of {} bytes.]",
                        meta["outputBytes"], meta["totalBytes"]
                    ));
                }
                if let Some(path) = meta["artifactId"].as_str() {
                    shown.push_str(&format!("\n[Full output: {path}]"));
                }
            }
            details["meta"] = meta;
        }
        json!({"content":state.content(shown, live),"details":details})
    }
}

impl State {
    fn status(&mut self, event: Value) {
        if event["op"] == "agent"
            && event["id"].is_string()
            && let Some(previous) = self
                .statuses
                .iter_mut()
                .find(|previous| previous["op"] == "agent" && previous["id"] == event["id"])
        {
            *previous = event;
            return;
        }
        self.statuses.push(event);
        if self.statuses.len() <= 100 {
            return;
        }
        let count = self.statuses[0]["count"]
            .as_u64()
            .filter(|_| self.statuses[0]["op"] == "status-events-omitted")
            .unwrap_or(0);
        self.statuses.drain(..2);
        self.statuses.insert(
            0,
            json!({"op":"status-events-omitted","count":count + if count == 0 { 2 } else { 1 }}),
        );
    }

    fn content(&self, shown: String, live: bool) -> Vec<Value> {
        let mut content = vec![json!({"type":"text","text":shown})];
        if !live {
            content.extend(self.images.clone());
            if let Some(notice) = self
                .memory
                .as_ref()
                .and_then(|memory| memory["notice"].as_str())
            {
                content.push(json!({"type":"text","text":notice}));
            }
        }
        content
    }
    pub fn own_time(&self) -> Duration {
        self.running_since
            .map_or(Duration::ZERO, |start| start.elapsed())
            .saturating_sub(self.blocked_time)
            .saturating_sub(
                self.blocked_since
                    .map_or(Duration::ZERO, |start| start.elapsed()),
            )
    }

    fn display(&mut self, event: &Value) -> Result<()> {
        let mime = event["mimeType"].as_str().unwrap_or_default();
        let data = event["dataBase64"]
            .as_str()
            .ok_or("display data is missing")?;
        if mime.starts_with("image/") {
            if self.images.len() >= 8 || self.image_bytes + data.len() > 24 * 1024 * 1024 {
                self.images_elided += 1;
            } else {
                self.image_bytes += data.len();
                self.images
                    .push(json!({"type":"image","data":data,"mimeType":mime}));
            }
        } else {
            let bytes = base64::prelude::BASE64_STANDARD.decode(data)?;
            let text = String::from_utf8(bytes)?;
            if mime == "application/json" {
                if self.json.len() >= 64 {
                    self.json_elided += 1;
                    return Ok(());
                }
                let value: Value = serde_json::from_str(&text)?;
                let count = self.json.len() + 1;
                let text = serde_json::to_string_pretty(&value)?;
                let length = text.chars().count();
                let text = if length > 8000 {
                    format!(
                        "{}\n[…{}ch elided…]",
                        text.chars().take(8000).collect::<String>(),
                        length - 8000
                    )
                } else {
                    text
                };
                self.output
                    .push(&format!("display[{count}]:\n{text}\n"), true)?;
                self.json.push(value);
            } else {
                self.markdown |= mime == "text/markdown";
                self.output.push(
                    &format!("{text}{}", if text.ends_with('\n') { "" } else { "\n" }),
                    true,
                )?;
            }
        }
        Ok(())
    }
}

pub(crate) fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
