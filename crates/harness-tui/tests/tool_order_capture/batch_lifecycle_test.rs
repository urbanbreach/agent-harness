use super::*;

#[test]
fn batch_children_settle_and_batch_output_opens_after_the_response_commit() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    let area = Rect::new(0, 0, 120, 40);
    for commit_first in [true, false] {
        let mut state = Capture::new(&fixture)?;
        state.app.set_frame_area(area);
        state.tools.insert("batch".into(), json!({
            "tool":"batch", "args":{"tool_calls":[{"tool":"bash", "parameters":{"command":"printf alpha"}}]},
            "output":"Batch completed"
        }));
        state.intent("batch");
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
                for id in ["batch", "command-a"] {
                    state.action(&json!({"op":"request", "id":id}), &fixture)?;
                    state.action(&json!({"op":"start", "id":id}), &fixture)?;
                }
            }
        }
        // The child call is coordinator-owned and absent from the provider's response.
        for id in ["command-a", "batch"] {
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

        let view = state
            .app
            .transcript_view_model()
            .ok_or("missing transcript")?;
        let tools = view
            .blocks
            .iter()
            .filter(|block| block.kind == harness_tui::transcript_blocks::BlockKind::Tool)
            .collect::<Vec<_>>();
        assert_eq!(tools.len(), 2);
        assert!(
            tools.iter().all(|tool| {
                tool.lifecycle == harness_tui::transcript_blocks::BlockLifecycle::Completed
                    && tool.fold_state == harness_tui::transcript_blocks::FoldState::Collapsed
            }),
            "completed tool previews must stay collapsed while the turn continues"
        );

        let settled = render(&state.app, area.width, area.height)?;
        state.action(&json!({"op":"advance", "ms":330}), &fixture)?;
        let later = render(&state.app, area.width, area.height)?;
        for label in ["Batch 1 tool", "printf alpha"] {
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
        assert!(state.app.select_transcript_tool("batch"));
        state
            .app
            .handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert!(state.app.is_tool_output_expanded_for_test("batch"));
        let opened = render(&state.app, area.width, area.height)?;
        assert!(opened.content.chunks(120).any(|row| row
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Batch completed")));
    }
    Ok(())
}
