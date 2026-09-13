//! Deterministic synthetic payloads through the public event, input and renderer boundaries.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::*;
use harness_tui::{
    app::{AppState, Focus},
    keybindings::KeyMap,
    ui::render_app,
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};

fn ingest(app: &mut AppState, seq: &mut u64, payload: EventV1) {
    app.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("interaction-{seq}"),
        seq: *seq,
        run_id: "interaction".into(),
        mono_ms: *seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("worker".into())),
        correlation_id: Some("turn".into()),
        causation_id: None,
        stream_key: None,
        payload,
    });
    *seq += 1;
}
fn key(app: &mut AppState, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, modifiers));
}
fn tool(app: &mut AppState, seq: &mut u64, id: &str, args: Value) {
    ingest(
        app,
        seq,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "call".into(),
            tool_id: id.into(),
            args_summary: args.to_string(),
            args_digest: "synthetic".into(),
            metadata: None,
        }),
    );
    ingest(
        app,
        seq,
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "call".into(),
        }),
    );
}

#[test]
#[expect(
    clippy::excessive_nesting,
    clippy::cognitive_complexity,
    reason = "capture the complete keyboard and mouse interaction matrix through public input handlers"
)]
fn capture_interaction_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let data: Value = serde_json::from_str(include_str!(
        "../../../scripts/qa/fixtures/tool-interaction-scenarios.json"
    ))?;
    let output = std::env::var_os("HARNESS_TOOL_INTERACTION_ARTIFACT_DIR").map(PathBuf::from);
    if let Some(dir) = &output {
        fs::create_dir_all(dir)?;
    }
    let mut receipts = Vec::new();
    for size in data["sizes"].as_array().ok_or("sizes")? {
        let w = u16::try_from(size[0].as_u64().ok_or("width")?)?;
        let h = u16::try_from(size[1].as_u64().ok_or("height")?)?;
        for case in data["cases"].as_array().ok_or("cases")? {
            let name = case["name"].as_str().ok_or("name")?;
            if case["reference_only"] == true {
                receipts.push(json!({"name":name,"width":w,"height":h,"ids":case["ids"],
                    "disposition":"unsupported","blocker":"No public AppState background-viewer ingestion/installation seam. Do not substitute a foreground tool viewer for the native reference background viewer."}));
                continue;
            }
            let mut app = AppState::new_live(None, false, None);
            app.set_frame_area(Rect::new(0, 0, w, h));
            app.restart_motion_epoch_for_evidence();
            let mut seq = 1;
            ingest(
                &mut app,
                &mut seq,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "turn".into(),
                    text: "Inspect the renderer".into(),
                }),
            );
            let mut keyboard = None;
            match name {
                _ if matches!(
                    name,
                    "question-choice" | "question-freeform" | "question-unfocused"
                ) || name.starts_with("permission-") =>
                {
                    let question = name.starts_with("question");
                    ingest(
                        &mut app,
                        &mut seq,
                        EventV1::PermissionRequested(PermissionRequestedEvent {
                            permission_id: "permission".into(),
                            kind: if question { "question" } else { "bash" }.into(),
                            tool_call_id: Some("call".into()),
                            summary: if question {
                                data["question"].to_string()
                            } else {
                                permission_description(case).join("\n")
                            },
                            request_digest: "synthetic".into(),
                            timeout_ms: 30000,
                            default_decision: PermissionDecision::Deny,
                        }),
                    );
                    assert!(app.active_permission_view().is_some());
                    if question {
                        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
                        if name == "question-unfocused" {
                            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
                        }
                        if name.ends_with("freeform") {
                            key(&mut app, KeyCode::Down, KeyModifiers::NONE);
                            key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
                            for c in data["freeform"].as_str().ok_or("freeform")?.chars() {
                                key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
                            }
                        }
                    } else {
                        for (code, modifiers) in permission_keys(case, &data)? {
                            key(&mut app, code, modifiers);
                        }
                    }
                    assert!(app.active_permission_view().is_some());
                    assert!(!app.permission_submission_pending("permission"));
                }
                _ if name.starts_with("todos-") => {
                    let args = json!({"todos": case.get("todo_items").unwrap_or(&data["todos"])});
                    tool(&mut app, &mut seq, "todo.write", args.clone());
                    ingest(
                        &mut app,
                        &mut seq,
                        EventV1::ToolCallFinished(ToolCallFinishedEvent {
                            tool_call_id: "call".into(),
                            status: ToolCallStatus::Succeeded,
                            output_summary: None,
                            output_digest: None,
                            output_json: Some(args),
                            metadata: None,
                        }),
                    );
                    key(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL);
                    if name.ends_with("filtered") {
                        key(&mut app, KeyCode::Char('h'), KeyModifiers::NONE);
                    }
                    if let Some(steps) = case["todo_keys"].as_array() {
                        for step in steps {
                            let step = step.as_str().ok_or("todo key")?;
                            if let Some(text) = step.strip_prefix("text:") {
                                for c in text.chars() {
                                    key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
                                }
                            } else {
                                key(&mut app, todo_key(step)?, KeyModifiers::NONE);
                            }
                        }
                    }
                    assert!(
                        app.composer.prompt_buffer.is_empty(),
                        "todo input leaked into composer"
                    );
                }
                "question-resolved" | "tool-running" | "tool-finished" => {
                    let question = name.starts_with("question");
                    tool(
                        &mut app,
                        &mut seq,
                        if question { "question" } else { "bash" },
                        if question {
                            data["resolved_question"].clone()
                        } else {
                            data["permission"].clone()
                        },
                    );
                    if question {
                        let prompts = &data["resolved_question"];
                        ingest(
                            &mut app,
                            &mut seq,
                            EventV1::PermissionRequested(PermissionRequestedEvent {
                                permission_id: "resolved-question".into(),
                                kind: "question".into(),
                                tool_call_id: Some("call".into()),
                                summary: prompts.to_string(),
                                request_digest: "synthetic".into(),
                                timeout_ms: 30000,
                                default_decision: PermissionDecision::Deny,
                            }),
                        );
                        ingest(
                            &mut app,
                            &mut seq,
                            EventV1::PermissionResolved(PermissionResolvedEvent {
                                permission_id: "resolved-question".into(),
                                decision: PermissionDecision::Allow,
                                reason: Some(json!([["PostgreSQL"], []]).to_string()),
                            }),
                        );
                    }
                    if name != "tool-running" {
                        let text = if question {
                            data["resolved"].as_str().ok_or("resolved")?
                        } else {
                            "ready"
                        };
                        ingest(
                            &mut app,
                            &mut seq,
                            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                                tool_call_id: "call".into(),
                                status: ToolCallStatus::Succeeded,
                                output_summary: Some(text.into()),
                                output_digest: None,
                                output_json: Some(json!({"output":text})),
                                metadata: None,
                            }),
                        );
                        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(4000));
                    }
                    app.expand_all_tool_outputs_for_test();
                }
                _ if case["focus"].is_string() => {
                    let draft = data["keyboard_draft"]["text"].as_str().ok_or("draft")?;
                    let cursor = usize::try_from(
                        data["keyboard_draft"]["cursor"].as_u64().ok_or("cursor")?,
                    )?;
                    let composer = case["focus"] == "composer";
                    app.composer.prompt_buffer = draft.into();
                    app.composer.prompt_cursor = cursor;
                    app.focus = if composer {
                        Focus::Prompt
                    } else {
                        Focus::Details
                    };
                    assert!(!app.composer_disabled());
                    let mut initial = Terminal::new(TestBackend::new(w, h))?;
                    initial.draw(|frame| render_app(frame, &app))?;
                    let c = case["key"]
                        .as_str()
                        .and_then(|s| s.chars().next())
                        .ok_or("key")?;
                    let event = KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
                    let before = json!({"focus":format!("{:?}",app.focus),
                        "prompt":app.composer.prompt_buffer,"cursor":app.composer.prompt_cursor,
                        "scroll":app.transcript_interaction_snapshot().scroll});
                    app.handle_key(event);
                    // These are editing contracts, not assertions that zero scroll proves a gap.
                    let (expected, expected_cursor) = if composer {
                        match c {
                            'j' => (
                                format!("{}\n{}", &draft[..cursor], &draft[cursor..]),
                                cursor + 1,
                            ),
                            'k' => (draft[..cursor].to_string(), cursor),
                            'u' => (draft[cursor..].to_string(), 0),
                            _ => return Err(format!("unknown composer key {c}").into()),
                        }
                    } else {
                        (draft.to_string(), cursor)
                    };
                    assert_eq!(app.composer.prompt_buffer, expected, "{name}");
                    assert_eq!(app.composer.prompt_cursor, expected_cursor, "{name}");
                    assert_eq!(
                        app.focus,
                        if composer {
                            Focus::Prompt
                        } else {
                            Focus::Details
                        }
                    );
                    keyboard = Some(json!({"requested_focus":case["focus"],"key":case["key"],
                        "default_global_binding":KeyMap::with_defaults().get_action(&event).map(|a| a.as_str()),
                        "before":before,"after":{"focus":format!("{:?}",app.focus),
                        "prompt":app.composer.prompt_buffer,"cursor":app.composer.prompt_cursor,
                        "scroll":app.transcript_interaction_snapshot().scroll}}));
                }
                _ => return Err(format!("unknown case {name}").into()),
            }
            assert_eq!(app.canonical_projection_error(), None);
            if w == 80 {
                // Re-render this very same state wide before the representative 80-column frame.
                let mut wide = Terminal::new(TestBackend::new(120, h))?;
                wide.draw(|frame| render_app(frame, &app))?;
            }
            let mut terminal = Terminal::new(TestBackend::new(w, h))?;
            terminal.draw(|frame| render_app(frame, &app))?;
            let text = terminal
                .backend()
                .buffer()
                .content
                .chunks(usize::from(w))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            if matches!(name, "todos-all" | "todos-filtered") {
                assert!(text.contains("□ Inspect storage interfaces"));
                assert!(text.contains("▶ Implement deterministic"));
                assert!(!text.contains("# Todos"));
                assert_eq!(
                    text.contains("✓ Run regression checks"),
                    name == "todos-all"
                );
                assert_eq!(
                    text.contains("✗ Deploy remote service"),
                    name == "todos-all"
                );
                assert!(text.contains(if name == "todos-all" {
                    "h:hide done"
                } else {
                    "h:show done"
                }));
            }
            if name == "permission-feedback-moved" {
                assert!(
                    text.contains("Use an in-memory store"),
                    "lost rejection draft: {text}"
                );
            }
            if name == "permission-freeform" && w == 40 {
                assert!(
                    text.lines()
                        .any(|line| line.trim_end().ends_with("Use an in-memory store")),
                    "wrapped feedback retained placeholder glyphs: {text}"
                );
            }
            if name == "permission-feedback-expanded" {
                assert!(
                    text.contains("Record the decision"),
                    "Ctrl-F did not expand while editing: {text}"
                );
            }
            assert!(terminal
                .backend()
                .buffer()
                .content
                .iter()
                .any(|cell| cell.symbol() != " "));
            let snapshot = app.transcript_interaction_snapshot();
            receipts.push(json!({"name":name,"width":w,"height":h,"ids":case["ids"],
                "scroll_after":snapshot.scroll,"following":snapshot.follow_mode,
                "expanded":snapshot.expanded_tool_call_ids,"viewer":format!("{:?}",app.transcript_viewer_mode()),
                "permission_active":app.active_permission_view().is_some(),"keyboard":keyboard}));
            if let Some(dir) = &output {
                let stem = format!("interaction-{name}-{w}x{h}-motion-0ms");
                let mut bytes = Vec::new();
                {
                    let mut terminal = Terminal::with_options(
                        CrosstermBackend::new(&mut bytes),
                        TerminalOptions {
                            viewport: Viewport::Fixed(Rect::new(0, 0, w, h)),
                        },
                    )?;
                    terminal.draw(|frame| render_app(frame, &app))?;
                }
                fs::write(dir.join(format!("{stem}.ansi")), bytes)?;
                fs::write(dir.join(format!("{stem}.txt")), text)?;
            }
        }
    }
    if let Some(dir) = &output {
        fs::write(
            dir.join("states.json"),
            serde_json::to_vec_pretty(&receipts)?,
        )?;
        fs::write(
            dir.join("producer.json"),
            serde_json::to_vec_pretty(&json!({
            "entrypoints":["AppState::ingest_event", "AppState::handle_key", "render_app"],
            "timing":{"animation_ms":0,"terminal_age_ms":4000,"mode":"injected motion clock"},
            "setup":"Synthetic events only; no tool executors, provider, network, or durable replay work"}))?,
        )?;
    }
    Ok(())
}

