//! Production interaction capture. Requires the existing `test-support` feature solely
//! to install a QuestionViewState and construct an idle AgentView, not to handle input.
use agent_client_protocol as acp;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend, layout::Rect, style::Style, widgets::StatefulWidget, Terminal,
    TerminalOptions, Viewport,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
use xai_grok_pager::{
    acp::{meta::NotificationMeta, tracker::AcpUpdateTracker},
    actions::ActionRegistry,
    app::{
        agent_view::{AgentPane, AgentView, AppRenderParams, BannerSlotParams},
        bundle::BundleState,
    },
    minimal_api,
    scrollback::{render::ScratchBuffer, scrollback_pane::ScrollbackPane, types::DisplayMode},
    theme::{Theme, ThemeKind},
    views::{
        block_viewer::BlockViewerPane,
        permission_view::{PermissionFocus, PermissionViewState},
        question_view::{Question, QuestionViewState},
    },
};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let dir = Path::new(args.get(1).ok_or("output directory required")?);
    let data: Value =
        serde_json::from_slice(&fs::read(args.get(2).ok_or("scenario JSON required")?)?)?;
    fs::create_dir_all(dir)?;
    Theme::apply_kind(ThemeKind::GrokNight);
    let registry = ActionRegistry::defaults();
    let mut receipts = Vec::new();
    for size in data["sizes"].as_array().ok_or("sizes")? {
        let w = size[0].as_u64().ok_or("width")? as u16;
        let h = size[1].as_u64().ok_or("height")? as u16;
        for case in data["cases"].as_array().ok_or("cases")? {
            let name = case["name"].as_str().ok_or("name")?;
            let mut agent = minimal_api::test_agent_view(None, Path::new("/tmp").to_path_buf());
            agent.tip_typing_dismissed = true;
            let mut tracker = AcpUpdateTracker::new();
            let mut outcomes = Vec::new();
            let mut isolated = false;
            let mut blocker = None;
            let mut keyboard = None;
            if case["reference_only"] == true {
                let config = &data["background_viewer"];
                let command = config["command"].as_str().ok_or("background command")?;
                let description = config["description"]
                    .as_str()
                    .ok_or("background description")?;
                assert!(tracker.handle_update(
                    acp::SessionUpdate::ToolCall(acp::ToolCall::new("background-anchor", description)
                        .kind(acp::ToolKind::Execute)
                        .status(acp::ToolCallStatus::InProgress)
                        .raw_input(Some(json!({"variant":"Bash", "command":command, "description":description})))),
                    &NotificationMeta::default(), &mut agent.scrollback));
                // The native viewer takes a central-store stdout snapshot. This exported
                // content producer is exercised separately from the private notification,
                // dock and overlay-installation route; the anchor is NOT a fake BgTask row.
                let entry = agent.scrollback.entry(0).ok_or("background anchor")?;
                let stdout = (1..=config["line_count"]
                    .as_u64()
                    .ok_or("background line count")?)
                    .map(|i| format!("worker_{i:03} alpha beta gamma"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut viewer = BlockViewerPane::for_bg_task(
                    entry.id,
                    config["task_id"].as_str().ok_or("task id")?,
                    &stdout,
                    true,
                );
                draw_background_viewer(&mut viewer, entry, w, h)?;
                let before_follow = viewer.list_state.follow_mode;
                if name.contains("detached") {
                    viewer.handle_scroll(-10);
                    assert!(!viewer.list_state.follow_mode);
                }
                let before_scroll = viewer.list_state.scroll_offset();
                if name.ends_with("finished") {
                    assert!(viewer.tick_bg_task(
                        &format!(
                            "{stdout}\n{}",
                            config["terminal_line"].as_str().ok_or("terminal line")?
                        ),
                        false
                    ));
                    assert!(!viewer.list_state.follow_mode);
                }
                if w == 80 {
                    draw_background_viewer(&mut viewer, entry, 120, h)?;
                }
                let bytes = draw_background_viewer(&mut viewer, entry, w, h)?;
                fs::write(
                    dir.join(format!("interaction-{name}-{w}x{h}-motion-0ms.ansi")),
                    bytes,
                )?;
                receipts.push(json!({"name":name,"width":w,"height":h,"ids":case["ids"],
                    "reference_only":true,"before_follow":before_follow,
                    "before_terminal_scroll":before_scroll,"after_scroll":viewer.list_state.scroll_offset(),
                    "after_follow":viewer.list_state.follow_mode,
                    "producer":"BlockViewerPane::for_bg_task -> handle_scroll/tick_bg_task -> render_content",
                    "blocker":"Content producer only; private task notification, dock and installed overlay route not invoked"}));
                continue;
            }
            match name {
                "question-initial" | "question-choice" | "question-freeform" | "question-unfocused" => {
                    let questions: Vec<Question> =
                        serde_json::from_value(data["question"]["questions"].clone())?;
                    let stash = agent.prompt.stash();
                    minimal_api::set_question_view(
                        &mut agent,
                        Some(QuestionViewState::new("call".into(), questions, stash)),
                    );
                    draw(&mut agent, &registry, w, h, false)?;
                    if name != "question-initial" {
                        outcomes.push(key(
                            &mut agent,
                            &registry,
                            KeyCode::Down,
                            KeyModifiers::NONE,
                        ));
                    }
                    if name == "question-freeform" {
                        outcomes.push(key(
                            &mut agent,
                            &registry,
                            KeyCode::Down,
                            KeyModifiers::NONE,
                        ));
                        outcomes.push(key(
                            &mut agent,
                            &registry,
                            KeyCode::Enter,
                            KeyModifiers::NONE,
                        ));
                        for c in data["freeform"].as_str().ok_or("freeform")?.chars() {
                            let _ =
                                key(&mut agent, &registry, KeyCode::Char(c), KeyModifiers::NONE);
                        }
                    }
                    if name == "question-unfocused" {
                        outcomes.push(key(&mut agent, &registry, KeyCode::Esc, KeyModifiers::NONE));
                    }
                    assert_eq!(
                        minimal_api::question_view(&agent)
                            .ok_or("question vanished")?
                            .cursor(),
                        match name {
                            "question-initial" => 0,
                            "question-freeform" => 2,
                            _ => 1,
                        }
                    );
                }
                _ if name.starts_with("permission-") => {
                    let options = vec![
                        acp::PermissionOption::new(
                            "enable-always-approve",
                            "Yes, and don't ask again for anything (always-approve mode)",
                            acp::PermissionOptionKind::AllowOnce,
                        ),
                        acp::PermissionOption::new(
                            // The long-description cases exercise an ordinary collapsible
                            // argument prompt. The reserved edit ID disables Ctrl-F for
                            // protected-edit warnings, so it only belongs to those cases.
                            if case.get("permission_description").is_some() { "allow-session" } else { "allow-edits-session" },
                            "Yes, allow all edits during this session",
                            acp::PermissionOptionKind::AllowAlways,
                        ),
                        acp::PermissionOption::new(
                            "allow",
                            "Yes",
                            acp::PermissionOptionKind::AllowOnce,
                        ),
                        acp::PermissionOption::new(
                            "deny",
                            "Reject",
                            acp::PermissionOptionKind::RejectOnce,
                        ),
                    ];
                    let (tx, _rx) = tokio::sync::oneshot::channel();
                    agent.permission_queue.push_back(PermissionViewState {
                        request: xai_acp_lib::AcpArgs {
                            request: acp::RequestPermissionRequest::new(
                                "interaction",
                                acp::ToolCallUpdate::new("call", acp::ToolCallUpdateFields::new()),
                                options.clone(),
                            ),
                            response_tx: tx,
                        },
                        id: 0,
                        focus: PermissionFocus::Options,
                        options,
                        active_idx: 0,
                        bash_highlights: None,
                        bash_selection_count: 0,
                        bash_deny_selection_count: 0,
                        bash_command_raw: None,
                        mcp_scope: None,
                        // Match the supplied Harness permission title, description, and option
                        // payload; the native view owns their layout and input handling.
                        title: "Allow Shell?".into(),
                        description: case["permission_description"].as_array().map_or_else(
                            || vec!["Check terminal output".into(), "printf ready".into()],
                            |lines| lines.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
                        ),
                        args_expanded: false,
                        desc_scroll: 0,
                        subagent_label: None,
                        options_area_height: 0,
                        options_scroll_offset: 0,
                    });
                    draw(&mut agent, &registry, w, h, false)?;
                    for step in case["permission_keys"].as_array().ok_or("permission keys")? {
                        let step = step.as_str().ok_or("permission key")?;
                        let text = if step == "feedback" {
                            Some(data["freeform"].as_str().ok_or("freeform")?)
                        } else {
                            step.strip_prefix("text:")
                        };
                        if let Some(text) = text {
                            for c in text.chars() {
                                outcomes.push(key(&mut agent, &registry, KeyCode::Char(c), KeyModifiers::NONE));
                            }
                        } else {
                            let (code, modifiers) = match step {
                                "Down" => (KeyCode::Down, KeyModifiers::NONE),
                                "Up" => (KeyCode::Up, KeyModifiers::NONE),
                                "Esc" => (KeyCode::Esc, KeyModifiers::NONE),
                                "Backspace" => (KeyCode::Backspace, KeyModifiers::NONE),
                                "Ctrl-f" => (KeyCode::Char('f'), KeyModifiers::CONTROL),
                                _ => return Err(format!("unknown permission key {step}").into()),
                            };
                            outcomes.push(key(&mut agent, &registry, code, modifiers));
                        }
                        draw(&mut agent, &registry, w, h, false)?;
                    }
                    assert_eq!(agent.permission_queue.len(), 1);
                    if name == "permission-feedback-expanded" {
                        assert!(agent.permission_queue[0].args_expanded);
                    }

                }
                _ if name.starts_with("todos-") => {
                    let todos = case.get("todo_items").unwrap_or(&data["todos"]);
                    // Native Plan handler ultimately calls this public producer. The tracker
                    // deliberately ignores Plan; do not substitute a generic ToolCallBlock.
                    let update = acp::SessionUpdate::ToolCall(
                        acp::ToolCall::new("todo", "TodoWrite")
                            .kind(acp::ToolKind::Other)
                            .raw_input(Some(json!({"variant":"TodoWrite", "todos":todos}))),
                    );
                    assert!(!tracker.handle_update(
                        update,
                        &NotificationMeta::default(),
                        &mut agent.scrollback
                    ));
                    agent
                        .todo
                        .update_todos(serde_json::from_value(todos.clone())?);
                    outcomes.push(key(&mut agent, &registry, KeyCode::Char('t'), KeyModifiers::CONTROL));
                    draw(&mut agent, &registry, w, h, false)?;
                    if name.ends_with("filtered") {
                        assert!(agent
                            .todo
                            .handle_key(&KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)));
                        assert!(!agent.todo.show_done());
                    }
                    if let Some(steps) = case["todo_keys"].as_array() {
                        for step in steps {
                            let step = step.as_str().ok_or("todo key")?;
                            let codes = if let Some(text) = step.strip_prefix("text:") {
                                text.chars().map(KeyCode::Char).collect::<Vec<_>>()
                            } else {
                                vec![match step {
                                    "Down" => KeyCode::Down,
                                    "Enter" => KeyCode::Enter,
                                    "Esc" => KeyCode::Esc,
                                    "Tab" => KeyCode::Tab,
                                    text if text.chars().count() == 1 => KeyCode::Char(text.chars().next().ok_or("key")?),
                                    _ => return Err(format!("unknown todo key {step}").into()),
                                }]
                            };
                            for code in codes {
                                outcomes.push(key(&mut agent, &registry, code, KeyModifiers::NONE));
                                draw(&mut agent, &registry, w, h, false)?;
                            }
                        }
                    }
                }
                "question-resolved" | "tool-running" | "tool-finished" => {
                    let question = name.starts_with("question");
                    let call = acp::ToolCall::new("call", if question { "AskUserQuestion" } else { "Check terminal output" })
                        .kind(if question { acp::ToolKind::Other } else { acp::ToolKind::Execute })
                        .status(acp::ToolCallStatus::InProgress)
                        .raw_input(Some(if question { data["resolved_question"].clone() } else { json!({"variant":"Bash", "command":"printf ready", "description":"Check terminal output"}) }));
                    assert!(tracker.handle_update(
                        acp::SessionUpdate::ToolCall(call),
                        &NotificationMeta::default(),
                        &mut agent.scrollback
                    ));
                    if name != "tool-running" {
                        let text = if question {
                            data["resolved"].as_str().ok_or("resolved")?
                        } else {
                            "ready"
                        };
                        let raw_output = if question {
                            None
                        } else {
                            use xai_grok_tools::types::output::{BashOutput, ToolOutput};
                            Some(serde_json::to_value(ToolOutput::Bash(BashOutput {
                                output_for_prompt: String::new(),
                                output: text.as_bytes().to_vec(),
                                exit_code: 0,
                                command: "printf ready".into(),
                                truncated: false,
                                signal: None,
                                timed_out: false,
                                description: None,
                                current_dir: "/tmp".into(),
                                output_file: String::new(),
                                total_bytes: text.len(),
                                output_delta: None,
                                was_bare_echo: false,
                            }))?)
                        };
                        assert!(tracker.handle_update(
                            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                                "call",
                                acp::ToolCallUpdateFields::new()
                                    .raw_output(raw_output)
                                    .status(Some(acp::ToolCallStatus::Completed))
                                    .content(Some(vec![acp::ToolCallContent::from(
                                        acp::ContentBlock::Text(acp::TextContent::new(text))
                                    )]))
                            )),
                            &NotificationMeta::default(),
                            &mut agent.scrollback
                        ));
                    }
                    for entry in agent.scrollback.entries_mut() {
                        entry.created_at = None;
                        entry.finished_at = entry
                            .finished_at
                            .map(|_| Instant::now() - Duration::from_millis(4000));
                        // Only clocks are normalized after production ingestion. Preserve the
                        // tracker-chosen specialized block, status, output and layout.
                        if let xai_grok_pager::scrollback::block::RenderBlock::ToolCall(
                            xai_grok_pager::scrollback::blocks::tool::ToolCallBlock::Execute(tool),
                        ) = &mut entry.block
                        {
                            tool.started_at = None;
                            tool.elapsed_ms = Some(0);
                            if name == "tool-finished" {
                                assert_eq!(tool.output.as_deref(), Some("ready"));
                            }
                        }
                        entry.set_display_mode(DisplayMode::Expanded);
                    }
                    isolated = true;
                }
                _ if case["focus"].is_string() => {
                    let draft = data["keyboard_draft"]["text"].as_str().ok_or("draft")?;
                    let cursor =
                        data["keyboard_draft"]["cursor"].as_u64().ok_or("cursor")? as usize;
                    let composer = case["focus"] == "composer";
                    agent.prompt.set_text(draft);
                    agent.prompt.set_cursor(cursor);
                    agent.active_pane = if composer {
                        AgentPane::Prompt
                    } else {
                        AgentPane::Scrollback
                    };
                    for entry in agent.scrollback.entries_mut() {
                        entry.created_at = None;
                    }
                    // Paint the actual focus and retain the inactive draft in transcript probes.
                    draw(&mut agent, &registry, w, h, false)?;
                    let before = json!({"focus":format!("{:?}",agent.active_pane),
                        "prompt":agent.prompt.text(),"cursor":agent.prompt.cursor(),
                        "scroll":agent.scrollback.scroll_offset()});
                    let c = case["key"]
                        .as_str()
                        .and_then(|s| s.chars().next())
                        .ok_or("key")?;
                    outcomes.push(key(
                        &mut agent,
                        &registry,
                        KeyCode::Char(c),
                        KeyModifiers::CONTROL,
                    ));
                    if !composer {
                        assert_eq!(agent.prompt.text(), draft);
                        assert_eq!(agent.prompt.cursor(), cursor);
                        blocker = Some(
                            "AgentView::handle_input returns Action; app::dispatch is private. No imitation action reducer applied. Frame is pre-dispatch, not scroll-motion evidence.",
                        );
                    }
                    keyboard = Some(json!({"requested_focus":case["focus"],"key":case["key"],
                        "before":before,"after":{"focus":format!("{:?}",agent.active_pane),
                        "prompt":agent.prompt.text(),"cursor":agent.prompt.cursor(),
                        "scroll":agent.scrollback.scroll_offset()},
                        "entrypoint":"AgentView::handle_input (public); full-TUI prompt-paging wrapper is crate-private"}));
                }
                _ => return Err(format!("unknown case {name}").into()),
            }
            if w == 80 {
                draw(&mut agent, &registry, 120, h, isolated)?;
            }
            let bytes = draw(&mut agent, &registry, w, h, isolated)?;
            fs::write(
                dir.join(format!("interaction-{name}-{w}x{h}-motion-0ms.ansi")),
                bytes,
            )?;
            receipts.push(json!({"name":name,"width":w,"height":h,"ids":case["ids"],"input_outcomes":outcomes,"blocker":blocker,
                "question_cursor":minimal_api::question_view(&agent).map(|q| q.cursor()),
                "question_focus":minimal_api::question_view(&agent).map(|q| format!("{:?}",q.focus)),
                "prompt":agent.prompt.text(),"scroll_offset":agent.scrollback.scroll_offset(),"keyboard":keyboard}));
        }
    }
    fs::write(
        dir.join("states.json"),
        serde_json::to_vec_pretty(&receipts)?,
    )?;
    fs::write(
        dir.join("producer.json"),
        serde_json::to_vec_pretty(&json!({
        "entrypoints":["AcpUpdateTracker::handle_update", "ScrollbackPane::render (StatefulWidget)", "AgentView::handle_input", "AgentView::draw", "TodoPane::update_todos", "TodoPane::handle_key", "BlockViewerPane::for_bg_task", "BlockViewerPane::tick_bg_task", "BlockViewerPane::render_content"],
        "timing":{"animation_tick":0,"terminal_age_ms":4000,"execute_elapsed_ms":0,"scope":"public entry finish clock aged beyond flash; tracker-produced execute clock frozen without changing its payload"},
        "setup":"Existing test-support installs native question state; all input and rendering use production methods. No provider/network execution."}))?,
    )?;
    println!("Captured {} reference interaction frames", receipts.len());
    Ok(())
}

