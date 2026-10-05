use super::*;

#[test]
fn tool_input_streams_before_execution_and_follows_the_wrapped_tail() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    for (name, field) in [
        ("eval", "code"),
        ("bash", "command"),
        ("write", "content"),
        ("edit", "newString"),
        ("read", "path"),
        ("grep", "pattern"),
        ("spawn_subagent", "prompt"),
        ("linear__save_issue", "note"),
    ] {
        let mut state = Capture::new(&fixture)?;
        for fragment in [
            format!(r#"{{"language":"js","{field}":"FIRST_LINE\nconst value = \"hello\";\n"#),
            "earlier line\\n".repeat(8),
            format!(r#"OLD_START {}LATEST \u754c \ud83d"#, "word ".repeat(50)),
            r#"\ude80"#.into(),
        ] {
            state.live("provider_tool_input_delta", json!({
                "request_id":"provider", "tool_call_id":"draft", "tool_name":name, "delta":fragment
            }))?;
            let screen = render(&mut state.app, 40, 30)?;
            let text = screen
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("writing"), "{name}: {text}");
        }
        let screen = render(&mut state.app, 40, 30)?;
        let text = screen
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(
            text.contains("LATEST 界") && text.contains('🚀'),
            "{name}: {text}"
        );
        assert!(!text.contains("FIRST_LINE"), "{name}: {text}");
        assert!(
            !text.contains("OLD_START"),
            "preview must clip wrapped rows: {text}"
        );
        assert!(!text.contains("\\u"), "escaped JSON leaked: {text}");
        assert!(
            !text.contains("queued"),
            "writing is not queued execution: {text}"
        );
        assert!(text.contains('…'), "{name}: {text}");
        state.app.focus = Focus::Details;
        assert!(state.app.select_transcript_tool("draft"));
        state
            .app
            .handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        let expanded = render(&mut state.app, 120, 40)?;
        let expanded = expanded
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(expanded.contains("FIRST_LINE"), "{name}: {expanded}");
        // Live input must never become replay history.
        state.app.replace_events(state.events.clone());
        let replay = render(&mut state.app, 40, 30)?;
        assert!(!replay.content.iter().any(|cell| cell.symbol() == "🚀"));
    }
    Ok(())
}

#[test]
fn concurrent_input_previews_keep_arrival_order_and_the_active_field_visible() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    let mut state = Capture::new(&fixture)?;
    for op in ["request", "start", "finish"] {
        state.action(&json!({"op":op, "id":"a"}), &fixture)?;
    }
    for (id, label) in [("z-first", "FIRST_CALL"), ("a-second", "SECOND_CALL")] {
        state.live(
            "provider_tool_input_delta",
            json!({
                "request_id":"provider", "tool_call_id":id, "tool_name":"custom.inspect",
                "delta":format!(r#"{{"z_older":"{}","a_current":"{label}"#, "older\\n".repeat(10))
            }),
        )?;
    }
    let screen = render(&mut state.app, 120, 40)?;
    let text = screen
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    let first = text
        .find("FIRST_CALL")
        .ok_or_else(|| format!("first input is hidden: {text}"))?;
    let second = text
        .find("SECOND_CALL")
        .ok_or_else(|| format!("second input is hidden: {text}"))?;
    assert!(first < second, "live calls were sorted by ID: {text}");
    Ok(())
}

#[test]
fn streaming_input_redacts_nested_credentials_and_terminal_controls() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    let mut state = Capture::new(&fixture)?;
    for fragment in [
        r#"{"arguments":{"password":"NEVER_SHOW"#,
        r#"_PASSWORD","path":"/private/fixture/note.txt","items":[{"note":"visible\u00"#,
        r#"1b[31m text","token":"NEVER_SHOW_TOKEN"}],"enabled":tru"#,
        "e}}",
    ] {
        state.live("provider_tool_input_delta", json!({
            "request_id":"provider", "tool_call_id":"draft", "tool_name":"custom.inspect", "delta":fragment
        }))?;
        let screen = render(&mut state.app, 120, 40)?;
        let text = screen
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        for hidden in ["NEVER_SHOW", "/private/fixture", "[31m", "\\u00"] {
            assert!(!text.contains(hidden), "{hidden} leaked: {text}");
        }
        assert!(text.contains("<redacted>"), "{text}");
    }
    let screen = render(&mut state.app, 120, 40)?;
    let text = screen
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        text.contains("visible text") && text.contains("note.txt"),
        "{text}"
    );
    Ok(())
}