fn todo_key(key: &str) -> Result<KeyCode, Box<dyn std::error::Error>> {
    Ok(match key {
        "Down" => KeyCode::Down,
        "Enter" => KeyCode::Enter,
        "Esc" => KeyCode::Esc,
        "Tab" => KeyCode::Tab,
        text if text.chars().count() == 1 => KeyCode::Char(text.chars().next().ok_or("key")?),
        _ => return Err(format!("unknown todo key {key}").into()),
    })
}

fn permission_description(case: &Value) -> Vec<&str> {
    case["permission_description"].as_array().map_or_else(
        || vec!["Check terminal output", "printf ready"],
        |lines| lines.iter().filter_map(Value::as_str).collect(),
    )
}

fn permission_keys(
    case: &Value,
    data: &Value,
) -> Result<Vec<(KeyCode, KeyModifiers)>, Box<dyn std::error::Error>> {
    let mut keys = Vec::new();
    for step in case["permission_keys"]
        .as_array()
        .ok_or("permission keys")?
    {
        let step = step.as_str().ok_or("permission key")?;
        let text = if step == "feedback" {
            Some(data["freeform"].as_str().ok_or("freeform")?)
        } else {
            step.strip_prefix("text:")
        };
        if let Some(text) = text {
            keys.extend(text.chars().map(|c| (KeyCode::Char(c), KeyModifiers::NONE)));
        } else {
            keys.push(match step {
                "Up" => (KeyCode::Up, KeyModifiers::NONE),
                "Backspace" => (KeyCode::Backspace, KeyModifiers::NONE),
                "Ctrl-f" => (KeyCode::Char('f'), KeyModifiers::CONTROL),
                _ => (todo_key(step)?, KeyModifiers::NONE),
            });
        }
    }
    Ok(keys)
}
