use super::*;
use crate::UnwrapOrAbort;

pub(super) fn slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_reduced_motion_for_evidence(true);
    app.restart_motion_epoch_for_evidence();
    let mut seq = 0;
    for turn in ["older turn one", "older turn two"] {
        for (kind, data) in [
            (
                "user_message_submitted",
                serde_json::json!({"request_id":turn,"text":turn}),
            ),
            (
                "provider_request_started",
                serde_json::json!({"request_id":turn,"provider_id":"mock","model_id":"model","prompt_summary":"fixture","request_digest":"fixture"}),
            ),
            (
                "assistant_message_finished",
                serde_json::json!({"request_id":turn,"tool_call_count":0,"parts":[{"kind":"text","text":turn}]}),
            ),
            (
                "provider_request_finished",
                serde_json::json!({"request_id":turn,"finish_reason":"stop"}),
            ),
        ] {
            seq += 1;
            app.ingest_event(envelope(
                seq,
                turn,
                serde_json::from_value(serde_json::json!({"event_type":kind,"data":data}))
                    .unwrap_or_abort(),
            ));
        }
    }
    let paint = |app: &mut AppState| {
        let area = Rect::new(0, 0, 100, 40);
        app.set_frame_area(area);
        crate::render_test::render_to_string(app, area, |app, frame, _| render_app(frame, app))
    };
    assert!(paint(&mut app).contains("older turn two"));
    for ch in "/new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.startup_shell_visible());
    assert!(!paint(&mut app).contains("older turn"));

    app.clear_prompt_input();
    for ch in "fresh run".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(!paint(&mut app).contains("older turn"));
    assert!(app.should_quit);
    assert!(!app.startup_shell_visible());
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewSession]
        ),
        "/new startup handoff must select a fresh session, not submit to the old live run"
    );

    let relaunched = AppState::new_live(None, false, None);
    assert_eq!(relaunched.composer.prompt_buffer, "");
    assert_eq!(
        relaunched.composer.prompt_history,
        vec!["fresh run".to_string()]
    );
    assert_eq!(
        relaunched
            .activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("fresh run")
    );
}
