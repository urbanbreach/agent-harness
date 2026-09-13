use super::{dispatch, test_app_with_agent};
use crate::app::{
    actions::Action,
    agent::AgentId,
    agent_view::{AgentPane, AppRenderParams, BannerSlotParams},
    app_view::{AppView, InputOutcome},
    bundle::BundleState,
};
use crate::scrollback::render::ScratchBuffer;
use crate::theme::{Theme, ThemeKind};
use agent_client_protocol as acp;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect, style::Style,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
use xai_acp_lib::{AcpArgs, AcpClientMessage};
use xai_grok_tools::types::output::{BashOutput, ToolOutput};

type CaptureResult<T> = Result<T, Box<dyn std::error::Error>>;

fn notify(app: &mut AppView, update: acp::SessionUpdate) -> CaptureResult<()> {
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    crate::app::acp_handler::handle(
        AcpClientMessage::SessionNotification(AcpArgs {
            request: acp::SessionNotification::new("test-session", update),
            response_tx: tx,
        }),
        app,
    );
    rx.try_recv()??;
    Ok(())
}

fn draw(app: &mut AppView, area: Rect) -> CaptureResult<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(&mut bytes),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )?;
        let agent = app.agents.get_mut(&AgentId(0)).ok_or("missing agent")?;
        terminal.draw(|frame| {
            let theme = Theme::current();
            frame.buffer_mut().set_style(
                area,
                Style::default().fg(theme.text_primary).bg(theme.bg_base),
            );
            let _ = agent.draw(
                area,
                frame.buffer_mut(),
                &app.registry,
                &mut ScratchBuffer::new(),
                None,
                false,
                BannerSlotParams::none(),
                &BundleState::default(),
                false,
                false,
                &mut Vec::new(),
                AppRenderParams::default(),
            );
        })?;
    }
    Ok(bytes)
}

fn save(out: Option<&Path>, name: &str, bytes: &[u8]) -> CaptureResult<()> {
    if let Some(out) = out {
        fs::write(out.join(format!("{name}.ansi")), bytes)?;
    }
    Ok(())
}

// Replay the same synthetic motion scenario through Grok's real ACP handler and
// AgentPane. Provider commit notifications have no separate ACP equivalent.
#[test]
fn native_reasoning_response_completion_capture() -> CaptureResult<()> {
    let Some(input) = std::env::var_os("HARNESS_REASONING_REFERENCE_INPUT") else {
        return Ok(());
    };
    let out = std::path::PathBuf::from(
        std::env::var_os("HARNESS_REASONING_REFERENCE_DIR").ok_or("output directory")?,
    );
    fs::create_dir_all(&out)?;
    let events: Vec<(u64, Value)> = serde_json::from_slice(&fs::read(input)?)?;
    Theme::apply_kind(ThemeKind::GrokNight);
    let mut captures = Vec::new();
    for height in [26, 40] {
        let area = Rect::new(0, 0, 120, height);
        let mut app = test_app_with_agent();
        for (index, (at, update)) in events.iter().enumerate() {
            let kind = update["event"]["payload"]["event_type"]
                .as_str()
                .ok_or("event type")?;
            let data = &update["event"]["payload"]["data"];
            let chunk = |text: &str| {
                acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)))
            };
            match kind {
                "user_message_submitted" => {
                    notify(
                        &mut app,
                        acp::SessionUpdate::UserMessageChunk(chunk(
                            data["text"].as_str().ok_or("prompt")?,
                        )),
                    )?;
                    let agent = app.agents.get_mut(&AgentId(0)).ok_or("agent")?;
                    agent.session.start_turn(&mut agent.scrollback);
                }
                "provider_reasoning_delta" => notify(
                    &mut app,
                    acp::SessionUpdate::AgentThoughtChunk(chunk(
                        data["delta"].as_str().ok_or("reasoning")?,
                    )),
                )?,
                "provider_text_delta" => notify(
                    &mut app,
                    acp::SessionUpdate::AgentMessageChunk(chunk(
                        data["delta"].as_str().ok_or("text")?,
                    )),
                )?,
                "tool_call_requested" => {
                    let args: Value =
                        serde_json::from_str(data["args_summary"].as_str().ok_or("args")?)?;
                    notify(
                        &mut app,
                        acp::SessionUpdate::ToolCall(
                            acp::ToolCall::new(
                                data["tool_call_id"].as_str().ok_or("id")?.to_owned(),
                                "Bash",
                            )
                            .kind(acp::ToolKind::Execute)
                            .status(acp::ToolCallStatus::InProgress)
                            .raw_input(Some(json!({"variant":"Bash", "command":args["command"]}))),
                        ),
                    )?;
                }
                "tool_call_finished" => notify(
                    &mut app,
                    acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                        data["tool_call_id"].as_str().ok_or("id")?.to_owned(),
                        acp::ToolCallUpdateFields::new()
                            .status(Some(acp::ToolCallStatus::Completed)),
                    )),
                )?,
                "task_completed" if data["metadata"]["task_scope"] == "agent_turn" => {
                    let agent = app.agents.get_mut(&AgentId(0)).ok_or("agent")?;
                    agent.session.finish_turn(&mut agent.scrollback);
                    agent.scrollback.push_block(
                        crate::scrollback::block::RenderBlock::session_event(
                            crate::scrollback::blocks::SessionEvent::TurnCompleted {
                                elapsed: Some(Duration::from_millis(at - 800)),
                            },
                        ),
                    );
                }
                _ => {}
            }
            let name = format!("reasoning-{index:02}-120x{height}-motion-{at}ms");
            save(Some(&out), &name, &draw(&mut app, area)?)?;
            let agent = app.agents.get(&AgentId(0)).ok_or("agent")?;
            let snapshot = agent.scrollback.capture_viewport_snapshot();
            captures.push(json!({"name":name, "clock_ms":at, "event":kind,
                "scroll_top":snapshot.scroll_offset, "viewport_height":snapshot.viewport_height}));
        }
    }
    fs::write(
        out.join("producer.json"),
        serde_json::to_vec_pretty(&json!({
            "captures":captures,
            "entrypoints":["acp_handler::handle", "AgentSession::start_turn", "AgentSession::finish_turn", "AgentPane::draw"],
            "timing":{"mode":"synchronous state transitions; real native animation clock"},
            "limitations":["synthetic content", "provider commits do not exist in ACP", "no provider traffic"]
        }))?,
    )?;
    Ok(())
}

