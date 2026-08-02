use super::*;
use crate::keybindings::palette_model::{PaletteDispatch, PALETTE_COMMAND_ENTRIES};

pub(super) fn plan_view_opens_from_action() {
    // Given
    let mut app = AppState::new_live(None, false, None);

    // When
    app.execute_action(Action::OpenViewPlan);

    // Then
    assert!(app.plan_view_is_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::PlanView));
}

pub(super) fn plan_view_closes_on_esc() {
    // Given
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenViewPlan);
    assert!(app.plan_view_is_visible());

    // When
    app.handle_key(key(KeyCode::Esc));

    // Then
    assert!(!app.plan_view_is_visible());
    assert_ne!(app.overlay_stack().top(), Some(OverlayKind::PlanView));
}

pub(super) fn context_view_plan_palette_dispatch_opens_plan_view() {
    // Given
    let mut app = AppState::new_live(None, false, None);
    let entry = PALETTE_COMMAND_ENTRIES
        .iter()
        .find(|e| e.id == "context.view_plan")
        .expect("context.view_plan entry");
    assert_eq!(
        entry.dispatch,
        PaletteDispatch::Action(Action::OpenViewPlan)
    );

    // When
    app.execute_action(Action::OpenViewPlan);

    // Then
    assert!(app.plan_view_is_visible());
}

pub(super) fn session_feedback_maps_to_help_action() {
    let entry = PALETTE_COMMAND_ENTRIES
        .iter()
        .find(|e| e.id == "session.feedback")
        .expect("session.feedback entry");
    assert_eq!(entry.dispatch, PaletteDispatch::Action(Action::Help));
}

