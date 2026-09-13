use super::*;
use ratatui::{backend::CrosstermBackend, TerminalOptions, Viewport};
use serde_json::{json, Value};
use std::time::Duration;

type CaptureResult<T> = Result<T, Box<dyn std::error::Error>>;

fn draw(app: &mut AppState, area: Rect) -> CaptureResult<(Vec<u8>, Vec<String>)> {
    app.set_frame_area(area);
    let mut bytes = Vec::new();
    let painted = {
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(&mut bytes),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )?;
        terminal
            .draw(|frame| render_app(frame, app))?
            .buffer
            .content
            .chunks(usize::from(area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    };
    Ok((bytes, painted))
}

fn scroll_app(config: &Value) -> CaptureResult<AppState> {
    let read = config["family"].as_str() == Some("read");
    let mut app = AppState::new_live(None, false, None);
    app.advance_wall_clock_for_motion_evidence(Duration::ZERO);
    let prompt = config["prompt"].as_str().ok_or("prompt")?;
    let output = (1..=config["line_count"].as_u64().ok_or("line count")?)
        .map(|i| {
            format!(
                "line_{i:03} {}",
                config["line_suffix"].as_str().unwrap_or("alpha beta gamma")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let events = [
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "runtime-turn".into(),
            text: prompt.into(),
        }),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "runtime-provider".into(),
            provider_id: "fixture".into(),
            model_id: "fixture".into(),
            prompt_summary: prompt.into(),
            request_digest: "fixture".into(),
            metadata: None,
        }),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "runtime-scroll".into(),
            tool_id: if read { "read" } else { "bash" }.into(),
            args_summary: if read {
                json!({"path":"/tmp/viewer.txt", "offset":7, "limit":160})
            } else {
                json!({
                    "command": config["command"],
                    "description": config["description"]
                })
            }
            .to_string(),
            args_digest: "fixture".into(),
            metadata: None,
        }),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "runtime-provider".into(),
            finish_reason: "tool_calls".into(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "runtime-scroll".into(),
        }),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "runtime-scroll".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some(output.clone()),
            output_digest: None,
            output_json: Some(if read {
                json!({"metadata":{"display":{"text":output, "lineStart":7, "lineEnd":166, "totalLines":200}}})
            } else {
                json!({"stdout":output, "stderr":"", "exit_code":0})
            }),
            metadata: None,
        }),
    ];
    for (index, event) in events.into_iter().enumerate() {
        let mut event = envelope(u64::try_from(index)? + 1, "runtime-turn", event);
        event.ts = None;
        app.ingest_event(event);
    }
    app.advance_wall_clock_for_motion_evidence(Duration::from_secs(60));
    app.toggle_tool_output_for_test("runtime-scroll");
    app.focus = Focus::Details;
    assert_eq!(app.canonical_projection_error(), None);
    Ok(app)
}

