use crate::live::coordinator_warmup::LiveCoordinatorConfigWarmup;
use crate::live::live_settings::{resolve_live_settings, LiveSettings};
use crate::live::workflow::LaunchSelectionState;
use harness_core::config::ConfigLoadContext;
use harness_core::coord::CoordinatorHandle;
use harness_core::event::{EventV1, RuntimeEvent};
use harness_tui::app::LaunchMetadata;
use harness_tui::{LiveUpdate, UiIntent};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

#[path = "gui_server/attachments.rs"]
mod attachments;
#[path = "gui_server/transport.rs"]
mod transport;
pub(crate) use transport::{execute, launch};
#[cfg(test)]
use transport::{serve, write_message};
#[path = "gui_server/access.rs"]
mod access;
use access::{auto_permission, set_access, Access};
#[path = "gui_server/admin.rs"]
mod admin;
#[path = "gui_server/blobs.rs"]
mod blobs;
#[path = "gui_server/catalog.rs"]
mod catalog;
#[path = "gui_server/inspection.rs"]
mod inspection;
#[path = "gui_server/mock_script.rs"]
mod mock_script;
#[path = "gui_server/runtime_ext.rs"]
mod runtime_ext;
#[path = "gui_server/skills_usage.rs"]
mod skills_usage;
#[path = "gui_server/subscription_anthropic.rs"]
mod subscription_anthropic;
#[path = "gui_server/subscription_codex.rs"]
mod subscription_codex;
#[path = "gui_server/wire.rs"]
mod wire;
use crate::live::live_runtime as resume;
#[cfg(test)]
#[path = "gui_server/tests.rs"]
mod tests;

struct Session {
    coordinator: CoordinatorHandle,
    run_dir: PathBuf,
    cwd: PathBuf,
    metadata: LaunchMetadata,
    warmup: LiveCoordinatorConfigWarmup,
    launch_selection: crate::live::workflow::LaunchSelection,
    intents: mpsc::UnboundedSender<UiIntent>,
    /// Access mode in the low byte; the upper bits count changes.
    access: Arc<Access>,
    tasks: Vec<tokio::task::JoinHandle<Result<(), String>>>,
    bridge: tokio::task::JoinHandle<()>,
}

struct Server {
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    config_context: ConfigLoadContext,
    mock: bool,
    mock_script: Option<mock_script::Script>,
    settings: Arc<tokio::sync::Mutex<BTreeMap<PathBuf, LiveSettings>>>,
    sessions: Arc<tokio::sync::Mutex<BTreeMap<String, Session>>>,
    output: mpsc::UnboundedSender<Value>,
    auth_attempt: Arc<Mutex<u64>>,
    auth_sequence: AtomicU64,
}

fn command(mock: bool) -> crate::tui::TuiCommand {
    crate::tui::TuiCommand {
        replay: None,
        continue_session: None,
        scenario: None,
        mock,
        yolo: false,
        deterministic: mock,
        session_dir: None,
        exit_on_finish: false,
        profile: None,
        no_alt_screen: false,
    }
}

impl Server {
    async fn settings(&self, cwd: &Path) -> Result<LiveSettings, String> {
        if !cwd.is_absolute() || !cwd.is_dir() {
            return Err("invalid_params: cwd must be an existing absolute directory".into());
        }
        let mut cached = self.settings.lock().await;
        if let Some(settings) = cached.get(cwd) {
            return Ok(settings.clone());
        }
        let context = self
            .config_context
            .clone()
            .with_current_dir(cwd.to_path_buf());
        let mut settings = resolve_live_settings(
            &command(self.mock),
            self.config_path.clone(),
            self.session_dir.clone(),
            cwd.to_path_buf(),
            &context,
        )?;
        if self.mock_script.is_some() {
            settings.config = crate::cli_config::load_optional_config_with_digest_context(
                self.config_path.as_deref(),
                &context,
            )?
            .map(|loaded| loaded.config);
        }
        runtime_ext::apply_overrides(&mut settings)?;
        cached.insert(cwd.to_path_buf(), settings.clone());
        Ok(settings)
    }

