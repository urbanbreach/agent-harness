use super::*;
use harness::UnwrapOrAbort;

#[test]
fn tui_lineage_clone_materializes_child_from_memory_snapshot() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let source_run_dir = temp_dir.path().join("run_tui_lineage_source");
    std::fs::create_dir(&source_run_dir).unwrap_or_abort();
    std::fs::write(source_run_dir.join(".writer.lock"), "locked").unwrap_or_abort();
    let events = stable_lineage_test_events();
    let stable_prefix =
        harness_core::session_lineage::latest_clone_stable_prefix(&events).unwrap_or_abort();

    let notice =
        materialize_tui_lineage_child("clone", source_run_dir.clone(), events, stable_prefix);

    let LiveUpdate::OperatorNotice { message, level } = notice else {
        panic!("expected lineage operator notice");
    };
    assert_eq!(level, OperatorNoticeLevel::Info);
    assert!(
        message.starts_with("Harness session clone created run_harness_child"),
        "unexpected success message: {message}"
    );
    assert!(
        temp_dir
            .path()
            .read_dir()
            .unwrap_or_abort()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("run_harness_child")),
        "expected published child run beside source"
    );
}

#[test]
fn tui_lineage_fork_continues_child_with_prompt_draft() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let source_run_dir = temp_dir.path().join("run_tui_lineage_source");
    std::fs::create_dir(&source_run_dir).unwrap_or_abort();
    std::fs::write(source_run_dir.join(".writer.lock"), "locked").unwrap_or_abort();
    let events = active_stable_lineage_test_events();
    let stable_prefix = harness_core::session_lineage::validate_tui_fork_stable_prefix(
        &events,
        events.len() as u64,
    )
    .unwrap_or_abort();

    let update = materialize_tui_fork_child(
        source_run_dir,
        events,
        stable_prefix,
        "repeat this prompt".to_string(),
    );

    let LiveUpdate::ContinueSession {
        run_id,
        run_dir,
        prompt_draft,
    } = update
    else {
        panic!("expected fork continuation update");
    };
    assert_eq!(prompt_draft, "repeat this prompt");
    assert_eq!(run_id, run_dir.file_name().unwrap().to_string_lossy());
    let child_events = load_events_from_run_dir(&run_dir).unwrap_or_abort();
    assert!(matches!(
        child_events.last().map(|event| &event.payload),
        Some(EventV1::RunFinished(_))
    ));
    let resume_plan = inspect_resume_plan(&run_dir);
    assert!(
        resume_plan.is_resumable,
        "child should be resumable: {:?}",
        resume_plan.resume_disabled_reason
    );
}

#[test]
fn tui_lineage_fork_first_prompt_uses_recorded_runtime_context_for_resume() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let source_run_dir = temp_dir.path().join("run_tui_lineage_source");
    std::fs::create_dir(&source_run_dir).unwrap_or_abort();
    std::fs::write(source_run_dir.join(".writer.lock"), "locked").unwrap_or_abort();
    write_recorded_runtime_context_meta(&source_run_dir);
    let events = first_prompt_lineage_test_events();
    let stable_prefix = harness_core::session_lineage::validate_tui_fork_stable_prefix(
        &events,
        events.len() as u64,
    )
    .unwrap_or_abort();

    let update = materialize_tui_fork_child(
        source_run_dir,
        events,
        stable_prefix,
        "first prompt".to_string(),
    );

    let LiveUpdate::ContinueSession { run_dir, .. } = update else {
        panic!("expected fork continuation update");
    };
    let resume_plan = inspect_resume_plan(&run_dir);
    assert_eq!(
        resume_plan.provider_model.as_deref(),
        Some("default/gpt-5.5")
    );
    assert!(
        resume_plan.is_resumable,
        "child should be resumable from copied metadata: {:?}",
        resume_plan.resume_disabled_reason
    );
}