fn scroll_app(config: &Value) -> CaptureResult<AppView> {
    let read = config["family"].as_str() == Some("read");
    let command = config["command"].as_str().ok_or("command")?;
    let description = config["description"].as_str().ok_or("description")?;
    let output = (1..=config["line_count"].as_u64().ok_or("line count")?)
        .map(|i| {
            format!(
                "line_{i:03} {}",
                config["line_suffix"].as_str().unwrap_or("alpha beta gamma")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = test_app_with_agent();
    notify(
        &mut app,
        acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(config["prompt"].as_str().ok_or("prompt")?),
        ))),
    )?;
    notify(
        &mut app,
        acp::SessionUpdate::ToolCall(
            acp::ToolCall::new("runtime-scroll", if read { "/tmp/viewer.txt" } else { description })
                .kind(if read { acp::ToolKind::Read } else { acp::ToolKind::Execute })
                .status(acp::ToolCallStatus::InProgress)
                .raw_input(Some(
                    if read { json!({"variant":"ReadFile", "file_path":"/tmp/viewer.txt", "offset":6, "limit":160}) }
                    else { json!({"variant":"Bash", "command":command, "description":description}) },
                )),
        ),
    )?;
    let result = ToolOutput::Bash(BashOutput {
        output_for_prompt: String::new(),
        output: output.as_bytes().to_vec(),
        exit_code: 0,
        command: command.into(),
        truncated: false,
        signal: None,
        timed_out: false,
        description: Some(description.into()),
        current_dir: "/tmp".into(),
        output_file: String::new(),
        total_bytes: output.len(),
        output_delta: None,
        was_bare_echo: false,
    });
    notify(
        &mut app,
        acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            "runtime-scroll",
            acp::ToolCallUpdateFields::new()
                .status(Some(acp::ToolCallStatus::Completed))
                .raw_output(Some(if read { json!({"type":"ReadFile", "FileContent":{"content":output, "absolute_path":"/tmp/viewer.txt", "offset":6, "limit":160, "raw_output":output, "total_lines":200}}) }
                    else { serde_json::to_value(result)? })),
        )),
    )?;
    assert!(dispatch(Action::ToggleExpandAll, &mut app).is_empty());
    assert!(matches!(
        app.agents.get(&AgentId(0)).ok_or("agent")?.session.state,
        crate::app::agent::AgentState::Idle
    ));
    for entry in app
        .agents
        .get_mut(&AgentId(0))
        .ok_or("agent")?
        .scrollback
        .entries_mut()
    {
        entry.created_at = None;
        entry.finished_at = None;
    }
    Ok(app)
}