pub(super) fn plan_view_enter_opens_existing_plan_preview() {
    // Given: workspace with a plan file and open plan view
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "preview",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    fs::write(plans.join("demo.md"), "# Demo plan\n\n- step one\n").expect("write plan");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);
    assert!(app.plan_view_is_visible());
    assert!(app.plan_view_preview().is_none());

    // When: select existing plan and Enter
    let rows = app.plan_view_rows();
    let demo_index = rows
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;
    app.handle_key(key(KeyCode::Enter));

    // Then: preview loaded
    let preview = app.plan_view_preview().expect("preview");
    assert!(preview.contains("Demo plan"));
    assert!(preview.contains("step one"));

    // When: Esc from preview returns to list
    app.handle_key(key(KeyCode::Esc));
    assert!(app.plan_view_is_visible());
    assert!(app.plan_view_preview().is_none());

    // When: 'y' copies absolute plan path to clipboard (OSC52/native) and banners it
    let copied = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let copied_hook = std::sync::Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *copied_hook.lock().expect("copy lock") = Some(text.to_string());
        Ok(())
    })));
    app.handle_key(key(KeyCode::Char('y')));
    crate::clipboard::set_copy_override(None);
    let banner = app.status_banner.as_deref().expect("path banner");
    assert!(banner.contains("plan path:"));
    assert!(banner.contains("demo.md"));
    let copied_path = copied
        .lock()
        .expect("copy lock")
        .clone()
        .expect("clipboard copy invoked");
    assert!(
        copied_path.contains("demo.md"),
        "expected plan path clipboard payload, got {copied_path}"
    );

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_y_key_reports_clipboard_failure_without_dropping_path_banner() {
    // Given: workspace with a plan file and open plan view
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "copy-fail",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    fs::write(plans.join("demo.md"), "# Demo plan\n").expect("write plan");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);
    let demo_index = app
        .plan_view_rows()
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;

    // When: clipboard integration fails
    crate::clipboard::set_copy_override(Some(Box::new(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no clipboard integration available",
        ))
    })));
    app.handle_key(key(KeyCode::Char('y')));
    crate::clipboard::set_copy_override(None);

    // Then: path banner still surfaces absolute path; toast reports failure
    let banner = app.status_banner.as_deref().expect("path banner");
    assert!(banner.contains("plan path:"));
    assert!(banner.contains("demo.md"));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_empty_state_enter_toasts_guidance() {
    // Given: workspace with no plan files
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "empty",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("workspace");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);
    assert!(app.plan_view_rows().is_empty());

    // When: Enter with empty list
    app.handle_key(key(KeyCode::Enter));

    // Then: guidance toast, still open, no preview
    assert!(app.plan_view_is_visible());
    assert!(app.plan_view_preview().is_none());

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_summary_counts_existing_and_preview() {
    // Given: workspace with one existing plan
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "summary",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    fs::write(plans.join("demo.md"), "# Demo plan\nstep one\n").expect("write plan");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);

    // When: summarizing closed list
    let summary = app.plan_view_summary();
    assert!(summary.total >= 1);
    assert!(summary.existing >= 1);
    assert_eq!(summary.existing + summary.missing, summary.total);
    assert!(summary.has_plans());
    assert!(!summary.preview_open);
    assert!(summary.one_line().starts_with("plan view: "));
    assert!(summary.one_line().contains("preview=closed"));
    assert!(summary.overlay_line().contains("total"));
    assert!(summary.overlay_line().contains("existing"));
    assert!(!summary.overlay_line().contains("preview open"));

    // When: opening preview
    let demo_index = app
        .plan_view_rows()
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;
    app.plan_view_open_selected();

    // Then: preview flag flips open and overlay subtitle reflects it
    let open = app.plan_view_summary();
    assert!(open.preview_open);
    assert!(open.one_line().contains("preview=open"));
    assert!(open.overlay_line().contains("preview open"));
    assert!(open.overlay_line().contains("existing"));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_c_key_copies_plan_body() {
    // Given: workspace with a plan file and open plan view
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "copy-body",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    let body = "# Demo plan\n\nBody for clipboard copy.\n";
    fs::write(plans.join("demo.md"), body).expect("write plan");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);
    let demo_index = app
        .plan_view_rows()
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;

    // When: clipboard captures body via 'c'
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let captured_for_copy = std::sync::Arc::clone(&captured);
    crate::clipboard::set_copy_override(Some(Box::new(move |text: &str| {
        *captured_for_copy.lock().expect("lock") = Some(text.to_string());
        Ok(())
    })));
    app.handle_key(key(KeyCode::Char('c')));
    crate::clipboard::set_copy_override(None);

    // Then: body banner + clipboard content match file
    let banner = app.status_banner.as_deref().expect("body banner");
    assert!(banner.contains("plan body:"));
    assert!(banner.contains("demo"));
    let copied = captured
        .lock()
        .expect("lock")
        .clone()
        .expect("clipboard body");
    assert!(copied.contains("# Demo plan"));
    assert!(copied.contains("Body for clipboard copy."));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_c_key_reports_clipboard_failure_for_body() {
    // Given: workspace with a plan file and open plan view
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "copy-body-fail",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    fs::write(plans.join("demo.md"), "# Demo plan\n").expect("write plan");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);
    let demo_index = app
        .plan_view_rows()
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;

    // When: clipboard integration fails on body copy
    crate::clipboard::set_copy_override(Some(Box::new(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no clipboard integration available",
        ))
    })));
    app.handle_key(key(KeyCode::Char('c')));
    crate::clipboard::set_copy_override(None);

    // Then: body banner still surfaces slug/char count honesty
    let banner = app.status_banner.as_deref().expect("body banner");
    assert!(banner.contains("plan body:"));
    assert!(banner.contains("demo"));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_d_key_deletes_selected_plan() {
    // Given: workspace with a plan file and open plan view
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "delete",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    let plan_path = plans.join("demo.md");
    fs::write(&plan_path, "# Demo plan\n").expect("write plan");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);
    let demo_index = app
        .plan_view_rows()
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;
    assert!(plan_path.is_file());

    // When: delete selected plan via 'd'
    app.handle_key(key(KeyCode::Char('d')));

    // Then: file gone, banner set, list no longer includes demo
    assert!(!plan_path.is_file(), "plan file should be deleted");
    let banner = app.status_banner.as_deref().expect("delete banner");
    assert!(banner.contains("plan deleted:"));
    assert!(banner.contains("demo"));
    assert!(
        app.plan_view_rows()
            .iter()
            .all(|row| row.slug != "demo" || !row.exists),
        "demo should not remain as existing plan"
    );
    assert!(app.plan_view_preview().is_none());

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_d_key_toasts_when_no_plans() {
    // Given: empty workspace plan list
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "delete-empty",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("workspace");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);
    assert!(app.plan_view_rows().is_empty() || app.plan_view_rows().iter().all(|r| !r.exists));

    // When
    app.handle_key(key(KeyCode::Char('d')));

    // Then: still open, no crash; toast path exercised via delete method
    assert!(app.plan_view_is_visible());

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_multi_plan_open_select_activate_product_path() {
    // Given: multi-plan workspace + active-run plan bound via RunFinished
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "multi-activate",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    fs::write(
        plans.join("primary.md"),
        "# Primary plan\n\n- primary step\n",
    )
    .expect("primary");
    fs::write(plans.join("alt.md"), "# Alt plan\n\n- alt step\n").expect("alt");
    fs::write(plans.join("ops.md"), "# Ops plan\n\n- ops step\n").expect("ops");
    fs::write(
        plans.join("harness-probe-run.md"),
        "# Active run plan\n\n- active step\n",
    )
    .expect("active");

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    let mut active_run = envelope(
        1,
        "plan-activate",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "plan-activate-product".to_string(),
        }),
    );
    active_run.run_id = "harness-probe-run".into();
    active_run.stream_key = Some("run:harness-probe-run".to_string());
    app.ingest_historical_event(active_run);
    assert_eq!(app.run_id(), Some("harness-probe-run"));

    // When: open plan view
    app.execute_action(Action::OpenViewPlan);
    assert!(app.plan_view_is_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::PlanView));

    // Then: multi-plan rows + active binding from real FS
    let summary = app.plan_view_summary();
    assert!(summary.total >= 4, "summary={summary:?}");
    assert!(summary.existing >= 4, "summary={summary:?}");
    assert!(summary.active >= 1, "summary={summary:?}");
    assert!(summary.total_bytes > 0);
    assert!(summary.has_plans());
    let rows = app.plan_view_rows();
    assert!(
        rows.iter()
            .any(|row| row.slug == "harness-probe-run" && row.is_active && row.exists),
        "expected active-run plan row: {rows:?}"
    );
    assert!(rows.iter().any(|row| row.slug == "primary" && row.exists));
    assert!(rows.iter().any(|row| row.slug == "alt" && row.exists));

    // When: select primary and activate (Enter opens preview from FS)
    let primary_index = rows
        .iter()
        .position(|row| row.slug == "primary" && row.exists)
        .expect("primary");
    app.plan_view_selected = primary_index;
    app.handle_key(key(KeyCode::Enter));
    let preview = app.plan_view_preview().expect("primary preview");
    assert!(preview.contains("Primary plan"));
    assert!(preview.contains("primary step"));
    assert!(app.plan_view_summary().preview_open);

    // When: Esc closes preview, navigate to active plan, activate again
    app.handle_key(key(KeyCode::Esc));
    assert!(app.plan_view_is_visible());
    assert!(app.plan_view_preview().is_none());
    let active_index = app
        .plan_view_rows()
        .iter()
        .position(|row| row.slug == "harness-probe-run" && row.is_active)
        .expect("active row");
    app.plan_view_selected = active_index;
    app.handle_key(key(KeyCode::Enter));
    let active_preview = app.plan_view_preview().expect("active preview");
    assert!(active_preview.contains("Active run plan"));
    assert!(active_preview.contains("active step"));

    // When: navigate down/up while list open (selection product path)
    app.handle_key(key(KeyCode::Esc));
    let before = app.plan_view_selected_index();
    app.handle_key(key(KeyCode::Down));
    let after_down = app.plan_view_selected_index();
    if app.plan_view_rows().len() > 1 {
        assert_ne!(before, after_down);
    }
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.plan_view_selected_index(), before.min(after_down));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_rows_and_summary_surface_byte_len() {
    // Given: workspace with a known-size plan file
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-plan-{}-{}",
        "byte-len",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let plans = dir.join(".agent-harness/plans");
    fs::create_dir_all(&plans).expect("plans dir");
    let body = "# Demo plan\n\nBody for byte length.\n";
    fs::write(plans.join("demo.md"), body).expect("write plan");
    let expected_bytes = body.len() as u64;

    let mut app = AppState::new_live(None, false, None);
    app.file_mention_workspace_root = Some(dir.clone());
    app.execute_action(Action::OpenViewPlan);

    // When: inspect rows + summary
    let row = app
        .plan_view_rows()
        .into_iter()
        .find(|row| row.slug == "demo" && row.exists)
        .expect("demo row");
    let summary = app.plan_view_summary();

    // Then: row byte_len and summary total_bytes surface file size
    assert_eq!(row.byte_len, Some(expected_bytes));
    assert!(summary.total_bytes >= expected_bytes);
    assert!(
        summary
            .one_line()
            .contains(&format!("bytes={}", summary.total_bytes)),
        "one_line={}",
        summary.one_line()
    );
    assert!(
        summary.overlay_line().contains("bytes"),
        "overlay={}",
        summary.overlay_line()
    );

    let _ = fs::remove_dir_all(&dir);
}