#[test]
fn eval_children_settle_and_eval_output_opens_after_the_response_commit() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    let area = Rect::new(0, 0, 120, 40);
    for commit_first in [true, false] {
        let mut state = Capture::new(&fixture)?;
        state.app.set_frame_area(area);
        state.tools.insert("eval".into(), json!({
            "tool":"eval", "args":{"language":"js","summary":"Inspect command output","code":"await tool.bash({command: 'printf alpha'})"},
            "output":"Eval completed"
        }));
        state.intent("eval");
        let parts = state.parts.clone();
        for before_requests in [true, false] {
            if before_requests == commit_first {
                state.action(&json!({"op":"provider-finish"}), &fixture)?;
                state.event(
                    "assistant_message_finished",
                    json!({
                        "request_id":"provider", "tool_call_count":1, "parts":parts
                    }),
                )?;
            }
            if before_requests {
                for id in ["eval", "command-a"] {
                    state.action(&json!({"op":"request", "id":id}), &fixture)?;
                    state.action(&json!({"op":"start", "id":id}), &fixture)?;
                }
            }
        }
        // The child call is coordinator-owned and absent from the provider's response.
        for id in ["command-a", "eval"] {
            state.action(&json!({"op":"finish", "id":id}), &fixture)?;
        }
        state.event(
            "provider_request_started",
            json!({
                "request_id":"provider-next", "provider_id":"fixture", "model_id":"model",
                "prompt_summary":"Continue", "request_digest":"synthetic"
            }),
        )?;
        state.action(&json!({"op":"advance", "ms":1000}), &fixture)?;

        let settled = render(&mut state.app, area.width, area.height)?;
        assert!(!settled.content.chunks(120).any(|row| row
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Eval completed")));
        state.action(&json!({"op":"advance", "ms":330}), &fixture)?;
        let later = render(&mut state.app, area.width, area.height)?;
        for label in ["Inspect command output", "printf alpha"] {
            let row = settled
                .content
                .chunks(120)
                .position(|row| {
                    row.iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>()
                        .contains(label)
                })
                .ok_or_else(|| format!("missing {label}"))?;
            assert_eq!(
                &settled.content[row * 120..(row + 1) * 120],
                &later.content[row * 120..(row + 1) * 120],
                "completed {label} must stop animating"
            );
        }
        state.app.focus = Focus::Details;
        assert!(state.app.select_transcript_tool("eval"));
        state
            .app
            .handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert!(state.app.is_tool_output_expanded_for_test("eval"));
        let opened = render(&mut state.app, area.width, area.height)?;
        assert!(opened.content.chunks(120).any(|row| row
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Eval completed")));
    }
    Ok(())
}

#[test]
fn context_tools_after_thinking_start_a_new_group() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    let mut state = Capture::new(&fixture)?;
    for (request, ids) in [("provider", ["a", "b"]), ("next", ["web-a", "web-b"])] {
        if request == "next" {
            state.parts.clear();
            state.event(
                "provider_request_started",
                json!({"request_id":request,
                    "provider_id":"fixture", "model_id":"model", "prompt_summary":"Continue",
                    "request_digest":"synthetic"}),
            )?;
        }
        state.live(
            "provider_reasoning_delta",
            json!({"request_id":request,
                "delta":"Inspect the next sources."}),
        )?;
        state
            .parts
            .push(json!({"kind":"reasoning", "text":"Inspect the next sources."}));
        state.action(&json!({"op":"advance", "ms":300}), &fixture)?;
        for phase in ["request", "start", "finish", "commit"] {
            if phase == "commit" {
                state.event(
                    "provider_request_finished",
                    json!({"request_id":request, "finish_reason":"tool_calls"}),
                )?;
                state.event(
                    "assistant_message_finished",
                    json!({"request_id":request,
                        "parts":state.parts, "tool_call_count":ids.len()}),
                )?;
            } else {
                for id in ids {
                    state.action(&json!({"op":phase, "id":id}), &fixture)?;
                }
            }
            if request != "next" {
                continue;
            }
            let search_label = if matches!(phase, "request" | "start") {
                "Searching 2 websites"
            } else {
                "Searched 4 websites"
            };
            for width in [40, 120] {
                let buffer = render(&mut state.app, width, 40)?;
                let rows = buffer
                    .content
                    .chunks(usize::from(width))
                    .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                    .collect::<Vec<_>>();
                let thought_rows = rows
                    .iter()
                    .enumerate()
                    .filter_map(|(row, text)| text.contains("Thought for").then_some(row))
                    .collect::<Vec<_>>();
                let read_row = rows.iter().position(|row| row.contains("Read 2 files"));
                let search_row = rows.iter().position(|row| row.contains(search_label));
                assert!(
                    matches!((thought_rows.as_slice(), read_row, search_row),
                        ([first, second], Some(read), Some(search))
                            if *first < read && read < *second && *second < search),
                    "{width} columns, {phase}\n{}",
                    rows.join("\n")
                );
            }
        }
    }
    Ok(())
}
