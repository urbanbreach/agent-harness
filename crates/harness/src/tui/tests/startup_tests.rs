use super::*;
use harness::UnwrapOrAbort;

#[test]
fn tui_startup_carries_unsent_draft_into_new_live_session() {
    // arrange
    // act
    // assert
    let _guard = startup_draft_test_lock().lock().unwrap_or_abort();
    set_pending_live_prompt_draft(None);

    set_pending_live_prompt_draft(Some("draft to keep".to_string()));

    let live = AppState::new_live(None, false, None);
    assert_eq!(live.composer.prompt_buffer, "draft to keep");

    set_pending_live_prompt_draft(None);
}

#[test]
fn workflow_managed_live_tuis_preserve_terminal_between_handoffs() {
    // arrange
    // act
    // assert
    let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
    let sink: UiIntentSink = Arc::new(|_| {});

    let fresh = new_live_tui_options(
        PathBuf::from("/tmp/run-new"),
        Vec::new(),
        rx,
        false,
        Arc::clone(&sink),
        true,
        None,
        None,
    );
    assert!(fresh.preserve_terminal_on_exit);
    assert!(matches!(
        fresh.mode,
        TuiMode::Live {
            compact_session_supported: true,
            ..
        }
    ));

    let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
    let resumed = continue_live_tui_options(
        PathBuf::from("/tmp/run-continue"),
        Vec::new(),
        Vec::new(),
        rx,
        false,
        sink,
        true,
        None,
        None,
    );
    assert!(resumed.preserve_terminal_on_exit);
    assert!(matches!(
        resumed.mode,
        TuiMode::Live {
            compact_session_supported: true,
            ..
        }
    ));
}

#[test]
fn new_live_tui_options_allow_pre_bootstrap_run_directory() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_projected_new_session");
    let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
    let sink: UiIntentSink = Arc::new(|_| {});

    let options = new_live_tui_options(
        run_dir.clone(),
        Vec::new(),
        rx,
        false,
        sink,
        true,
        None,
        None,
    );

    let TuiMode::Live {
        run_dir: configured_run_dir,
        historical_events,
        ..
    } = options.mode
    else {
        panic!("expected live TUI mode");
    };
    assert_eq!(configured_run_dir, run_dir);
    assert!(historical_events.is_empty());
    assert!(options.preserve_terminal_on_exit);
}

#[tokio::test]
async fn session_history_refresh_sends_bootstrapped_catalog() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_projected_new_session");
    write_catalog_run(&run_dir, &catalog_events("run_projected_new_session"));
    let (tx, rx) = std_mpsc::channel::<LiveUpdate>();

    await_task(
        "session history refresh",
        spawn_session_history_refresh(temp_dir.path().to_path_buf(), tx),
    )
    .await
    .unwrap_or_abort();

    let update = rx.try_recv().unwrap_or_abort();
    let LiveUpdate::SessionHistory(entries) = update else {
        panic!("expected session history update");
    };
    assert!(entries
        .iter()
        .any(|entry| entry.catalog.run_id == "run_projected_new_session"));
}

#[test]
fn resumed_live_tui_options_carry_normalized_lineage_history() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let root_dir = temp_dir.path().join("root_session");
    let child_dir = temp_dir.path().join("child_session");
    write_catalog_run(&root_dir, &catalog_events("root_session"));
    write_catalog_run(&child_dir, &catalog_events("child_session"));
    std::fs::write(
        child_dir.join("meta.json"),
        r#"{"harness_lineage":{"harness_source_run_id":"root_session"}}"#,
    )
    .unwrap_or_abort();

    let entries = load_live_session_history_entries(&child_dir, temp_dir.path()).unwrap_or_abort();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.catalog.run_id == "child_session")
            .and_then(|entry| entry.catalog.parent_session_id.as_deref()),
        Some("root_session")
    );

    let (_tx, rx) = std_mpsc::channel::<LiveUpdate>();
    let sink: UiIntentSink = Arc::new(|_| {});
    let options = continue_live_tui_options(
        child_dir,
        Vec::new(),
        entries,
        rx,
        false,
        sink,
        true,
        None,
        None,
    );

    let TuiMode::Live {
        session_history_entries,
        ..
    } = options.mode
    else {
        panic!("expected live TUI mode");
    };
    assert_eq!(session_history_entries.len(), 2);
    assert!(session_history_entries.iter().any(|entry| {
        entry.catalog.run_id == "child_session"
            && entry.catalog.parent_session_id.as_deref() == Some("root_session")
    }));
}
