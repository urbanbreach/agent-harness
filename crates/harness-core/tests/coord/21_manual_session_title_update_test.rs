#[tokio::test]
async fn coordinator_update_session_title_appends_event_and_updates_metadata() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = spawn_coordinator(
        CoordinatorConfig::new(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("initial title", temp_dir.path())
        .await
        .expect("start run");

    let updated = coordinator
        .update_session_title(" renamed session ")
        .await
        .expect("update title");

    assert_eq!(updated.run_id, run.run_id);
    assert_eq!(updated.run_name, "renamed session");
    let events = load_events(&run.events_path);
    assert_eq!(
        events.iter().rev().find_map(|event| match &event.payload {
            EventV1::SessionTitleUpdated(payload) => Some(payload.title.as_str()),
            _ => None,
        }),
        Some("renamed session")
    );
    let meta = fs::read_to_string(run.run_dir.join("meta.json")).expect("read meta");
    assert!(
        meta.contains("\"run_name\": \"renamed session\""),
        "metadata should reflect renamed title: {meta}"
    );
}

#[tokio::test]
async fn coordinator_update_session_title_rejects_empty_titles() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = spawn_coordinator(
        CoordinatorConfig::new(temp_dir.path()),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("initial title", temp_dir.path())
        .await
        .expect("start run");

    let err = coordinator
        .update_session_title("   ")
        .await
        .expect_err("empty title should be rejected");

    assert!(matches!(err, CoordinatorError::InvalidSessionTitle));
}