#[test]
fn native_tool_scroll_inputs_follow_reference_line_and_half_page_steps() -> CaptureResult<()> {
    let config: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/qa/fixtures/tool-runtime-scenarios.json"
    )))?;
    let out = std::env::var_os("HARNESS_TOOL_RUNTIME_HARNESS_DIR").map(std::path::PathBuf::from);
    if let Some(out) = &out {
        fs::create_dir_all(out)?;
    }
    let mut failures = Vec::new();
    let mut receipts = Vec::new();
    for size in config["sizes"].as_array().ok_or("sizes")? {
        let width = u16::try_from(size[0].as_u64().ok_or("width")?)?;
        let height = u16::try_from(size[1].as_u64().ok_or("height")?)?;
        let area = Rect::new(0, 0, width, height);
        for key in config["keys"].as_array().ok_or("keys")? {
            let key = key
                .as_str()
                .and_then(|key| key.chars().next())
                .ok_or("key")?;
            let mut app = scroll_app(&config)?;
            draw(&mut app, area)?;
            assert!(
                !app.live_turn_status_visible(),
                "native idle fixture must not render unfinished foreground work: {:?}",
                app.runtime_state().kind
            );
            let max = app.transcript_view.last_transcript_max_scroll.get();
            let start = usize::try_from(config["start_offset"].as_u64().ok_or("offset")?)?;
            assert!(
                max > start * 2,
                "the actual tool output must overflow: {width}x{height}, max={max}"
            );
            app.set_transcript_scroll_for_test(max - start);
            let (before_bytes, _) = draw(&mut app, area)?;
            let before = app.transcript_view.transcript_scroll;
            let viewport = app.transcript_view.last_transcript_viewport_height.get();
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::CONTROL));
            let after = app.transcript_view.transcript_scroll;
            let step = if matches!(key, 'k' | 'j') {
                1
            } else {
                // Independently captured native AgentView paging, not the
                // Harness measurement whose correctness this test protects.
                match (width, height) {
                    (40 | 80, 32) => 11,
                    (120, 40) => 15,
                    _ => return Err("missing native paging expectation for fixture size".into()),
                }
            };
            let expected = if matches!(key, 'k' | 'u') {
                before + step
            } else {
                before - step
            };
            if after != expected {
                failures.push(format!(
                    "{width}x{height} Ctrl-{key}: expected {expected}, got {after}"
                ));
            }
            let (bytes, _) = draw(&mut app, area)?;
            let name = format!("runtime-scroll-{key}-{width}x{height}-motion-0ms");
            if let Some(out) = &out {
                if key == 'k' {
                    fs::write(
                        out.join(format!(
                            "runtime-scroll-before-{width}x{height}-motion-0ms.ansi"
                        )),
                        &before_bytes,
                    )?;
                }
                fs::write(out.join(format!("{name}.ansi")), bytes)?;
            }
            receipts.push(json!({"name":name, "before_from_bottom":before, "after_from_bottom":after,
                "viewport":viewport, "max_scroll":max, "route":"AppState::handle_key with measured Details viewport"}));
            if key == 'j' {
                app.focus = Focus::Prompt;
                let before = app.composer_render_text();
                app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
                assert_eq!(app.composer_render_text(), format!("{before}\n"));
            }
        }
    }
    if let Some(out) = &out {
        fs::write(
            out.join("producer.json"),
            serde_json::to_vec_pretty(&receipts)?,
        )?;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "capture the complete native mouse lifecycle across reference widths"
)]
fn native_tool_header_mouse_capture_uses_rendered_hit_targets() -> CaptureResult<()> {
    let config: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/qa/fixtures/tool-runtime-scenarios.json"
    )))?;
    let out = std::env::var_os("HARNESS_TOOL_RUNTIME_HARNESS_DIR").map(std::path::PathBuf::from);
    if let Some(out) = &out {
        fs::create_dir_all(out)?;
    }
    let _clipboard = ClipboardModeGuard::disabled_copy_on_select();
    let header_word = config["description"]
        .as_str()
        .and_then(|description| description.split_whitespace().next())
        .ok_or("tool description")?;
    let mut receipts = Vec::new();
    for size in config["sizes"].as_array().ok_or("sizes")? {
        let width = u16::try_from(size[0].as_u64().ok_or("width")?)?;
        let height = u16::try_from(size[1].as_u64().ok_or("height")?)?;
        let area = Rect::new(0, 0, width, height);
        let mut app = scroll_app(&config)?;
        draw(&mut app, area)?;
        app.set_transcript_scroll_for_test(app.transcript_view.last_transcript_max_scroll.get());
        let (_, mut painted) = draw(&mut app, area)?;
        let mut first_header_row = None;
        for (count, expanded) in [(1, true), (2, false), (3, true), (4, true)] {
            let row = (0..height)
                .find(|&row| {
                    painted[usize::from(row)].contains(header_word)
                        && matches!(
                            transcript_mouse_target(&app, area, 5, row),
                            Some(TranscriptMouseTarget::Tool { ref tool_call_id })
                                if tool_call_id == "runtime-scroll"
                        )
                })
                .ok_or("rendered tool header hit target")?;
            let original_header_row = *first_header_row.get_or_insert(row);
            app.advance_wall_clock_for_motion_evidence(Duration::from_millis(10));
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                assert!(app.handle_mouse(
                    MouseEvent {
                        kind,
                        column: 5,
                        row,
                        modifiers: KeyModifiers::NONE
                    },
                    area,
                    None,
                    None,
                    None,
                ));
            }
            assert!(
                app.transcript_view.hovered_transcript_target.is_none(),
                "native click {count} synthesized pointer hover at {width} columns"
            );
            let (bytes, next_painted) = draw(&mut app, area)?;
            painted = next_painted;
            assert!(
                painted.iter().any(|line| line.contains(header_word)),
                "painted header disappeared after native click {count} at {width} columns:\n{}",
                painted.join("\n")
            );
            if count >= 3 {
                assert_eq!(
                    painted.iter().position(|line| line.contains(header_word)),
                    Some(usize::from(original_header_row)),
                    "native click {count} changed header alignment at {width} columns"
                );
            }
            assert_eq!(
                app.transcript_view
                    .expanded_tool_outputs
                    .contains("runtime-scroll"),
                expanded,
                "native header click {count} at {width} columns"
            );
            if expanded {
                assert!(
                    painted[usize::from(original_header_row)].contains('◆'),
                    "selecting an expanded tool must preserve its completion marker"
                );
            }
            if expanded && width == 40 {
                let continuation = config["description"]
                    .as_str()
                    .and_then(|description| description.split_whitespace().last())
                    .ok_or("description continuation")?;
                let continuation_row = original_header_row + 1;
                assert!(
                    painted[usize::from(continuation_row)].contains(continuation),
                    "expanded description must wrap instead of truncating:\n{}",
                    painted.join("\n")
                );
                let first_line = &painted[usize::from(original_header_row)];
                let next_line = &painted[usize::from(continuation_row)];
                let first_column = unicode_width::UnicodeWidthStr::width(
                    &first_line[..first_line.find(header_word).ok_or("header word")?],
                );
                let next_column = unicode_width::UnicodeWidthStr::width(
                    &next_line[..next_line.find(continuation).ok_or("continuation word")?],
                );
                assert_eq!(
                    next_column,
                    first_column.saturating_sub(2),
                    "description continuation must use the native Run hanging indent"
                );
                assert!(matches!(
                    transcript_mouse_target(&app, area, 5, continuation_row),
                    Some(TranscriptMouseTarget::Tool { ref tool_call_id })
                        if tool_call_id == "runtime-scroll"
                ));
            }
            assert!(matches!(
                app.selected_transcript_entry().and_then(|entry| entry.target),
                Some(TranscriptMouseTarget::Tool { ref tool_call_id })
                    if tool_call_id == "runtime-scroll"
            ));
            let name = format!("runtime-mouse-header-{count}-{width}x{height}-motion-0ms");
            if let Some(out) = &out {
                fs::write(out.join(format!("{name}.ansi")), bytes)?;
            }
            receipts.push(json!({
                "name":name, "column":5, "row":row, "click_count":count,
                "expanded":app.transcript_view.expanded_tool_outputs.contains("runtime-scroll"),
                "focus":format!("{:?}", app.focus),
                "route":"AppState::handle_mouse Down/Up using real rendered hit targets and deterministic AppState::now",
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
        "/../../scripts/qa/fixtures/tool-runtime-scenarios.json"
    )))?;
    let out = std::env::var_os("HARNESS_TOOL_RUNTIME_HARNESS_DIR").map(std::path::PathBuf::from);
    if let Some(out) = &out {
        fs::create_dir_all(out)?;
    }

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
            app.focus = Focus::Details;
            assert!(app.select_transcript_tool("runtime-scroll"));
            let code = KeyCode::Enter;
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
            assert!(app.transcript_viewer().is_some());
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
                app.handle_key(KeyEvent::new(code, modifiers));
                draw(&mut app, area)?;
            }
            let name = format!(
                "runtime-viewer-{}-{width}x{height}-motion-0ms",
                case["name"].as_str().ok_or("case")?
            );
            let (bytes, painted) = draw(&mut app, area)?;
            assert!(painted.iter().any(|line| line.contains("[x]")));
            if case["name"]
                .as_str()
                .is_some_and(|name| name.ends_with("select"))
            {
                let copied = app
                    .transcript_viewer()
                    .ok_or("viewer")?
                    .copy_selection_text()?;
                assert_eq!(
                    copied.lines().count(),
                    2,
                    "wrapped selection keeps source lines"
                );
                assert!(copied.contains("line_001") && copied.contains("line_002"));
            }
            if let Some(out) = &out {
                fs::write(out.join(format!("{name}.ansi")), bytes)?;
            }
            for _ in 0..3 {
                if app.transcript_viewer().is_none() {
                    break;
                }
                let code = KeyCode::Esc;
                app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
            }
            assert!(app.transcript_viewer().is_none());
        }
    }
    Ok(())
}
