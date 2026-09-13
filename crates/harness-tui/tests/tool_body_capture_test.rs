//! Production event/input ingestion and render_app captures, owned by the tool-body lane.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::{
    app::{AppState, Focus},
    theme::{ColorLevel, GlyphMode},
    ui::render_app,
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};
use serde_json::{json, Value};
use std::{fs, path::Path, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn patch_lines(text: &str, prefix: char) -> String {
    text.lines().fold(String::new(), |mut output, line| {
        output.push(prefix);
        output.push_str(line);
        output.push('\n');
        output
    })
}
fn ingest(app: &mut AppState, seq: &mut u64, payload: EventV1) {
    app.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("body-{seq}"),
        seq: *seq,
        run_id: "tool-bodies".into(),
        mono_ms: *seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("capture".into())),
        correlation_id: Some("turn".into()),
        causation_id: None,
        stream_key: None,
        payload,
    });
    *seq += 1;
    assert_eq!(app.canonical_projection_error(), None);
}
fn output_text(config: &Value, case: &Value) -> String {
    match string(case,"family") {
        "list" => string(case, "text").into(),
        "memory" if case["empty"] == true => "No memory results found.".into(),
        "memory" => (1..=3).map(|i| format!(
            "### Result {i} (score: 0.{}, source: workspace)\n**File:** memory/note-{i}.md (lines 10-14)\n```\nKeep the tool identity stable.\nA long memory snippet is bounded without losing the source range or score.\nThird snippet line.\nFourth snippet line.\n```",90-i
        )).collect::<Vec<_>>().join("\n"),
        "mcp" => format!("**Issue ready**\nResource note: preserve the full tool response.\n{}",string(config,"long_text")),
        "media" if string(case,"mime")=="application/pdf" => "PDF read successfully".into(),
        "media" => "Image read successfully".into(),
        "sent" if string(case,"outcome")=="admission_uncertain" => "Message admission could not be confirmed; the message may or may not have been accepted.".into(),
        "sent" => "Message accepted (message_id: message-1).".into(),
        "integration" if case["empty"]==true => json!({"results":[]}).to_string(),
        "integration" => json!({"results":[{"server":"linear","tools":[{"tool_name":"linear__save_issue","description":"Save an issue", "score":0.95}]},{"server":"calendar","tools":[{"tool_name":"calendar__find_event","description":"Find an event", "score":0.8}]}]}).to_string(),
        _ => case["text"].as_str().unwrap_or(string(config,"long_text")).into(),
    }
}
fn payload(config: &Value, case: &Value, alias: &str) -> Result<(Value, String, Value)> {
    let family = string(case, "family");
    let path = string(case, "path");
    let text = output_text(config, case);
    let (args, result) = match family {
        "execute" => (
            json!({"command":"printf 'first\\nsecond\\n'\nprintf 'a deliberately long command argument for terminal reflow'", "description":"Inspect terminal output"}),
            json!({"stdout":text,"stderr":"","exit_code":0}),
        ),
        "read" => (
            json!({"path":path,"offset":case["offset"],"limit":case["limit"]}),
            json!({"path":path,"total_lines":case["total"],"metadata":{"display":{"text":text,"lineStart":case["offset"],"lineEnd":52,"totalLines":case["total"]}}}),
        ),
        "media" => (
            json!({"path":path}),
            json!({"title":path,"path":path,"resolved_path":path,"metadata":{"preview":text,"truncated":false,"loaded":[]},"attachments":[{"type":"file","mime":case["mime"],"url":config["assets"][path]["data_url"]}]}),
        ),
        "edit" => {
            let args = match string(case, "operation") {
                "create" => json!({"path":path,"content":config["after"]}),
                "patch" => {
                    let removed = patch_lines(string(config, "before"), '-');
                    let added = patch_lines(string(config, "after"), '+');
                    json!({"patchText":format!("*** Begin Patch\n*** Update File: src/renderer.rs\n@@\n{removed}{added}*** Add File: src/second.rs\n+pub const READY: bool = true;\n*** End Patch")})
                }
                _ => {
                    json!({"filePath":path,"oldString":config["before"],"newString":config["after"]})
                }
            };
            (
                args,
                if alias == "apply_patch" {
                    json!({"files":[path,"src/second.rs"],"edits":[{"path":path,"diff_rel_path":"artifacts/body.diff"},{"path":"src/second.rs","diff_rel_path":"artifacts/second.diff"}]})
                } else {
                    json!({"path":path})
                },
            )
        }
        "list" => (
            json!({"path":path}),
            json!({"path":path,"count":case["file_count"],"truncated":false}),
        ),
        "search" => {
            let mode = string(case, "mode");
            let result = if string(case, "id") == "glob" {
                json!({"paths":["src/renderer.rs","src/layout.rs"],"total_count":2})
            } else {
                match mode {
                    "files_with_matches" => {
                        json!({"output_mode":mode,"files":["src/renderer.rs","src/layout.rs"],"total_count":2})
                    }
                    "count" => {
                        json!({"output_mode":mode,"counts":[{"file":"src/renderer.rs","count":2},{"file":"src/layout.rs","count":1}],"total_count":3})
                    }
                    _ => {
                        json!({"output_mode":mode,"matches":[{"path":"src/renderer.rs","line_number":41,"text":"ready: first match with enough explanatory text to exercise narrow wrapped output"},{"path":"src/renderer.rs","line_number":58,"text":"ready: second match"},{"path":"src/layout.rs","line_number":12,"text":"ready: layout match"}],"total_count":3})
                    }
                }
            };
            (
                if string(case, "id") == "glob" {
                    json!({"pattern":"**/*.rs","path":"src"})
                } else {
                    json!({"pattern":"ready","path":"src","output_mode":mode})
                },
                result,
            )
        }
        "fetch" => (
            json!({"url":"https://docs.example.org/terminal","format":"text"}),
            json!({"url":"https://docs.example.org/terminal","status":200,"content_type":"text/plain","media_type":"text/plain","response_kind":"text","byte_len":text.len()}),
        ),
        "web" => {
            let mut result = json!({"query":"terminal reflow","results":5,"empty":false});
            result[string(case, "source_field")] = config["citations"].clone();
            (json!({"query":"terminal reflow"}), result)
        }
        "mcp" => {
            let input = json!({"title":"Preserve output","labels":["terminal","capture"]});
            let args = if alias.ends_with("tool.call") {
                json!({"tool":"save_issue","arguments":input})
            } else {
                input.clone()
            };
            (
                args,
                json!({"server":{"id":"linear","transport":"stdio"},"protocolVersion":"2024-11-05","serverInfo":{"name":"capture"},"payload":{"tool":"save_issue","arguments":input,"result":{"content":[{"type":"text","text":"**Issue ready**"},{"type":"resource","resource":{"uri":"memory://capture/note","mimeType":"text/plain","text":format!("Resource note: preserve the full tool response.\n{}",string(config,"long_text"))}}]}}}),
            )
        }
        "integration" => (
            json!({"query":"issue calendar","limit":2}),
            serde_json::from_str(&text)?,
        ),
        "memory" => (
            json!({"query":"stable tool identity"}),
            json!({"text":text}),
        ),
        "sent" => (
            json!({"subagent_id":"child-capture","text":"Inspect the tool body.\nKeep the original request and report the result.","queue":false}),
            json!({"outcome":case["outcome"],"message_id":"message-1"}),
        ),
        "unknown" => (json!({"query":"external result"}), json!({"text":text})),
        _ => return Err(format!("unhandled family: {family}").into()),
    };
    Ok((args, text, result))
}
fn render(app: &AppState, width: u16, height: u16) -> Result<Buffer> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| render_app(frame, app))?;
    Ok(terminal.backend().buffer().clone())
}
fn write_diffs(config: &Value, root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("artifacts"))?;
    let before = string(config, "before");
    let after = string(config, "after");
    let mut diff = format!(
        "--- a/src/renderer.rs\n+++ b/src/renderer.rs\n@@ -1,{} +1,{} @@\n",
        before.lines().count(),
        after.lines().count()
    );
    for line in before.lines() {
        diff.push_str(&format!("-{line}\n"));
    }
    for line in after.lines() {
        diff.push_str(&format!("+{line}\n"));
    }
    fs::write(root.join("artifacts/body.diff"), diff)?;
    fs::write(
        root.join("artifacts/second.diff"),
        "--- /dev/null\n+++ b/src/second.rs\n@@ -0,0 +1 @@\n+pub const READY: bool = true;\n",
    )?;
    Ok(())
}
#[expect(
    clippy::cognitive_complexity,
    reason = "map the existing tool families and lifecycle states into one production capture"
)]
fn capture(
    config: &Value,
    case: &Value,
    alias: &str,
    state: &str,
    width: u16,
    out: Option<&Path>,
) -> Result<()> {
    let height = u16::try_from(config["height"].as_u64().ok_or("height must be a u64")?)?;
    let session = tempfile::tempdir()?;
    write_diffs(config, session.path())?;
    let mut app = AppState::new_live(Some(session.path().into()), false, None);
    app.set_startup_logo_capabilities_for_evidence(ColorLevel::TrueColor, GlyphMode::Preferred);
    app.restart_motion_epoch_for_evidence();
    app.advance_wall_clock_for_motion_evidence(Duration::ZERO);
    let mut seq = 1;
    ingest(
        &mut app,
        &mut seq,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "turn".into(),
            text: "Inspect the tool body".into(),
        }),
    );
    ingest(
        &mut app,
        &mut seq,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider".into(),
            provider_id: "fixture".into(),
            model_id: "capture".into(),
            prompt_summary: "Inspect the tool body".into(),
            request_digest: "capture".into(),
            metadata: None,
        }),
    );
    let (args, text, result) = payload(config, case, alias)?;
    // Both producers receive the same serialized input order; serde_json's
    // default map would otherwise sort this before the TUI ever receives it.
    let args_summary = if string(case, "family") == "mcp" {
        let input = string(config, "mcp_arguments");
        if alias.ends_with("tool.call") {
            format!(r#"{{"tool":"save_issue","arguments":{input}}}"#)
        } else {
            input.into()
        }
    } else {
        args.to_string()
    };
    ingest(
        &mut app,
        &mut seq,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "body-call".into(),
            tool_id: alias.into(),
            args_summary,
            args_digest: "capture".into(),
            metadata: None,
        }),
    );
    if state != "pending" {
        ingest(
            &mut app,
            &mut seq,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "body-call".into(),
            }),
        );
    }
    let terminal = state.starts_with("success") || state.starts_with("failure");
    if terminal {
        // Establish the running frame, then settle at an exact age without sleeping.
        render(&app, width, height)?;
        let failed = state.starts_with("failure");
        ingest(
            &mut app,
            &mut seq,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "body-call".into(),
                status: if failed {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Succeeded
                },
                output_summary: Some(if failed {
                    if string(case, "family") == "sent" {
                        "Subagent is not active or is finalizing."
                    } else {
                        "Tool failed: deterministic capture error"
                    }
                    .into()
                } else {
                    text.clone()
                }),
                output_digest: None,
                output_json: (!failed).then_some(result.clone()),
                metadata: None,
            }),
        );
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(
            config["clock"]["terminal_age_ms"]
                .as_u64()
                .ok_or("terminal_age_ms must be a u64")?,
        ));
    }
    if matches!(alias, "edit" | "write" | "fs.write")
        && state != "success-default"
        && app.is_tool_output_expanded_for_test("body-call")
    {
        app.focus = Focus::Details;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert!(!app.is_tool_output_expanded_for_test("body-call"));
        app.focus = Focus::Prompt;
    }
    if state == "success-member-closed" {
        // Open the native singleton group without opening its member body.
        app.focus = Focus::Details;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert!(!app.is_tool_output_expanded_for_test("body-call"));
        app.focus = Focus::Prompt;
    }
    if state.ends_with("open") {
        // The actual transcript keyboard route, not a forged disclosure field.
        app.focus = Focus::Details;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        if !app.is_tool_output_expanded_for_test("body-call") {
            // A native singleton group owns the first navigation stop.
            // Descend to its member only when the exact tool state stayed closed.
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        }
        // apply_patch deliberately has no tool-level disclosure (ui_tool_visibility.rs).
        // A failed patch has no file preview either, so activation is a real no-op.
        assert_eq!(
            app.is_tool_output_expanded_for_test("body-call"),
            !(string(case, "family") == "media"
                || (state.starts_with("failure")
                    && matches!(
                        alias,
                        "apply_patch"
                            | "fs.read"
                            | "read"
                            | "fs.glob"
                            | "glob"
                            | "fs.grep"
                            | "grep"
                            | "fs.ls"
                            | "list"
                            | "search.web"
                            | "websearch"
                            | "web.fetch"
                            | "webfetch"
                            | "search_tool"
                            | "memory.search"
                            | "agent.message"
                            | "agent.send_message"
                            | "fixture.inspect"
                    ))),
            "unexpected native disclosure state: {alias} ({state})"
        );
        app.focus = Focus::Prompt;
    }
    if width == 80 {
        render(&app, 120, height)?;
    }
    let buffer = render(&app, width, height)?;
    assert_eq!(
        buffer,
        render(&app, width, height)?,
        "same exact clock must produce identical cells"
    );
    assert_eq!(app.canonical_projection_error(), None);
    assert!(buffer.content.iter().any(|cell| cell.symbol() != " "));
    if string(case, "family") == "execute" && state == "failure-open" {
        let text = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(text.matches("deterministic").count(), 1, "{text}");
    }
    if matches!(string(case, "id"), "create" | "edit") {
        let rendered = buffer
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            rendered.contains("fn ready()"),
            state.starts_with("success") && (state.ends_with("open") || state == "success-default"),
            "proposed diff escaped lifecycle/disclosure policy: {alias} {state}\n{rendered}"
        );
        if matches!(state, "pending" | "running") {
            assert!(
                rendered.contains(
                    string(case, "path")
                        .rsplit('/')
                        .next()
                        .ok_or("target filename")?
                ),
                "unfinished edits must retain the target path: {rendered}"
            );
        }
        if state == "failure-open" {
            assert!(
                rendered.contains("deterministic capture error"),
                "opening a failed edit must preserve its error: {rendered}"
            );
        }
    }
    if let Some(out) = out {
        let name = format!(
            "body-{}-{}-{state}-{width}x{height}-motion-0ms",
            string(case, "id"),
            alias.replace(['.', '_'], "-")
        );
        let mut bytes = Vec::new();
        {
            let mut terminal = Terminal::with_options(
                CrosstermBackend::new(&mut bytes),
                TerminalOptions {
                    viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
                },
            )?;
            terminal.draw(|frame| render_app(frame, &app))?;
        }
        assert!(!bytes.is_empty());
        fs::write(out.join(format!("{name}.ansi")), bytes)?;
        fs::write(
            out.join(format!("{name}.producer.json")),
            serde_json::to_vec_pretty(&json!({
                "case":case["id"],"alias":alias,"state":state,"audit":config["producers"][string(case,"family")]["audit"],
                "input":args,"success_output_summary":text,"success_output_json":result,"clock":config["clock"],"support":case["support"],
                "input_route":if state.ends_with("open") {"Focus::Details, Down, Ctrl+E; if tool still closed, Down, Ctrl+E; Focus::Prompt"} else if state == "success-member-closed" {"Focus::Details, Down, Ctrl+E; Focus::Prompt (group only)"} else {"default disclosure"},
                "expanded_tool_ids":app.transcript_interaction_snapshot().expanded_tool_call_ids
            }))?,
        )?;
    }
    Ok(())
}
#[test]
#[expect(
    clippy::excessive_nesting,
    reason = "the capture matrix enumerates tool cases, aliases, widths, and lifecycle states"
)]
fn production_tool_body_captures() -> Result<()> {
    let config: Value = if let Some(path) = std::env::var_os("TOOL_BODY_SCENARIOS") {
        serde_json::from_slice(&fs::read(path)?)?
    } else {
        serde_json::from_str(include_str!(
            "../../../scripts/qa/fixtures/tool-body-scenarios.json"
        ))?
    };
    assert_eq!(config["schema"], "tool-body-production-v1");
    let output = std::env::var_os("TOOL_BODY_HARNESS_OUT").map(std::path::PathBuf::from);
    if let Some(out) = &output {
        fs::create_dir_all(out)?;
    }
    let mut count = 0;
    for case in config["cases"].as_array().ok_or("cases must be an array")? {
        assert!(!config["producers"][string(case, "family")]["audit"]
            .as_array()
            .ok_or("producer audit must be an array")?
            .is_empty());
        for alias in case["aliases"]
            .as_array()
            .ok_or("aliases must be an array")?
        {
            for width in config["widths"]
                .as_array()
                .ok_or("widths must be an array")?
            {
                for state in case
                    .get("states")
                    .unwrap_or(&config["states"])
                    .as_array()
                    .ok_or("states must be an array")?
                {
                    if width == 80 && state != "success-open" {
                        continue;
                    }
                    capture(
                        &config,
                        case,
                        alias.as_str().ok_or("alias must be a string")?,
                        state.as_str().ok_or("state must be a string")?,
                        u16::try_from(width.as_u64().ok_or("width must be a u64")?)?,
                        output.as_deref(),
                    )?;
                    count += 1;
                }
            }
        }
    }
    if let Some(out) = output {
        fs::write(
            out.join("producer.json"),
            serde_json::to_vec_pretty(&json!({
                "entrypoints":["AppState::ingest_event","AppState::handle_key","harness_tui::ui::render_app"],
                "frames":count,"timing":config["clock"],"scope":"Production event shapes; unsupported executors use explicit render-only external events, not invented execution."
            }))?,
        )?;
    }
    println!("Harness production tool bodies: {count} frames");
    Ok(())
}
