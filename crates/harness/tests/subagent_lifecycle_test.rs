//! Exercise delegation through the real coordinator, native tools, history and TUI.
use std::{path::Path, sync::Arc, time::Duration};

use async_trait::async_trait;
use harness_core::{
    agent::{AgentModelSettings, AgentProfile},
    clock::RealClock,
    config::{PermissionMode, ShellAllowlist},
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, TaskTerminalScope, ToolCallStatus},
    perm::PermissionPolicy,
    redact::DefaultRedactor,
    store::EventStream,
};
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use harness_tui::{
    app::{AppState, Focus},
    ui::render_app,
};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal, TerminalOptions, Viewport};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Response = mpsc::UnboundedSender<ProviderStreamEvent>;

#[derive(Debug)]
struct GatedProvider(mpsc::UnboundedSender<(CompletionRequest, Response)>);

#[async_trait]
impl Provider for GatedProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        index: usize,
    ) -> std::result::Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, index)
    }

    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(ProviderStreamEvent::Start);
        let _ = self.0.send((request, tx));
        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }
}

fn tool(response: &Response, id: &str, name: &str, args: Value) -> Result<()> {
    response.send(ProviderStreamEvent::ToolCallComplete {
        tool_call_id: id.into(),
        function_name: name.into(),
        arguments_json: args.to_string(),
    })?;
    Ok(())
}
fn done(response: &Response, text: &str) -> Result<()> {
    if !text.is_empty() {
        response.send(ProviderStreamEvent::TextDelta(text.into()))?;
    }
    response.send(ProviderStreamEvent::Done { usage: None })?;
    Ok(())
}
async fn incoming(
    rx: &mut mpsc::UnboundedReceiver<(CompletionRequest, Response)>,
) -> Result<(CompletionRequest, Response)> {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await?
        .ok_or_else(|| "provider channel closed".into())
}

struct History {
    session_path: std::path::PathBuf,
    stream: EventStream,
    events: Vec<EventEnvelopeV1>,
    app: AppState,
}
impl History {
    async fn until(&mut self, predicate: impl Fn(&[EventEnvelopeV1]) -> bool) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !predicate(&self.events) {
                let event = self.stream.next().await.ok_or("event stream closed")??;
                self.app.ingest_event(event.clone());
                self.events.push(event);
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await??;
        Ok(())
    }
    fn output(&self, tool_id: &str) -> Option<&Value> {
        self.events
            .iter()
            .rev()
            .find_map(|event| match &event.payload {
                EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_id => {
                    data.output_json.as_ref()
                }
                _ => None,
            })
    }
    fn capture(&mut self, phase: &str, replay: bool) -> Result<Vec<String>> {
        let original = if replay {
            Some(std::mem::replace(
                &mut self.app,
                AppState::new_replay(self.session_path.clone(), self.events.clone()),
            ))
        } else {
            None
        };
        let mut screens = Vec::new();
        for width in [40, 80, 120] {
            let area = Rect::new(0, 0, width, 45);
            self.app.set_frame_area(area);
            self.app.focus = Focus::Prompt;
            self.app.scroll_goto_bottom();
            let text =
                harness_tui::render_test::render_to_string(&self.app, area, |app, frame, _| {
                    render_app(frame, app)
                });
            assert!(
                !text.contains("QUEUED"),
                "system wakeups must not become user cards: {text}"
            );
            assert_eq!(self.app.canonical_projection_error(), None);
            assert_eq!(self.app.queued_prompt_count, 0);
            if let Some(directory) = std::env::var_os("HARNESS_SUBAGENT_ARTIFACT_DIR") {
                std::fs::create_dir_all(&directory)?;
                let mut bytes = Vec::new();
                let mut terminal = Terminal::with_options(
                    CrosstermBackend::new(&mut bytes),
                    TerminalOptions {
                        viewport: Viewport::Fixed(area),
                    },
                )?;
                terminal.draw(|frame| render_app(frame, &self.app))?;
                drop(terminal);
                std::fs::write(
                    Path::new(&directory)
                        .join(format!("runtime-{phase}-{width}x45-motion-0ms.ansi")),
                    bytes,
                )?;
            }
            screens.push(text);
        }
        if let Some(original) = original {
            self.app = original;
        }
        Ok(screens)
    }
}

