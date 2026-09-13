//! Matched semantic fixtures, ingested by the production AppState and painted by render_app.
use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, LiveEventEnvelope, LiveEventV1, RuntimeEvent,
    SCHEMA_VERSION,
};
use harness_tui::{
    app::{AppState, Focus},
    ui::render_app,
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const FIXTURE: &str = include_str!("../../../scripts/qa/fixtures/tool-order-scenarios.json");

struct Capture {
    app: AppState,
    events: Vec<EventEnvelopeV1>,
    tools: BTreeMap<String, Value>,
    parts: Vec<Value>,
    now_ms: u64,
    live_seq: u64,
    selection_anchor: Option<String>,
    timestamp: Option<String>,
}

impl Capture {
    fn new(fixture: &Value) -> Result<Self> {
        let mut app = AppState::new_live(None, false, None);
        app.restart_motion_epoch_for_evidence();
        let mut state = Self {
            app,
            events: Vec::new(),
            parts: Vec::new(),
            now_ms: 0,
            live_seq: 0,
            selection_anchor: None,
            timestamp: fixture["timestamp"].as_str().map(str::to_string),
            tools: serde_json::from_value(fixture["tools"].clone())?,
        };
        state.event(
            "user_message_submitted",
            json!({"request_id":"turn", "text":fixture["prompt"]}),
        )?;
        state.event(
            "provider_request_started",
            json!({"request_id":"provider", "provider_id":"fixture",
            "model_id":"model", "prompt_summary":fixture["prompt"], "request_digest":"synthetic"}),
        )?;
        Ok(state)
    }

    fn event(&mut self, kind: &str, data: Value) -> Result<()> {
        let seq = self.events.len() as u64 + 1;
        let payload: EventV1 = serde_json::from_value(json!({"event_type":kind, "data":data}))?;
        let event = EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("order-{seq}"),
            seq,
            run_id: "order".into(),
            mono_ms: self.now_ms,
            ts: self.timestamp.clone(),
            actor: EventActor::new(ActorKind::Worker, Some("worker".into())),
            correlation_id: Some("turn".into()),
            causation_id: None,
            stream_key: None,
            payload,
        };
        self.app.ingest_event(event.clone());
        self.events.push(event);
        assert_eq!(self.app.canonical_projection_error(), None);
        Ok(())
    }

    fn live(&mut self, kind: &str, data: Value) -> Result<()> {
        self.live_seq += 1;
        let payload: LiveEventV1 = serde_json::from_value(json!({"event_type":kind, "data":data}))?;
        self.app
            .ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
                event_id: format!("delta-{}", self.live_seq),
                run_id: "order".into(),
                mono_ms: self.now_ms,
                ts: None,
                actor: EventActor::new(ActorKind::Worker, Some("worker".into())),
                correlation_id: Some("turn".into()),
                causation_id: None,
                stream_key: None,
                payload,
            })));
        Ok(())
    }

    fn intent(&mut self, id: &str) {
        if !self.parts.iter().any(|p| p["tool_call_id"] == id) {
            let tool = &self.tools[id];
            self.parts.push(json!({"kind":"tool_call", "tool_call_id":id, "tool_id":tool["tool"],
                "args_summary":tool["args"].to_string(), "args_digest":"synthetic", "provider_call_id":null}));
        }
    }

    fn action(&mut self, action: &Value, fixture: &Value) -> Result<()> {
        let id = action["id"].as_str().unwrap_or("");
        match action["op"].as_str().ok_or("missing op")? {
            "reasoning" => {
                let text = action["text"].as_str().ok_or("reasoning text")?;
                if let Some(part) = self
                    .parts
                    .last_mut()
                    .filter(|part| part["kind"] == "reasoning")
                {
                    part["text"] = json!(format!(
                        "{}{text}",
                        part["text"].as_str().ok_or("reasoning part")?
                    ));
                } else {
                    self.parts.push(json!({"kind":"reasoning", "text":text}));
                }
                self.live(
                    "provider_reasoning_delta",
                    json!({"request_id":"provider", "delta":text}),
                )?;
            }
            "text" => {
                self.parts
                    .push(json!({"kind":"text", "text":action["text"]}));
                self.live(
                    "provider_text_delta",
                    json!({"request_id":"provider", "delta":action["text"]}),
                )?;
            }
            "input" => {
                self.intent(id);
                self.live(
                    "provider_tool_input_delta",
                    json!({"request_id":"provider", "tool_call_id":id,
                    "delta":action["delta"]}),
                )?;
            }
            "request" => {
                self.intent(id);
                let tool = &self.tools[id];
                self.event(
                    "tool_call_requested",
                    json!({"tool_call_id":id, "tool_id":tool["tool"],
                    "args_summary":tool["args"].to_string(), "args_digest":"synthetic",
                    "metadata": if id == "task" { json!({"lineage":{"child_session_id":"child", "child_request_id":"child-request"}}) } else { Value::Null }}),
                )?;
            }
            "start" => {
                self.event(
                    "task_scheduled",
                    json!({"task_id":format!("scheduled-{id}"), "state":"started",
                    "metadata":{"lineage":{"parent_tool_call_id":id}}}),
                )?;
                self.event("tool_call_started", json!({"tool_call_id":id}))?;
            }
            "finish" => {
                let tool = &self.tools[id];
                let failed = action["failed"] == true;
                let output = if failed {
                    "Fixture read denied"
                } else {
                    tool["output"].as_str().ok_or("missing output")?
                };
                let output_json = if tool["kind"] == "read" {
                    json!({"metadata":{"display":{"text":output, "lineStart":1}}})
                } else if tool.get("sources").is_some() {
                    json!({"content":output, "sources":tool["sources"]})
                } else {
                    json!({"result":output})
                };
                let hooks = if action["hooks"] == true {
                    fixture["hooks"]
                        .as_array()
                        .ok_or("hooks array")?
                        .iter()
                        .map(|h| {
                            json!({
                                "hook_name":h["name"], "hook_event":h["phase"],
                                "status":h["status"],
                                "duration_ms":h["duration_ms"], "output_summary":h["output"]
                            })
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let data = json!({"tool_call_id":id, "status":if failed {"failed"} else {"succeeded"},
                    "output_summary":output, "output_json":if failed {Value::Null} else {output_json},
                    "metadata":{"timing":{"elapsed_ms":self.now_ms}, "hook_executions":hooks}});
                self.event("task_completed", json!({"task_id":format!("scheduled-{id}"),
                    "result_summary":"", "result_digest":"synthetic", "metadata":{"task_scope":"tool_call"}}))?;
                self.event("tool_call_finished", data)?;
            }
            "barrier" => {
                self.action(&json!({"op":"provider-finish"}), fixture)?;
                self.action(&json!({"op":"commit"}), fixture)?;
            }
            "provider-finish" => self.event(
                "provider_request_finished",
                json!({"request_id":"provider", "finish_reason":"tool_calls"}),
            )?,
            "commit" => {
                self.event("assistant_message_finished", json!({"request_id":"provider",
                    "tool_call_count":self.parts.iter().filter(|p| p["kind"] == "tool_call").count(), "parts":self.parts}))?;
            }
            "advance" => {
                let ms = action["ms"].as_u64().ok_or("missing ms")?;
                self.now_ms += ms;
                self.app
                    .advance_wall_clock_for_motion_evidence(Duration::from_millis(ms));
                // The production scheduler expires terminal flashes separately from sampling time.
                self.app.refresh_motion_for_evidence();
            }
            "wait" => self.event(
                "permission_requested",
                json!({"permission_id":"permission", "kind":self.tools[id]["tool"],
                "tool_call_id":id, "summary":"Allow the fixture tool", "request_digest":"synthetic",
                "timeout_ms":30000, "default_decision":"deny"}),
            )?,
            "unwait" => self.event(
                "permission_resolved",
                json!({"permission_id":"permission", "decision":"allow"}),
            )?,
            "open" | "fold" => {
                if action["op"] == "fold" {
                    self.app.toggle_tool_output_for_test(id);
                } else {
                    self.app.set_tool_output_expanded_for_test(id, true);
                }
                let viewer_only = matches!(
                    self.tools[id]["tool"].as_str(),
                    Some("fs.read" | "read" | "fs.ls" | "list")
                ) && self.events.iter().any(|event| {
                    matches!(&event.payload,
                        EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == id
                            && data.status == harness_core::event::ToolCallStatus::Failed)
                });
                assert_eq!(self.app.is_tool_output_expanded_for_test(id), !viewer_only);
            }
            "select" => {
                self.app.focus = Focus::Details;
                assert!(self.app.select_transcript_tool(id));
                self.selection_anchor = Some(id.to_string());
            }
            "group" => {
                // Both producers reselect the same semantic anchor after a dense expansion.
                assert!(self.app.select_transcript_tool(
                    self.selection_anchor.as_deref().ok_or("group anchor")?
                ));
                self.app
                    .handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
            }
            "fold-selected" => {
                self.app
                    .handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
            }
            "details-off" => {
                self.app
                    .handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
                for c in "Hide tool details".chars() {
                    self.app
                        .handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
                }
                self.app
                    .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                assert!(!self.app.transcript_interaction_snapshot().show_tool_details);
            }
            "dense" => {
                for i in 0..action["count"].as_u64().ok_or("missing count")? {
                    let id = format!("cmd-{i:02}");
                    self.tools.insert(id.clone(), json!({"tool":"bash", "kind":"execute",
                        "title":format!("Run command {i:02}"), "args":{"command":format!("printf command-{i:02}")},
                        "output":format!("command-{i:02}")}));
                    for op in ["request", "start", "finish"] {
                        self.action(&json!({"op":op, "id":id}), fixture)?;
                    }
                }
            }
            "replay" => {
                let encoded = serde_json::to_string(&self.events)?;
                let events = serde_json::from_str(&encoded)?;
                self.app = AppState::new_replay(
                    Path::new("/tmp/order-fixture-session").to_path_buf(),
                    events,
                );
                self.app.focus = Focus::Prompt;
                self.app.restart_motion_epoch_for_evidence();
                self.app
                    .advance_wall_clock_for_motion_evidence(Duration::from_millis(self.now_ms));
            }
            "lifecycle" => match action["event"].as_str().ok_or("event name")? {
                "session_start" => self.event(
                    "run_started",
                    json!({"run_name":"order", "workspace_root":"/workspace"}),
                )?,
                "session_end" => self.event("run_finished", json!({"summary":"session_end"}))?,
                // No equivalent durable hook lifecycle event exists in EventV1. Never invent one.
                "user_prompt_submit" => {}
                event => return Err(format!("unknown lifecycle {event}").into()),
            },
            "cancel-task" => {
                self.event(
                    "task_cancelled",
                    json!({"task_id":format!("scheduled-{id}"),
                    "reason":"Background task cancelled", "task_scope":"tool_call"}),
                )?;
                self.event("background_task_notification", json!({
                    "parent_session_id":"order", "child_session_id":"child", "child_request_id":"child-request",
                    "task_id":format!("scheduled-{id}"), "description":"Inspect renderer", "status":"cancelled",
                    "summary":"Background task cancelled", "terminal_event_id":"fixture-terminal", "terminal_task_id":format!("scheduled-{id}"),
                    "delivered_turn_request_id":"turn"
                }))?;
            }
            op => return Err(format!("unknown op {op}").into()),
        }
        Ok(())
    }
}

#[test]
fn background_notification_keeps_launch_identity_and_replays_without_a_user_message() -> Result<()>
{
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    for delivery in [Some("turn"), None] {
        let mut state = Capture::new(&fixture)?;
        for action in [
            json!({"op":"request", "id":"task"}),
            json!({"op":"start", "id":"task"}),
            json!({"op":"advance", "ms":430}),
        ] {
            state.action(&action, &fixture)?;
        }
        state.event("background_task_notification", json!({
            "parent_session_id":"order", "child_session_id":"child", "child_request_id":"child-request",
            "task_id":"scheduled-task", "description":"Inspect renderer", "status":"cancelled",
            "summary":"Background task cancelled", "terminal_event_id":"fixture-terminal", "terminal_task_id":"scheduled-task",
            "delivered_turn_request_id":delivery
        }))?;
        for replay in [false, true] {
            if replay {
                state.app = AppState::new_replay(
                    Path::new("/tmp/order-fixture-session").to_path_buf(),
                    state.events.clone(),
                );
            }
            state.app.set_frame_area(Rect::new(0, 0, 120, 40));
            state.app.focus = Focus::Details;
            // The fixture has a user row and at most two collapsed groups.
            // Reach the notification group in both live and replay selection state.
            for _ in 0..3 {
                state
                    .app
                    .handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            }
            state
                .app
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            let buffer = render(&state.app, 120, 40)?;
            let text = buffer
                .content
                .chunks(120)
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("Ran 1 subagent"), "{text}");
            assert!(
                text.contains("Subagent cancelled in 0.4s: “Inspect renderer”"),
                "{text}"
            );
            assert!(!text.contains("Background task cancelled"), "{text}");
            if delivery.is_some() {
                let started = text
                    .find("Subagent started:")
                    .ok_or("missing original launch row")?;
                let finished = text
                    .find("Subagent cancelled")
                    .ok_or("missing terminal row")?;
                assert!(started < finished, "{text}");
            }
            assert_eq!(state.app.canonical_projection_error(), None);
        }
    }
    Ok(())
}

fn render(app: &AppState, width: u16, height: u16) -> Result<Buffer> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| render_app(frame, app))?;
    Ok(terminal.backend().buffer().clone())
}

