use super::*;

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