fn draw_background_viewer(
    viewer: &mut BlockViewerPane,
    entry: &xai_grok_pager::scrollback::entry::ScrollbackEntry,
    w: u16,
    h: u16,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    let area = Rect::new(0, 0, w, h);
    {
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(&mut bytes),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )?;
        terminal.draw(|frame| {
            let theme = Theme::current();
            frame.buffer_mut().set_style(
                area,
                Style::default().fg(theme.text_primary).bg(theme.bg_base),
            );
            // Native content only, reserving the scrollbar columns its producer requires.
            viewer.render_content(
                Rect::new(0, 0, w.saturating_sub(2), h),
                frame.buffer_mut(),
                entry,
                true,
                &[],
            );
        })?;
    }
    Ok(bytes)
}

fn key(
    agent: &mut AgentView,
    registry: &ActionRegistry,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> String {
    format!(
        "{:?}",
        agent.handle_input(&Event::Key(KeyEvent::new(code, modifiers)), registry)
    )
}

fn draw(
    agent: &mut AgentView,
    registry: &ActionRegistry,
    w: u16,
    h: u16,
    isolated: bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    let area = Rect::new(0, 0, w, h);
    {
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(&mut bytes),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )?;
        terminal.draw(|frame| {
            let theme = Theme::current();
            frame.buffer_mut().set_style(
                area,
                Style::default().fg(theme.text_primary).bg(theme.bg_base),
            );
            if isolated {
                agent.scrollback.prepare_layout(w, h);
                ScrollbackPane::new().active(true).render(
                    area,
                    frame.buffer_mut(),
                    &mut agent.scrollback,
                );
            } else {
                let _ = agent.draw(
                    area,
                    frame.buffer_mut(),
                    registry,
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
            }
        })?;
    }
    Ok(bytes)
}