#[test]
fn reasoning_headers_and_answer_rows_stay_in_place_across_response_commits() -> Result<()> {
    let mut fixture: Value = serde_json::from_str(FIXTURE)?;
    fixture["timestamp"] = json!("2026-09-13T00:43:00Z");
    let headers = [
        "**Planning the inspection**",
        "\n\n**Checking the commands**",
        "\n\n**Verifying the result**",
    ];
    for width in [40, 80, 120] {
        let mut state = Capture::new(&fixture)?;
        state.app.set_frame_area(Rect::new(0, 0, width, 80));
        state.event(
            "task_scheduled",
            json!({"task_id":"turn-task", "state":"started",
            "queue_key":"provider_model:fixture:model"}),
        )?;
        let mut stable_rows = BTreeMap::new();
        for (request, answer, extra_thinking_ms, expected_durations, labels) in [
            ("provider", "Inspect first.", 600, vec!["0.9s"], vec!["Inspect first."]),
            ("provider-next", "Both commands completed.\n\n- Verified the output\n- Checked the workspace\n\nNo other files were modified.", 900,
                vec!["0.9s", "1.2s"], vec!["Inspect first.", "printf alpha", "Both commands"]),
        ] {
            if request == "provider-next" {
                state.action(&json!({"op":"advance", "ms":2000}), &fixture)?;
                state.event("provider_request_started", json!({"request_id":request,
                    "provider_id":"fixture", "model_id":"model", "prompt_summary":"Continue",
                    "request_digest":"synthetic"}))?;
            }
            for delta in headers {
                state.live("provider_reasoning_delta", json!({"request_id":request, "delta":delta}))?;
                state.action(&json!({"op":"advance", "ms":100}), &fixture)?;
            }
            let thinking = render(&state.app, width, 80)?;
            assert!(thinking.content.chunks(usize::from(width)).any(|row|
                row.iter().map(|cell| cell.symbol()).collect::<String>().contains("Thinking…")),
                "each response must show its own live reasoning header");
            state.action(&json!({"op":"advance", "ms":extra_thinking_ms}), &fixture)?;
            state.live("provider_text_delta", json!({"request_id":request, "delta":answer}))?;
            let mut parts = vec![json!({"kind":"reasoning", "text":headers.join("")}), json!({"kind":"text", "text":answer})];
            for phase in ["streaming", "tool-request", "provider-finish", "commit"] {
                state.action(&json!({"op":"advance", "ms":300}), &fixture)?;
                match phase {
                    "tool-request" if request == "provider" => {
                        state.action(&json!({"op":"request", "id":"command-a"}), &fixture)?;
                        parts.extend(state.parts.clone());
                    }
                    "provider-finish" => state.event("provider_request_finished",
                        json!({"request_id":request, "finish_reason":"stop"}))?,
                    "commit" => state.event("assistant_message_finished", json!({"request_id":request,
                        "parts":parts, "tool_call_count":usize::from(request == "provider")}))?,
                    _ => {}
                }
                let buffer = render(&state.app, width, 80)?;
                let rows = buffer.content.chunks(usize::from(width))
                    .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>()).collect::<Vec<_>>();
                let durations = rows.iter().filter_map(|row| row.split_once("Thought for ")
                    .map(|(_, duration)| duration.trim())).collect::<Vec<_>>();
                assert_eq!(durations, expected_durations,
                    "each thought keeps its own frozen duration: {request}, {phase}\n{}", rows.join("\n"));
                for label in &labels {
                    let row = rows.iter().position(|row| row.contains(label))
                        .ok_or_else(|| format!("missing {label}, {request}, {phase}\n{}", rows.join("\n")))?;
                    let expected = stable_rows.entry(*label).or_insert(row);
                    assert_eq!(*expected, row, "{width} columns, {request}, {phase}, {label}\n{}", rows.join("\n"));
                }
            }
            if request == "provider" {
                state.action(&json!({"op":"start", "id":"command-a"}), &fixture)?;
                state.action(&json!({"op":"finish", "id":"command-a"}), &fixture)?;
            }
        }
        // At the tail of an overflowing viewport, the two-row Worked footer
        // takes the live status row and its gap without moving the answer.
        state.app.set_frame_area(Rect::new(0, 0, width, 26));
        let mut completion_rows = None;
        for completed in [false, true] {
            if completed {
                state.event("task_completed", json!({"task_id":"turn-task", "result_summary":"Complete",
                    "result_digest":"synthetic", "metadata":{"task_scope":"agent_turn", "outcome":"completed"}}))?;
            }
            let buffer = render(&state.app, width, 26)?;
            let rows = buffer
                .content
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>();
            let positions = ["Both commands", "No other files"]
                .map(|label| rows.iter().position(|row| row.contains(label)));
            assert!(
                positions.iter().all(Option::is_some),
                "{width} columns\n{}",
                rows.join("\n")
            );
            if let Some(expected) = completion_rows {
                assert_eq!(
                    expected,
                    positions,
                    "completion moved the answer at {width} columns\n{}",
                    rows.join("\n")
                );
                assert!(
                    rows.iter().any(|row| row.contains("Worked for")),
                    "{}",
                    rows.join("\n")
                );
            }
            completion_rows = Some(positions);
        }
    }
    Ok(())
}