#[test]
fn native_tool_scroll_inputs_reach_the_real_dispatcher() -> CaptureResult<()> {
    let config: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../../scripts/qa/fixtures/tool-runtime-scenarios.json"
    )))?;
    let out = std::env::var_os("HARNESS_TOOL_RUNTIME_REFERENCE_DIR").map(std::path::PathBuf::from);
    if let Some(out) = &out {
        fs::create_dir_all(out)?;
    }
    Theme::apply_kind(ThemeKind::GrokNight);
    let mut receipts = Vec::new();
    for size in config["sizes"].as_array().ok_or("sizes")? {
        let width = u16::try_from(size[0].as_u64().ok_or("width")?)?;
        let height = u16::try_from(size[1].as_u64().ok_or("height")?)?;
        let area = Rect::new(0, 0, width, height);
        let mut app = scroll_app(&config)?;
        draw(&mut app, area)?;
        let start = usize::try_from(config["start_offset"].as_u64().ok_or("offset")?)?;
        for key in config["keys"].as_array().ok_or("keys")? {
            let key = key
                .as_str()
                .and_then(|key| key.chars().next())
                .ok_or("key")?;
            app.agents
                .get_mut(&AgentId(0))
                .ok_or("agent")?
                .scrollback
                .set_scroll_offset(start);
            let before_bytes = draw(&mut app, area)?;
            let before = app
                .agents
                .get(&AgentId(0))
                .ok_or("agent")?
                .scrollback
                .capture_viewport_snapshot();
            assert_eq!(
                before.scroll_offset, start,
                "the real rendered tool must overflow"
            );
            if key == 'k' {
                save(
                    out.as_deref(),
                    &format!("runtime-scroll-before-{width}x{height}-motion-0ms"),
                    &before_bytes,
                )?;
            }
            let outcome = app.handle_input(&Event::Key(KeyEvent::new(
                KeyCode::Char(key),
                KeyModifiers::CONTROL,
            )));
            let InputOutcome::Action(action) = outcome else {
                return Err(format!(
                    "Ctrl-{key} did not reach native action dispatch: {outcome:?}"
                )
                .into());
            };
            assert!(dispatch(action, &mut app).is_empty());
            let after = app
                .agents
                .get(&AgentId(0))
                .ok_or("agent")?
                .scrollback
                .scroll_offset();
            let step = if matches!(key, 'k' | 'j') {
                1
            } else {
                usize::from(before.viewport_height / 2)
            };
            let expected = if matches!(key, 'k' | 'u') {
                start - step
            } else {
                start + step
            };
            assert_eq!(after, expected, "native Ctrl-{key} movement");
            let bytes = draw(&mut app, area)?;
            let name = format!("runtime-scroll-{key}-{width}x{height}-motion-0ms");
            save(out.as_deref(), &name, &bytes)?;
            receipts.push(json!({"name":name, "before":start, "after":after, "viewport":before.viewport_height,
                "route":"AppView::handle_input -> app::dispatch::dispatch -> ScrollbackState",
                "producer":"AgentView::draw after native ACP session notification ingestion"}));
        }
    }
    if let Some(out) = &out {
        fs::write(
            out.join("producer.json"),
            serde_json::to_vec_pretty(&receipts)?,
        )?;
        fs::write(
            out.join("runtime-timing.json"),
            serde_json::to_vec_pretty(&json!({
                "clock": "synchronous native input steps; no animation timing assertion",
                "completion_state": "settled, with transient finished_at markers removed",
                "frames": receipts,
            }))?,
        )?;
    }
    Ok(())
}

#[test]
fn native_tool_header_click_capture_uses_the_mouse_handlers_timed_seam() -> CaptureResult<()> {
    let config: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../../scripts/qa/fixtures/tool-runtime-scenarios.json"
    )))?;
    let out = std::env::var_os("HARNESS_TOOL_RUNTIME_REFERENCE_DIR").map(std::path::PathBuf::from);
    if let Some(out) = &out {
        fs::create_dir_all(out)?;
    }
    Theme::apply_kind(ThemeKind::GrokNight);
    let mut receipts = Vec::new();
    for size in config["sizes"].as_array().ok_or("sizes")? {
        let width = u16::try_from(size[0].as_u64().ok_or("width")?)?;
        let height = u16::try_from(size[1].as_u64().ok_or("height")?)?;
        let area = Rect::new(0, 0, width, height);
        let mut app = scroll_app(&config)?;
        draw(&mut app, area)?;
        let agent = app.agents.get_mut(&AgentId(0)).ok_or("agent")?;
        agent.scrollback.goto_top();
        let _ = agent.set_active_pane(AgentPane::Scrollback, false);
        let index = agent.scrollback.len().checked_sub(1).ok_or("tool entry")?;
        draw(&mut app, area)?;
        let start = Instant::now();
        for count in 1..=4_u64 {
            let agent = app.agents.get_mut(&AgentId(0)).ok_or("agent")?;
            // The real mouse handler calls this timed seam and stores its returned
            // click state. Supplying the time explicitly avoids scheduler luck.
            let (last_click, show_tip) = agent.handle_scrollback_click(
                start + Duration::from_millis(count * 10),
                index,
                false,
            );
            agent.last_click = last_click;
            assert!(
                !show_tip,
                "tool headers do not trigger assistant word-selection tips"
            );
            let display_mode = format!(
                "{:?}",
                agent.scrollback.entry(index).ok_or("entry")?.display_mode
            );
            let semantic_count = last_click.map(|(_, _, count)| count);
            let bytes = draw(&mut app, area)?;
            let name = format!("runtime-mouse-header-{count}-{width}x{height}-motion-0ms");
            save(out.as_deref(), &name, &bytes)?;
            receipts.push(json!({
                "name":name, "input_count":count, "relative_click_ms":count * 10,
                "semantic_count":semantic_count, "display_mode":display_mode,
                "route":"AgentView::handle_scrollback_click, including its native counter and caller last_click handoff",
                "scope":"tool-header semantic mouse behavior; OS event decoding is not measured",
            }));
        }
    }
    if let Some(out) = &out {
        fs::write(
            out.join("mouse-producer.json"),
            serde_json::to_vec_pretty(&receipts)?,
        )?;
    }
    Ok(())
}