    async fn close(&self, id: &str) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .await
            .remove(id)
            .ok_or("not_found: live session not found")?;
        let result = session
            .coordinator
            .stop_run()
            .await
            .map_err(|e| e.to_string());
        for task in session.tasks {
            task.abort();
        }
        session.bridge.abort();
        let _ = self.output.send(
            json!({"method":"session.update","params":{"sessionId":id,"update":{"kind":"exited"}}}),
        );
        result
    }

    async fn create(&self, params: &Value, continuing: bool) -> Result<Value, String> {
        let mode = params
            .get("accessMode")
            .and_then(Value::as_str)
            .unwrap_or("supervised");
        if !matches!(mode, "supervised" | "autoEdits" | "auto" | "full") {
            return Err("invalid_params: unknown access mode".into());
        }
        // Serialize opening so two requests cannot resume the same journal concurrently.
        let mut sessions = self.sessions.lock().await;
        let run_dir = if continuing {
            Some(PathBuf::from(required(params, "runDir")?))
        } else {
            None
        };
        if run_dir.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err("invalid_params: runDir must be absolute".into());
        }
        let cwd = if let Some(path) = &run_dir {
            let plan = harness_core::proj::inspect_resume_plan(path);
            PathBuf::from(
                plan.workspace_root
                    .ok_or("not_found: session workspace unavailable")?,
            )
        } else {
            PathBuf::from(required(params, "cwd")?)
        };
        if let Some((id, session)) = sessions
            .iter()
            .find(|(_, s)| Some(&s.run_dir) == run_dir.as_ref())
        {
            return Ok(json!({"sessionId":id,"runDir":session.run_dir,"cwd":session.cwd}));
        }
        let mut settings = self.settings(&cwd).await?;
        let mode = if params.get("accessMode").is_none() && settings.yolo {
            "full"
        } else {
            mode
        };
        if let Some(path) = &run_dir {
            settings.session_dir = path
                .parent()
                .ok_or("invalid_params: runDir has no parent")?
                .to_path_buf();
        }
        if !continuing && let Some(worktree) = params.get("worktree") {
            settings = crate::live::new_live::prepare_worktree_live_settings(
                &settings,
                worktree.get("name").and_then(Value::as_str),
                worktree.get("baseBranch").and_then(Value::as_str),
            )?;
        }
        let metadata = if let Some(selection) = params.get("selection") {
            wire::selection(selection, &settings.launch_metadata)?
        } else {
            settings.launch_metadata.clone()
        };
        let selection = Arc::new(Mutex::new(LaunchSelectionState {
            metadata: metadata.clone(),
            config_digest: settings.config_digest.clone(),
        }));
        let mut warmup = LiveCoordinatorConfigWarmup::start(&settings, self.mock);
        if let Some(script) = &self.mock_script {
            warmup = warmup.with_mock_provider(script.provider());
        }
        let (updates, receiver) = harness_tui::live_update_channel();
        let (intents, intent_rx) = mpsc::unbounded_channel();
        let id = if let Some(path) = &run_dir {
            path.file_name()
                .and_then(|s| s.to_str())
                .ok_or("invalid_params: invalid runDir")?
                .to_owned()
        } else {
            crate::live::unique_interactive_run_id()
        };
        let (runtime, metadata) = if let Some(path) = run_dir {
            let (runtime, resumed_metadata, _) = resume::bootstrap(
                &settings,
                self.mock,
                id.clone(),
                path,
                Arc::clone(&selection),
                warmup.clone(),
                updates,
                intent_rx,
            )
            .await?;
            (runtime, resumed_metadata)
        } else {
            let runtime = crate::live::live_runtime::bootstrap_new_live_runtime(
                &settings,
                self.mock,
                settings.workspace_root.clone(),
                id.clone(),
                metadata.clone(),
                warmup.clone(),
                Arc::clone(&selection),
                updates,
                intent_rx,
            )
            .await?;
            (runtime, metadata)
        };
        let info = runtime
            .coordinator
            .run_info()
            .await
            .map_err(|e| e.to_string())?;
        let access = Arc::new(Access::default());
        set_access(&runtime.coordinator, &access, mode).await?;
        let output = self.output.clone();
        let bridge_coordinator = runtime.coordinator.clone();
        let bridge_access = Arc::clone(&access);
        let bridge_id = id.clone();
        let bridge_settings = settings.clone();
        let bridge = tokio::task::spawn_blocking(move || {
            while let Ok(update) = receiver.recv() {
                auto_permission(
                    &bridge_coordinator,
                    &bridge_access,
                    &bridge_settings,
                    &update,
                );
                let session_id = match &update {
                    LiveUpdate::Event(event) => match event.as_ref() {
                        RuntimeEvent::Durable(event) => event.run_id.as_str(),
                        RuntimeEvent::Live(event) => event.run_id.as_str(),
                    },
                    _ => bridge_id.as_str(),
                };
                if output.send(json!({"method":"session.update","params":{"sessionId":session_id,"update":wire::update(update)}})).is_err() { break; }
            }
        });
        let result = json!({"sessionId":id,"runDir":info.run_dir,"cwd":settings.workspace_root});
        sessions.insert(
            id,
            Session {
                coordinator: runtime.coordinator,
                run_dir: info.run_dir,
                cwd: settings.workspace_root,
                metadata,
                warmup,
                launch_selection: selection,
                intents,
                access,
                tasks: vec![runtime.event_forwarder_task, runtime.ui_intent_task],
                bridge,
            },
        );
        Ok(result)
    }

    async fn intent(&self, params: &Value) -> Result<Value, String> {
        let id = required(params, "sessionId")?;
        let value = params
            .get("intent")
            .ok_or("invalid_params: intent is required")?;
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(id)
            .ok_or("not_found: live session not found")?;
        let kind = required(value, "kind")?;
        if kind == "setYolo" {
            let enabled = value["enabled"]
                .as_bool()
                .ok_or("invalid_params: enabled must be a boolean")?;
            set_access(
                &session.coordinator,
                &session.access,
                if enabled { "full" } else { "supervised" },
            )
            .await?;
            return Ok(Value::Null);
        }
        if kind == "setAccessMode" {
            set_access(
                &session.coordinator,
                &session.access,
                required(value, "mode")?,
            )
            .await?;
            return Ok(Value::Null);
        }
        let intent = if kind == "fork" || kind == "clone" {
            let events = crate::cli_io::load_events_from_run_dir(&session.run_dir)?;
            let request = value.get("promptRequestId").and_then(Value::as_str);
            let selected=request.map(|id|events.iter().find(|event| matches!(&event.payload,EventV1::UserMessageSubmitted(message) if message.request_id.as_str()==id)).ok_or("not_found: prompt request not found")).transpose()?;
            let cutoff = selected.map_or_else(
                || events.last().map_or(0, |e| e.seq),
                |selected| {
                    events
                        .iter()
                        .find(|event| {
                            event.seq > selected.seq
                                && matches!(event.payload, EventV1::UserMessageSubmitted(_))
                        })
                        .map_or_else(
                            || events.last().map_or(0, |event| event.seq),
                            |next| next.seq.saturating_sub(1),
                        )
                },
            );
            let prefix = if kind == "clone" {
                harness_core::session_lineage::latest_clone_stable_prefix(&events)
            } else {
                harness_core::session_lineage::validate_tui_fork_stable_prefix(&events, cutoff)
            }
            .map_err(|e| e.to_string())?;
            UiIntent::ForkSession {
                source_run_dir: session.run_dir.clone(),
                events,
                stable_prefix: prefix,
                prompt_text: String::new(),
            }
        } else {
            wire::intent(value, &session.metadata)
                .map_err(|error| format!("invalid_params: {error}"))?
        };
        if let UiIntent::SwitchModel {
            launch_metadata, ..
        } = &intent
        {
            session.metadata = launch_metadata.clone();
        }
        session
            .intents
            .send(intent)
            .map_err(|_| "live session input channel is closed")?;
        Ok(Value::Null)
    }

    async fn handle(&self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "app.initialize" => {
                let cwd = params
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.config_context.discovery.current_dir.clone());
                let settings = self.settings(&cwd).await?;
                Ok(
                    json!({"version":env!("CARGO_PKG_VERSION"),"home":settings.data_dir,"guiStateDir":settings.data_dir.join("gui"),"defaultCwd":self.config_context.discovery.current_dir,"mock":self.mock}),
                )
            }
            "session.create" => self.create(params, false).await,
            "session.continue" => self.create(params, true).await,
            "session.close" => {
                self.close(required(params, "sessionId")?).await?;
                Ok(Value::Null)
            }
            "session.intent" => self.intent(params).await,
            "sessions.attachment" => attachments::read(params).await,
            _ => {
                if matches!(method, "blobs.put" | "blobs.get") {
                    return blobs::dispatch(self, method, params).await;
                }
                if let Some(result) = admin::handle(self, method, params).await {
                    return result;
                }
                if let Some(result) = catalog::handle(self, method, params).await {
                    return result;
                }
                if matches!(
                    method,
                    "skills.list"
                        | "skills.read"
                        | "skills.setEnabled"
                        | "skills.trash"
                        | "usage.history"
                        | "usage.subscription"
                ) {
                    return skills_usage::dispatch(self, method, params).await;
                }
                if matches!(
                    method,
                    "projects.scratch"
                        | "sessions.discoverForeign"
                        | "sessions.importForeign"
                        | "sessions.exportMarkdown"
                        | "sessions.trace"
                        | "sessions.tree"
                        | "worktrees.list"
                        | "worktrees.remove"
                        | "diagnostics.rebuildSessionIndex"
                        | "diagnostics.crashScan"
                        | "diagnostics.refreshModelCatalog"
                        | "providers.protocols"
                        | "config.validate"
                        | "config.explain"
                        | "history.prompts"
                        | "memory.list"
                        | "memory.get"
                        | "memory.delete"
                        | "memory.search"
                        | "plans.list"
                        | "plans.read"
                ) {
                    return inspection::dispatch(self, method, params).await;
                }
                if matches!(
                    method,
                    "llm.complete"
                        | "toggles.get"
                        | "toggles.set"
                        | "trust.check"
                        | "trust.set"
                        | "app.doctor"
                        | "app.configSources"
                        | "app.releaseNotes"
                        | "session.info"
                ) {
                    return runtime_ext::dispatch(self, method, params).await;
                }
                Err(format!("invalid_params: unknown method {method}"))
            }
        }
    }
}

fn required<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("invalid_params: {name} must be a string"))
}
