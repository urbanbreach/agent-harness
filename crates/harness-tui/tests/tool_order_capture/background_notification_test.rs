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