fn terminal(events: &[EventEnvelopeV1], request: &str) -> bool {
    events.iter().any(|event| {
        event.correlation_id.as_deref() == Some(request)
            && match &event.payload {
                EventV1::TaskCompleted(data) => {
                    data.metadata.as_ref().and_then(|m| m.task_scope)
                        == Some(TaskTerminalScope::AgentTurn)
                }
                EventV1::TaskCancelled(data) => {
                    data.task_scope == Some(TaskTerminalScope::AgentTurn)
                }
                _ => false,
            }
    })
}

#[tokio::test]
async fn sync_and_async_children_share_durable_lifecycle_and_parent_runtime() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(GatedProvider(tx));
    config.provider_model_concurrency = 2;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    );
    let mut parent = AgentProfile::fallback("default");
    parent.model_ref = "parent:default".into();
    parent.toolset = vec![
        "task".into(),
        "background_output".into(),
        "background_cancel".into(),
    ];
    let mut child = AgentProfile::fallback("explore");
    child.model_ref = "children:worker".into();
    child.model_ref_explicit = true;
    config.agent_profiles = [("default".into(), parent), ("explore".into(), child)].into();
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("delegation-regression", temp.path())
        .await?;
    let store = handle.event_store().await?;
    let mut history = History {
        session_path: run.run_dir.clone(),
        stream: store.subscribe(1)?,
        events: Vec::new(),
        app: AppState::new_live(Some(run.run_dir.clone()), false, None),
    };
    let supervisor = EventActor::new(ActorKind::Supervisor, None);
    let parent_id = handle
        .spawn_agent_idle(supervisor.clone(), "default", None)
        .await?;
    let request_id = handle
        .request_agent_turn_with_model(
            EventActor::new(ActorKind::User, None),
            &parent_id,
            "Inspect three parts of the workspace and report their findings.",
            Some("parent:chosen".into()),
            Some(AgentModelSettings {
                reasoning_effort: Some("low".into()),
                ..Default::default()
            }),
        )
        .await?;
    let (_, response) = incoming(&mut rx).await?;
    for label in ["core", "tools", "ui"] {
        tool(
            &response,
            label,
            "task",
            json!({"description":format!("Inspect {label}"),"prompt":label,
            "subagent_type":"explore","run_in_background":true,"load_skills":[]}),
        )?;
    }
    done(&response, "")?;
    let mut children = Vec::new();
    let mut parent_response = None;
    while children.len() < 2 || parent_response.is_none() {
        let (request, response) = incoming(&mut rx).await?;
        if request.model_id == "chosen" {
            parent_response = Some(response);
        } else {
            children.push(response);
        }
    }
    history.until(|events| events.iter().filter(|event| matches!(&event.payload, EventV1::ToolCallFinished(data) if data.output_json.as_ref().is_some_and(|v| v["mode"] == "background"))).count() == 3).await?;
    let launches: Vec<_> = history
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) => data
                .output_json
                .as_ref()
                .filter(|v| v["mode"] == "background")
                .map(|v| {
                    (
                        data.tool_call_id.to_string(),
                        v["request_id"].as_str().unwrap_or_default().to_string(),
                    )
                }),
            _ => None,
        })
        .collect();
    let request_ids: Vec<_> = launches.iter().map(|(_, id)| id.clone()).collect();
    let screens = history.capture("queued", false)?;
    for text in screens {
        assert!(text.contains("queued:"), "{text}");
        assert!(text.contains("Explore · worker"), "{text}");
    }
    let parent_response = parent_response.ok_or("missing parent continuation")?;
    tool(
        &parent_response,
        "wait-all",
        "background_output",
        json!({"request_ids":request_ids,"wait_mode":"all","block":true,"timeout":600000}),
    )?;
    done(&parent_response, "")?;
    history.until(|events| events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallRequested(data) if data.tool_id == "background_output"))).await?;
    let wait_id = history
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data) if data.tool_id == "background_output" => {
                Some(data.tool_call_id.to_string())
            }
            _ => None,
        })
        .ok_or("missing wait")?;
    let report = format!("{}FULL-REPORT-TAIL", "Child report detail. ".repeat(180));
    done(&children[0], &report)?;
    history
        .until(|events| {
            events
                .iter()
                .any(|event| matches!(event.payload, EventV1::BackgroundTaskNotification(_)))
        })
        .await?;
    for text in history.capture("mixed", false)? {
        assert!(text.contains("completed:"), "{text}");
        assert!(text.contains("running:"), "{text}");
    }
    assert!(
        history.output(&wait_id).is_none(),
        "wait-all must not finish with one child done"
    );
    let (_, third) = incoming(&mut rx).await?;
    let queued_request = history
        .events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if data.state == harness_core::event::TaskScheduleState::Queued
                    && data.queue_key.as_deref() == Some("provider_model:children:worker") =>
            {
                event.correlation_id.clone()
            }
            _ => None,
        })
        .ok_or("missing queued child")?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent_id.clone()));
    handle
        .cancel_background_request(
            actor.clone(),
            Some(queued_request.clone()),
            None,
            "operator stop",
        )
        .await?;
    drop(third);
    done(&children[1], "Second full report")?;
    history.until(|events| events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == wait_id))).await?;
    let output = history.output(&wait_id).ok_or("missing wait result")?;
    assert_eq!(output["satisfied"], true);
    assert_eq!(output["timed_out"], false);
    assert_eq!(output["timeout_ms"], 300000);
    let results = output["results"]
        .as_array()
        .ok_or("missing child results")?
        .clone();
    assert_eq!(results.len(), 3);
    assert!(results.iter().any(|row| row["result_summary"] == report));
    assert_eq!(
        results
            .iter()
            .filter(|row| row["status"] == "cancelled")
            .count(),
        1
    );
    assert!(results
        .iter()
        .all(|row| row["scheduler_task_id"] != row["session_id"]));
    let (parent_request, parent_response) = incoming(&mut rx).await?;
    let model_input = serde_json::to_string(&parent_request.messages)?;
    assert!(
        model_input.contains("FULL-REPORT-TAIL"),
        "wait-all must deliver reports to the provider, not only structured UI metadata"
    );
    assert!(model_input.contains("Second full report"));
    assert!(model_input.contains("operator stop"));
    done(&parent_response, "All child reports collected.")?;
    history
        .until(|events| terminal(events, &request_id))
        .await?;
    // Every automatic continuation retains the selected model; no profile-default switch.
    for _ in 0..3 {
        let (request, response) = incoming(&mut rx).await?;
        assert_eq!(request.model_id, "chosen");
        assert_eq!(request.reasoning_effort.as_deref(), Some("low"));
        done(&response, "Completion acknowledged.")?;
    }
    history
        .until(|events| {
            events
                .iter()
                .filter_map(|event| match &event.payload {
                    EventV1::BackgroundTaskNotification(data) => {
                        data.delivered_turn_request_id.as_deref()
                    }
                    _ => None,
                })
                .all(|id| terminal(events, id))
        })
        .await?;
    history.capture("complete", false)?;
    history.capture("replay", true)?;
    // Continue a completed child synchronously. Earlier completion must not finish this turn.
    let completed = results
        .iter()
        .find(|row| row["status"] == "completed")
        .ok_or("missing completed child")?;
    let session_id = completed["session_id"]
        .as_str()
        .ok_or("missing session id")?
        .to_string();
    let sync_parent = handle
        .request_agent_turn_with_model(
            EventActor::new(ActorKind::User, None),
            &parent_id,
            "Continue the completed child synchronously.",
            Some("parent:chosen".into()),
            Some(AgentModelSettings {
                reasoning_effort: Some("low".into()),
                ..Default::default()
            }),
        )
        .await?;
    let (_, parent_response) = incoming(&mut rx).await?;
    tool(
        &parent_response,
        "sync-continuation",
        "task",
        json!({
            "description":"Continue child", "prompt":"Return another report", "subagent_type":"explore",
            "session_id":session_id, "run_in_background":false,"load_skills":[]
        }),
    )?;
    done(&parent_response, "")?;
    history
        .until(|events| {
            events.iter().any(|event| event.correlation_id.as_deref() == Some(&sync_parent)
        && matches!(&event.payload, EventV1::ToolCallRequested(data) if data.tool_id == "task"))
        })
        .await?;
    let sync_id = history
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data)
                if event.correlation_id.as_deref() == Some(&sync_parent)
                    && data.tool_id == "task" =>
            {
                Some(data.tool_call_id.to_string())
            }
            _ => None,
        })
        .ok_or("missing sync launch")?;
    let (_, sync_response) = incoming(&mut rx).await?;
    history.until(|events| events.iter().any(|event| matches!(&event.payload, EventV1::TaskScheduled(data) if data.metadata.as_ref().and_then(|m|m.lineage.as_ref()).is_some_and(|l| l.parent_tool_call_id.as_deref() == Some(&sync_id)) && data.queue_key.as_deref() == Some("provider_model:children:worker")))).await?;
    assert!(history.output(&sync_id).is_none());
    for text in history.capture("sync-running", false)? {
        assert!(text.contains("Subagent running:"), "{text}");
        assert!(!text.contains("model unknown"), "{text}");
    }
    done(&sync_response, &report)?;
    history.until(|events| events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == sync_id && data.status == ToolCallStatus::Succeeded))).await?;
    let sync = history.output(&sync_id).ok_or("missing sync result")?;
    assert_eq!(sync["result_summary"], report);
    assert_eq!(sync["status"], "completed");
    assert_eq!(sync["session_id"], session_id);
    assert!(!request_ids.iter().any(|id| sync["request_id"] == *id));
    assert_eq!(
        history
            .events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::BackgroundTaskNotification(_)))
            .count(),
        3
    );
    let (parent_request, parent_response) = incoming(&mut rx).await?;
    assert!(parent_request
        .messages
        .iter()
        .any(|message| message.content.contains("FULL-REPORT-TAIL")));
    done(&parent_response, "Synchronous child report received.")?;
    history
        .until(|events| terminal(events, &sync_parent))
        .await?;
    history.capture("sync-complete", false)?;
    // A provider error is a failure in either mode, independent of its prose.
    for (background, cancel) in [(false, false), (true, false), (false, true)] {
        let expected_status = if cancel { "cancelled" } else { "failed" };
        let parent_turn = handle
            .request_agent_turn_with_model(
                EventActor::new(ActorKind::User, None),
                &parent_id,
                "Handle a child that does not complete successfully.",
                Some("parent:chosen".into()),
                Some(AgentModelSettings {
                    reasoning_effort: Some("low".into()),
                    ..Default::default()
                }),
            )
            .await?;
        let (_, parent_response) = incoming(&mut rx).await?;
        tool(
            &parent_response,
            "failing-child",
            "task",
            json!({
                "description":"Failing child", "prompt":"Fail this request", "subagent_type":"explore",
                "run_in_background":background,"load_skills":[]
            }),
        )?;
        done(&parent_response, "")?;
        let mut child_response = None;
        let mut parent_continued = false;
        while child_response.is_none() || (background && !parent_continued) {
            let (request, response) = incoming(&mut rx).await?;
            if request.model_id == "chosen" {
                done(&response, "Background child launched.")?;
                parent_continued = true;
            } else {
                child_response = Some(response);
            }
        }
        let response = child_response.ok_or("missing failing child")?;
        history
            .until(|events| {
                events.iter().any(|event|
            event.correlation_id.as_deref() == Some(&parent_turn)
            && matches!(&event.payload, EventV1::ToolCallRequested(data) if data.tool_id == "task"))
            })
            .await?;
        let task = history
            .events
            .iter()
            .rev()
            .find_map(|event| match &event.payload {
                EventV1::ToolCallRequested(data)
                    if event.correlation_id.as_deref() == Some(&parent_turn)
                        && data.tool_id == "task" =>
                {
                    Some(data.tool_call_id.to_string())
                }
                _ => None,
            })
            .ok_or("missing failing launch")?;
        history
            .until(|events| {
                events.iter().any(|event| {
                    matches!(&event.payload,
            EventV1::TaskScheduled(data) if data.metadata.as_ref().and_then(|m|m.lineage.as_ref())
                .is_some_and(|lineage|lineage.parent_tool_call_id.as_deref() == Some(&task))
                && data.queue_key.as_deref() == Some("provider_model:children:worker"))
                })
            })
            .await?;
        let child_request = history
            .events
            .iter()
            .rev()
            .find_map(|event| match &event.payload {
                EventV1::TaskScheduled(data)
                    if data
                        .metadata
                        .as_ref()
                        .and_then(|m| m.lineage.as_ref())
                        .is_some_and(|lineage| {
                            lineage.parent_tool_call_id.as_deref() == Some(&task)
                        })
                        && data.queue_key.as_deref() == Some("provider_model:children:worker") =>
                {
                    event.correlation_id.clone()
                }
                _ => None,
            })
            .ok_or("missing child request")?;
        let poll = handle
            .request_tool_call(
                actor.clone(),
                Some("default".into()),
                "background_output",
                json!({"request_id":child_request,"block":true,"timeout":0}),
            )
            .await?;
        history.until(|events| events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == poll))).await?;
        let polled = history.output(&poll).ok_or("missing poll result")?;
        assert_eq!(polled["terminal"], false);
        assert_eq!(polled["timed_out"], true);
        assert_eq!(polled["status"], "running");
        if cancel {
            handle
                .cancel_background_request(
                    actor.clone(),
                    Some(child_request.clone()),
                    None,
                    "operator stop",
                )
                .await?;
        } else {
            response.send(ProviderStreamEvent::categorized_error(
                "upstream cancelled response",
                harness_providers::ProviderErrorCategory::InvalidCredentials,
            ))?;
        }
        history
            .until(|events| terminal(events, &child_request))
            .await?;
        let projection = handle
            .background_request_projection(actor.clone(), Some(child_request.clone()), None)
            .await?;
        assert_eq!(projection.status, expected_status);
        assert!(projection.terminal);
        if background {
            history.until(|events| events.iter().any(|event| matches!(&event.payload, EventV1::BackgroundTaskNotification(data) if data.child_request_id == child_request))).await?;
            let (request, response) = incoming(&mut rx).await?;
            assert_eq!(request.model_id, "chosen");
            assert_eq!(request.reasoning_effort.as_deref(), Some("low"));
            done(&response, "Failure acknowledged.")?;
            let wakeup = history
                .events
                .iter()
                .find_map(|event| match &event.payload {
                    EventV1::BackgroundTaskNotification(data)
                        if data.child_request_id == child_request =>
                    {
                        data.delivered_turn_request_id.clone()
                    }
                    _ => None,
                })
                .ok_or("missing failure wakeup")?;
            history.until(|events| terminal(events, &wakeup)).await?;
        } else {
            history.until(|events| events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == task))).await?;
            assert_eq!(
                history.output(&task).ok_or("missing failed sync result")?["status"],
                expected_status
            );
            let (request, response) = incoming(&mut rx).await?;
            let messages = serde_json::to_string(&request.messages)?;
            assert!(messages.contains(if cancel {
                "operator stop"
            } else {
                "upstream cancelled response"
            }));
            done(&response, "Child failure handled.")?;
        }
        history
            .until(|events| terminal(events, &parent_turn))
            .await?;
        history.app.set_frame_area(Rect::new(0, 0, 80, 45));
        history.app.focus = Focus::Details;
        assert!(history.app.select_transcript_tool(&task));
        history.app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('e'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        let screens = history.capture(
            if cancel {
                "cancelled-sync"
            } else if background {
                "failed-async"
            } else {
                "failed-sync"
            },
            false,
        )?;
        for text in screens {
            assert!(
                text.contains(&format!("Subagent {expected_status}:")),
                "{text}"
            );
            assert!(text.contains("Explore · worker"), "{text}");
            assert!(text.contains(&format!("1 {expected_status}")), "{text}");
        }
    }
    let persisted = std::fs::read_to_string(&run.events_path)?;
    let disk: Vec<EventEnvelopeV1> = persisted
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    let mut terminal_requests = std::collections::BTreeSet::new();
    for (index, event) in disk.iter().enumerate() {
        assert_eq!(event.seq, index as u64 + 1);
        let terminal_turn = match &event.payload {
            EventV1::TaskCompleted(data) => {
                data.metadata.as_ref().and_then(|m| m.task_scope)
                    == Some(TaskTerminalScope::AgentTurn)
            }
            EventV1::TaskCancelled(data) => data.task_scope == Some(TaskTerminalScope::AgentTurn),
            _ => false,
        };
        if terminal_turn {
            assert!(
                terminal_requests.insert(event.correlation_id.clone()),
                "duplicate terminal for {:?}",
                event.correlation_id
            );
        }
    }
    drop(history);
    drop(store);
    handle.stop_run().await?;
    handle
        .resume_run(run.run_id.to_string(), "delegation-regression")
        .await?;
    for (_, request) in &launches {
        let projection = handle
            .background_request_projection(actor.clone(), Some(request.clone()), None)
            .await?;
        assert!(projection.terminal);
        if projection
            .result_summary
            .as_deref()
            .is_some_and(|value| value.contains("FULL-REPORT-TAIL"))
        {
            assert_eq!(projection.result_summary.as_deref(), Some(report.as_str()));
        }
    }
    assert!(
        rx.try_recv().is_err(),
        "recovery must not rerun completed children or delivered wakeups"
    );
    handle.stop_run().await?;
    Ok(())
}
