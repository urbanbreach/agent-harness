#[test]
fn replace_events_resets_question_identity_before_reusing_permission_id() {
    // arrange — Given initialized prompt state for a pending question.
    let event = custom_question_event("question_reused_id", false);
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event.clone());
    app.handle_key(key(KeyCode::Down));

    // act — When replay replacement restores a question with the same permission id.
    app.replace_events(vec![event]);
    app.handle_key(key(KeyCode::Enter));

    // assert — Then state is reinitialized rather than indexing stale empty vectors.
    assert_eq!(
        app.question_prompt_answers("question_reused_id"),
        vec![vec!["A".to_string()]]
    );
}

#[test]
fn new_session_resets_question_identity_before_reusing_permission_id() {
    // arrange — Given a locally hidden question whose prompt state has been initialized.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_new_reused_id", false));
    app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));

    // act — When a new session receives a question with the same permission id.
    app.execute_slash_command("new", None);
    app.ingest_event(custom_question_event("question_new_reused_id", false));
    app.handle_key(key(KeyCode::Enter));

    // assert — Then prompt vectors are reinitialized and the default option submits normally.
    assert_eq!(
        app.question_prompt_answers("question_new_reused_id"),
        vec![vec!["A".to_string()]]
    );
}