#[test]
fn native_tool_viewer_capture_uses_the_enter_handler() -> CaptureResult<()> {
    let config: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../../scripts/qa/fixtures/tool-runtime-scenarios.json"
    )))?;
    let out = std::env::var_os("HARNESS_TOOL_RUNTIME_REFERENCE_DIR").map(std::path::PathBuf::from);
    if let Some(out) = &out {
        fs::create_dir_all(out)?;
    }
    Theme::apply_kind(ThemeKind::GrokNight);
    for size in config["sizes"].as_array().ok_or("sizes")? {
        let width = u16::try_from(size[0].as_u64().ok_or("width")?)?;
        let height = u16::try_from(size[1].as_u64().ok_or("height")?)?;
        let area = Rect::new(0, 0, width, height);
        for case in config["viewer_cases"].as_array().ok_or("viewer cases")? {
            let mut inputs = config.clone();
            inputs["line_suffix"] = case["line_suffix"].clone();
            inputs["family"] = case["family"].clone();
            let mut app = scroll_app(&inputs)?;
            draw(&mut app, area)?;
            let agent = app.agents.get_mut(&AgentId(0)).ok_or("agent")?;
            let index = agent.scrollback.len().checked_sub(1).ok_or("tool")?;
            agent.scrollback.set_selected(Some(index));
            let _ = agent.set_active_pane(AgentPane::Scrollback, false);
            let code = KeyCode::Enter;
            let outcome = app.handle_input(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
            if let InputOutcome::Action(action) = outcome {
                assert!(dispatch(action, &mut app).is_empty());
            }
            assert!(
                app.agents
                    .get(&AgentId(0))
                    .ok_or("agent")?
                    .block_viewer
                    .is_some()
            );
            draw(&mut app, area)?;
            for input in case["keys"].as_array().ok_or("keys")? {
                let key = input.as_str().ok_or("key")?;
                let modifiers = if key.starts_with("Ctrl-") {
                    KeyModifiers::CONTROL
                } else {
                    KeyModifiers::NONE
                };
                let code = match key.strip_prefix("Ctrl-").unwrap_or(key) {
                    "Enter" => KeyCode::Enter,
                    "Down" => KeyCode::Down,
                    "PageDown" => KeyCode::PageDown,
                    "End" => KeyCode::End,
                    key => KeyCode::Char(key.chars().next().ok_or("character")?),
                };
                let outcome = app.handle_input(&Event::Key(KeyEvent::new(code, modifiers)));
                if let InputOutcome::Action(action) = outcome {
                    assert!(dispatch(action, &mut app).is_empty());
                }
                draw(&mut app, area)?;
            }
            let name = format!(
                "runtime-viewer-{}-{width}x{height}-motion-0ms",
                case["name"].as_str().ok_or("case")?
            );
            let bytes = draw(&mut app, area)?;
            save(out.as_deref(), &name, &bytes)?;
            for _ in 0..3 {
                if app
                    .agents
                    .get(&AgentId(0))
                    .ok_or("agent")?
                    .block_viewer
                    .is_none()
                {
                    break;
                }
                let code = KeyCode::Esc;
                let outcome =
                    app.handle_input(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
                if let InputOutcome::Action(action) = outcome {
                    assert!(dispatch(action, &mut app).is_empty());
                }
            }
            assert!(
                app.agents
                    .get(&AgentId(0))
                    .ok_or("agent")?
                    .block_viewer
                    .is_none()
            );
        }
    }
    Ok(())
}