#[test]
#[expect(
    clippy::excessive_nesting,
    clippy::cognitive_complexity,
    reason = "one fixture matrix covers lifecycle actions, terminal sizes, and row/motion invariants"
)]
fn production_ordering_capture() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    let directory = std::env::var_os("HARNESS_TOOL_ORDER_ARTIFACT_DIR");
    if let Some(ref directory) = directory {
        fs::create_dir_all(directory)?;
    }
    let mut captures = Vec::new();
    let mut failures = Vec::new();
    let scenario_filter = std::env::var("HARNESS_TOOL_ORDER_FILTER").unwrap_or_default();
    for scenario in fixture["scenarios"]
        .as_array()
        .ok_or("scenarios array")?
        .iter()
        .filter(|scenario| {
            scenario["name"]
                .as_str()
                .is_some_and(|name| name.contains(&scenario_filter))
        })
    {
        let mut state = Capture::new(&fixture)?;
        let mut anchored_rows = BTreeMap::new();
        let mut markers_at_tick = BTreeMap::new();
        for action in scenario["actions"].as_array().ok_or("actions array")? {
            if action["op"] != "snapshot" {
                state.action(action, &fixture)?;
                continue;
            }
            for size in fixture["sizes"].as_array().ok_or("sizes array")? {
                let width = u16::try_from(size[0].as_u64().ok_or("width")?)?;
                let height = u16::try_from(size[1].as_u64().ok_or("height")?)?;
                let name = format!(
                    "order-{}-{}-{width}x{height}-motion-{}ms",
                    scenario["name"].as_str().ok_or("scene name")?,
                    action["name"].as_str().ok_or("snapshot name")?,
                    state.now_ms
                );
                let buffer = render(&state.app, width, height)?;
                assert_eq!(
                    buffer,
                    render(&state.app, width, height)?,
                    "nondeterministic {name}"
                );
                assert_eq!(state.app.canonical_projection_error(), None);
                let text = buffer
                    .content
                    .chunks(width as usize)
                    .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n");
                for (field, present) in [("contains", true), ("absent", false)] {
                    for needle in action[field].as_array().into_iter().flatten() {
                        let needle = needle.as_str().ok_or("expected text")?;
                        if text.contains(needle) != present {
                            failures.push(format!(
                                "{name}: expected {needle:?} present={present}\n{text}"
                            ));
                        }
                    }
                }
                if scenario["name"] == "thought-preview"
                    && action["name"] == "three-headers"
                    && width == 40
                {
                    let preview = text
                        .lines()
                        .map(str::trim)
                        .skip_while(|row| *row != "┃  …")
                        .take(4)
                        .collect::<Vec<_>>();
                    assert_eq!(
                        preview,
                        ["┃  …", "┃", "┃  Planning synthetic status", "┃  checks"],
                        "preview must use Grok's wrapped rows before taking the last three\n{text}"
                    );
                }
                if scenario["name"] == "interleave" && action["name"] == "requested-ab" {
                    assert!(text.contains("Reading 2 files"), "{name}: {text}");
                }
                if scenario["name"] == "dense" && action["name"] == "expanded" {
                    let row = buffer
                        .content
                        .chunks(width as usize)
                        .find(|row| {
                            row.iter()
                                .map(|cell| cell.symbol())
                                .collect::<String>()
                                .contains("Run printf command-11")
                        })
                        .ok_or("missing selected final command")?;
                    let label = row
                        .windows(3)
                        .find(|cells| {
                            cells[0].symbol() == "R"
                                && cells[1].symbol() == "u"
                                && cells[2].symbol() == "n"
                        })
                        .ok_or("missing selected command label")?;
                    assert_eq!(
                        label[0].fg,
                        state.app.theme().text.primary,
                        "selected command must brighten after cached group expansion: {name}"
                    );
                    let command = row
                        .iter()
                        .find(|cell| cell.symbol() == "p")
                        .ok_or("missing command text")?;
                    assert_ne!(
                        command.fg,
                        state.app.theme().text.primary,
                        "selected command must retain its shell syntax colors: {name}"
                    );
                }
                if scenario["name"] == "cancel" && action["name"] == "terminal" {
                    assert!(text.contains("◈ Ran 1 subagent"), "{name}: {text}");
                }
                if scenario["name"] == "read-fold" && action["name"] == "first-fold" {
                    assert!(
                        text.contains("line 05") && text.contains("line 13"),
                        "{text}"
                    );
                    assert!(
                        !text.contains("line 06"),
                        "Read preview must retain only its head and tail: {text}"
                    );
                }
                if scenario["name"] == "hooks" && width == 120 {
                    let required: &[&str] = if action["name"] == "grouped" {
                        &["[hooks: 1 ok, 1 blocked, 1 failed]"]
                    } else {
                        &[
                            "pre_tool_use",
                            "pre-check (12ms)",
                            "pre-skip skipped",
                            "post_tool_use",
                            "post-fail (23ms)",
                            "fixture hook error",
                        ]
                    };
                    for required in required {
                        if !text.contains(required) {
                            failures.push(format!("missing hook metadata {required:?} in {name}"));
                        }
                    }
                }
                if scenario["name"] == "skill" && action["name"] == "members" && width == 120 {
                    let first = text
                        .lines()
                        .find(|line| line.contains("Skill review"))
                        .ok_or("skill member")?;
                    if !first.contains("› Skill review") && !first.contains("> Skill review") {
                        failures.push(format!(
                            "expanded group did not select its first member: {first}"
                        ));
                    }
                }
                if let Some(order) = action["order"].as_array() {
                    let rows = order
                        .iter()
                        .map(|needle| {
                            text.lines()
                                .position(|line| line.contains(needle.as_str().unwrap_or_default()))
                        })
                        .collect::<Vec<_>>();
                    if rows.iter().any(Option::is_none)
                        || !rows.windows(2).all(|pair| pair[0] < pair[1])
                    {
                        failures.push(format!("out-of-order {name}: {rows:?}"));
                    }
                    let anchor = anchored_rows.entry((width, height)).or_insert(rows.clone());
                    if *anchor != rows {
                        failures.push(format!("moving rows in {name}: {anchor:?} -> {rows:?}"));
                    }
                }
                if let Some(labels) = action["same_markers"].as_array() {
                    let colors = labels
                        .iter()
                        .map(|label| {
                            let row = text.lines().position(|line| {
                                line.contains(label.as_str().unwrap_or_default())
                            })?;
                            buffer
                                .content
                                .chunks(usize::from(width))
                                .nth(row)?
                                .iter()
                                .find(|cell| cell.symbol() == "◆")
                                .map(|cell| cell.fg)
                        })
                        .collect::<Vec<_>>();
                    if colors.iter().any(Option::is_none)
                        || !colors.windows(2).all(|pair| pair[0] == pair[1])
                    {
                        failures.push(format!("out-of-phase markers in {name}: {colors:?}"));
                    }
                    let prior = markers_at_tick
                        .entry((width, height, state.now_ms / 33))
                        .or_insert(colors.clone());
                    if *prior != colors {
                        failures.push(format!("wave changed within one tick in {name}"));
                    }
                }
                if let Some(ref directory) = directory {
                    let mut bytes = Vec::new();
                    {
                        let mut terminal = Terminal::with_options(
                            CrosstermBackend::new(&mut bytes),
                            TerminalOptions {
                                viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
                            },
                        )?;
                        terminal.draw(|frame| render_app(frame, &state.app))?;
                    }
                    fs::write(Path::new(directory).join(format!("{name}.ansi")), bytes)?;
                    fs::write(Path::new(directory).join(format!("{name}.txt")), text)?;
                }
                captures
                    .push(json!({"name":name, "audit":action["audit"], "clock_ms":state.now_ms}));
            }
        }
    }
    assert!(!captures.is_empty());
    if let Some(directory) = directory {
        fs::write(
            Path::new(&directory).join("producer.json"),
            serde_json::to_vec_pretty(&json!({
                "entrypoints":["AppState::ingest_event", "AppState::ingest_runtime_event", "AppState::new_replay",
                    "AppState::handle_key", "AppState::toggle_tool_output_for_test", "harness_tui::ui::render_app"],
                "timing":{"mode":"AppState injected now_fn; exact synchronous event transitions", "unit":"milliseconds"},
                "captures":captures,
                "limitations":["user_prompt_submit hook lifecycle has no EventV1 equivalent",
                    "blocked hook retains its explicit recorded outcome",
                    "Read first fold expands full output; no native intermediate Truncated state",
                    "replay exercises valid serialized event ingestion, not on-disk corruption policy"]
            }))?,
        )?;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
