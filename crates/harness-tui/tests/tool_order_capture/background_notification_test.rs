#[test]
fn background_notification_keeps_launch_identity_and_replays_without_a_user_message() -> Result<()>
{
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    for (delivery, status, verb) in [Some("turn"), None].into_iter().flat_map(|delivery| {
        [("completed", "completed"), ("cancelled", "cancelled"), ("failed", "failed"), ("timed_out", "timed out")]
            .map(|(status, verb)| (delivery, status, verb))
    }) {
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
            "task_id":"scheduled-task", "description":"Inspect renderer", "status":status,
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
            assert!(state.app.select_transcript_tool("task"));
            state.app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
            let buffer = render(&state.app, 120, 40)?;
            let text = buffer
                .content
                .chunks(120)
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("Ran 1 subagent"), "{text}");
            assert!(
                text.contains(&format!("Subagent {verb}:")) && text.contains("0.4s"),
                "{text}"
            );
            assert!(!text.contains("Background task cancelled"), "{text}");
            if delivery.is_some() {
                let started = text
                    .find(&format!("Subagent {verb}:"))
                    .ok_or("missing original launch row")?;
                let finished = text
                    .rfind(&format!("Subagent {verb}:"))
                    .ok_or("missing terminal row")?;
                assert!(started < finished, "{text}");
            }
            assert_eq!(state.app.canonical_projection_error(), None);
        }
    }
    Ok(())
}

#[test]
#[expect(clippy::cognitive_complexity, reason = "one lifecycle matrix covers status, metadata, folding, and replay at three terminal widths")]
fn subagent_rows_show_current_status_and_model_without_unfolding() -> Result<()> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    let mut state = Capture::new(&fixture)?;
    for (id, agent, model, description) in [
        ("core", "explore", "gpt-5.4-mini", "Inspect core runtime architecture"),
        ("tools", "review", "gpt-5.4", "Inspect provider, tool, and CLI layers"),
        ("ui", "build", "gpt-5.4-mini", "Inspect TUI, testkit, and test infrastructure"),
    ] {
        state.tools.insert(id.into(), json!({"tool":"task", "args":{
            "description":description, "subagent_type":agent, "model":model}, "output":"Complete"}));
        state.intent(id);
        state.event("tool_call_requested", json!({"tool_call_id":id, "tool_id":"task",
            "args_summary":state.tools[id]["args"].to_string(), "args_digest":"synthetic",
            "metadata":{"lineage":{"child_session_id":format!("child-{id}"),
                "child_request_id":format!("request-{id}")}}}))?;
        state.action(&json!({"op":"start", "id":id}), &fixture)?;
    }
    let directory = std::env::var_os("HARNESS_SUBAGENT_ARTIFACT_DIR");
    if let Some(ref directory) = directory {
        fs::create_dir_all(directory)?;
    }
    for phase in ["running", "waiting", "mixed", "settled", "replay"] {
        match phase {
            "waiting" => state.action(&json!({"op":"wait", "id":"ui"}), &fixture)?,
            "mixed" => {
                state.action(&json!({"op":"unwait", "id":"ui"}), &fixture)?;
                state.event("task_completed", json!({"task_id":"scheduled-core",
                    "result_summary":"Complete", "result_digest":"synthetic", "metadata":{"task_scope":"tool_call"}}))?;
                state.event("tool_call_finished", json!({"tool_call_id":"core", "status":"succeeded",
                    "output_summary":"Complete", "output_json":{"route":{"resolved_profile":"explore",
                        "model":{"model":"gpt-5.4"}}}}))?;
            }
            "settled" => {
                state.event("task_cancelled", json!({"task_id":"scheduled-tools",
                    "reason":"Cancelled", "task_scope":"tool_call"}))?;
                state.event("task_cancelled", json!({"task_id":"scheduled-ui",
                    "reason":"Provider failed", "task_scope":"tool_call"}))?;
                state.event("tool_call_finished", json!({"tool_call_id":"ui", "status":"failed",
                    "output_summary":"Provider failed"}))?;
            }
            "replay" => state.action(&json!({"op":"replay"}), &fixture)?,
            _ => {}
        }
        for width in [40, 80, 120] {
            state.app.set_frame_area(Rect::new(0, 0, width, 30));
            state.app.focus = Focus::Prompt;
            if width == 40 && matches!(phase, "settled" | "replay") {
                // Completed groups remain available through the existing fold control.
                state.app.focus = Focus::Details;
                assert!(state.app.select_transcript_tool("core"));
                state.app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
            }
            let buffer = render(&state.app, width, 30)?;
            let text = buffer.content.chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>().join("\n");
            for agent in ["Explore", "Review", "Build"] {
                assert!(text.contains(agent), "missing {agent} at {width}, {phase}\n{text}");
            }
            assert!(text.contains("gpt-5.4"), "{text}");
            let (running, completed, cancelled, failed) = match phase {
                "running" => (3, 0, 0, 0),
                "waiting" => (2, 0, 0, 0),
                "mixed" => (2, 1, 0, 0),
                _ => (0, 1, 1, 1),
            };
            for (label, count) in [("running:", running), ("completed:", completed),
                ("cancelled:", cancelled), ("failed:", failed)] {
                assert_eq!(text.matches(label).count(), count, "{width}, {phase}\n{text}");
            }
            if phase == "waiting" {
                assert!(text.contains("waiting for approval"), "{text}");
            }
            if phase == "mixed" {
                assert!(text.contains("Running 2 subagents, 1 done"), "{text}");
                assert!(text.contains("Explore · gpt-5.4"), "{text}");
                assert!(!text.contains("Explore · gpt-5.4-mini"), "{text}");
            }
            if let Some(ref directory) = directory {
                let mut bytes = Vec::new();
                {
                    let mut terminal = Terminal::with_options(CrosstermBackend::new(&mut bytes),
                        TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, width, 30)) })?;
                    terminal.draw(|frame| render_app(frame, &state.app))?;
                }
                fs::write(Path::new(directory).join(format!("subagents-{phase}-{width}x30-motion-0ms.ansi")), bytes)?;
            }
        }
    }
    Ok(())
}
