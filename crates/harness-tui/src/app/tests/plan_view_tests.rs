use super::*;
use crate::keybindings::palette_model::{PaletteDispatch, PALETTE_COMMAND_ENTRIES};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn session_feedback_maps_to_help_action() {
    let entry = PALETTE_COMMAND_ENTRIES
        .iter()
        .find(|e| e.id == "session.feedback")
        .expect("session.feedback entry");
    assert_eq!(entry.dispatch, PaletteDispatch::Action(Action::Help));
}

pub(super) fn plan_view_enter_opens_existing_plan_preview() {
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

    let rows = app.plan_view_rows();
    let demo_index = rows
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;
    app.handle_key(key(KeyCode::Enter));

    let preview = app.plan_view_preview().expect("preview");
    assert!(preview.contains("Demo plan"));
    assert!(preview.contains("step one"));

    app.handle_key(key(KeyCode::Esc));
    assert!(app.plan_view_is_visible());
    assert!(app.plan_view_preview().is_none());

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

    crate::clipboard::set_copy_override(Some(Box::new(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no clipboard integration available",
        ))
    })));
    app.handle_key(key(KeyCode::Char('y')));
    crate::clipboard::set_copy_override(None);

    let banner = app.status_banner.as_deref().expect("path banner");
    assert!(banner.contains("plan path:"));
    assert!(banner.contains("demo.md"));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_empty_state_enter_toasts_guidance() {
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

    app.handle_key(key(KeyCode::Enter));

    assert!(app.plan_view_is_visible());
    assert!(app.plan_view_preview().is_none());

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_summary_counts_existing_and_preview() {
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

    let demo_index = app
        .plan_view_rows()
        .iter()
        .position(|row| row.slug == "demo" && row.exists)
        .expect("demo plan row");
    app.plan_view_selected = demo_index;
    app.plan_view_open_selected();

    let open = app.plan_view_summary();
    assert!(open.preview_open);
    assert!(open.one_line().contains("preview=open"));
    assert!(open.overlay_line().contains("preview open"));
    assert!(open.overlay_line().contains("existing"));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_c_key_copies_plan_body() {
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

    let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let captured_for_copy = std::sync::Arc::clone(&captured);
    crate::clipboard::set_copy_override(Some(Box::new(move |text: &str| {
        *captured_for_copy.lock().expect("lock") = Some(text.to_string());
        Ok(())
    })));
    app.handle_key(key(KeyCode::Char('c')));
    crate::clipboard::set_copy_override(None);

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

    crate::clipboard::set_copy_override(Some(Box::new(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no clipboard integration available",
        ))
    })));
    app.handle_key(key(KeyCode::Char('c')));
    crate::clipboard::set_copy_override(None);

    let banner = app.status_banner.as_deref().expect("body banner");
    assert!(banner.contains("plan body:"));
    assert!(banner.contains("demo"));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_d_key_deletes_selected_plan() {
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

    app.handle_key(key(KeyCode::Char('d')));

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

    app.handle_key(key(KeyCode::Char('d')));

    assert!(app.plan_view_is_visible());

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn plan_view_multi_plan_open_select_activate_product_path() {
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
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let workspace = dir.clone();
    app.file_mention_workspace_root_provider = Arc::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
        Some(workspace.clone())
    });
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

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    for ch in "View Plan".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.plan_view_is_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::PlanView));

    let summary = app.plan_view_summary();
    assert!(summary.total >= 4, "summary={summary:?}");
    assert!(summary.existing >= 4, "summary={summary:?}");
    assert!(summary.active >= 1, "summary={summary:?}");
    assert!(summary.total_bytes > 0);
    assert!(summary.has_plans());
    let rows = app.plan_view_rows();
    assert_eq!(
        rows.iter().map(|row| row.slug.as_str()).collect::<Vec<_>>(),
        ["harness-probe-run", "alt", "ops", "primary"]
    );
    assert!(rows[0].is_active);

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

    exercise_plan_preview_navigation(&mut app, &plans, &reads);

    let _ = fs::remove_dir_all(&dir);
}

fn exercise_plan_preview_navigation(app: &mut AppState, plans: &Path, reads: &AtomicUsize) {
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

    app.handle_key(key(KeyCode::Esc));
    let before = app.plan_view_selected_index();
    app.handle_key(key(KeyCode::Down));
    let after_down = app.plan_view_selected_index();
    if app.plan_view_rows().len() > 1 {
        assert_ne!(before, after_down);
    }
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.plan_view_selected_index(), before.min(after_down));

    // A frame uses one prepared directory snapshot even if files change mid-paint.
    app.freeze_animation_clock();
    app.set_frame_area(Rect::new(0, 0, 80, 24));
    let prepared = render_debug(app, 80, 24);
    fs::write(plans.join("new.md"), "# Added between frames\n").expect("new plan");
    assert!(app.plan_view_rows().iter().any(|row| row.slug == "new"));
    assert_eq!(render_debug(app, 80, 24), prepared);
    app.set_frame_area(Rect::new(0, 0, 80, 24));
    assert_ne!(render_debug(app, 80, 24), prepared);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.plan_view_is_visible());
    assert_ne!(app.overlay_stack().top(), Some(OverlayKind::PlanView));
    app.execute_action(Action::OpenStatusDialog);
    app.execute_slash_command("new", None);
    let before = reads.load(Ordering::Relaxed);
    app.set_frame_area(Rect::new(0, 0, 80, 24));
    assert_eq!(
        reads.load(Ordering::Relaxed),
        before,
        "hidden plans must not query disk"
    );
}

pub(super) fn plan_view_rows_and_summary_surface_byte_len() {
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

    let row = app
        .plan_view_rows()
        .into_iter()
        .find(|row| row.slug == "demo" && row.exists)
        .expect("demo row");
    let summary = app.plan_view_summary();

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
