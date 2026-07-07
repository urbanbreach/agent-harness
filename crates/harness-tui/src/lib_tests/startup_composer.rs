use super::*;
use crate::UnwrapOrAbort;

pub(super) fn startup_home_screen_renders_compose_first_shell() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4")
            .with_available_models(vec![
                app::ModelOption::from_model_ref("deep", "proxy:gpt-5.4"),
                app::ModelOption::from_model_ref("planner", "proxy:gpt-5.4-mini"),
            ])
            .with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 160, 48);
    assert!(rendered.contains("██╗  ██╗"));
    assert!(!rendered.contains("Launch: deep · gpt-5.4"));
    assert!(!rendered.contains("Provider proxy"));
    assert!(rendered.contains("Deep gpt-5.4 proxy · Demo"));
    assert!(rendered.contains("ctrl+p commands"));
    assert!(!rendered.contains("Enter select"));
    assert!(rendered.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(rendered.contains("commands"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions: New session · Continue session · Replay session"));
}

pub(super) fn startup_home_screen_uses_minimal_compat_shell() {
    let app = app::AppState::new_startup(Vec::new(), None);

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("██╗  ██╗") || rendered.contains("Harness"));
    assert!(rendered.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("New session"));
    assert!(!rendered.contains("Continue session"));
    assert!(!rendered.contains("Replay session"));
}

pub(super) fn startup_composer_keeps_inset_input_then_metadata_row_order() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    for (width, height) in [(100, 30), (80, 24), (160, 48)] {
        let rendered = render_live_lines(&app, width, height);
        let lines = rendered.lines().collect::<Vec<_>>();
        let composer_input_row = find_line_containing(
            &lines,
            "Ask anything... \"What is the tech stack of this project?\"",
        )
        .unwrap_or_else(|| panic!("startup composer input row at {width}x{height}\n{rendered}"));
        let composer_first_row = composer_input_row.saturating_sub(1);
        let metadata_gap_start = composer_input_row.saturating_add(1);
        let metadata_row = find_line_containing(&lines, "Deep gpt-5.4 proxy · Demo")
            .unwrap_or_else(|| panic!("startup metadata row at {width}x{height}\n{rendered}"));
        let composer_cap_row = metadata_row.saturating_add(1);

        assert_eq!(
            composer_input_row,
            composer_first_row + 1,
            "startup composer should keep a blank inset row before input at {width}x{height}\n{rendered}"
        );
        assert_eq!(
            metadata_row,
            composer_input_row + 2,
            "startup metadata should keep one blank spacer after the lowered input at {width}x{height}\n{rendered}"
        );
        assert_eq!(
            composer_cap_row,
            metadata_row + 1,
            "startup composer should end with the cap row immediately after metadata at {width}x{height}\n{rendered}"
        );
        assert!(
            lines[composer_cap_row].contains('▀'),
            "startup metadata should sit directly above the shell box cap at {width}x{height}\n{rendered}"
        );
        assert!(
            !lines[composer_first_row].chars().any(char::is_alphanumeric),
            "startup inset row should stay visually blank at {width}x{height}\n{rendered}"
        );
        for row in metadata_gap_start..metadata_row {
            assert!(
                !lines[row].chars().any(char::is_alphanumeric),
                "startup metadata spacer row should stay visually blank at {width}x{height}\n{rendered}"
            );
        }
    }
}

pub(super) fn startup_composer_width_stays_capped_for_shell() {
    let app = app::AppState::new_startup(Vec::new(), None);

    for (width, height) in [(80, 24), (100, 30), (160, 48)] {
        let plan = FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, width, height));
        let dock = plan.dock.unwrap_or_abort();

        assert_eq!(
            dock.shell.width, 75,
            "startup composer should keep the shell width cap at {width}x{height}"
        );
        assert_eq!(dock.composer.width, 75);
    }
}

pub(super) fn dense_live_composer_uses_full_height_without_metadata_row() {
    let mut ready = app::AppState::new_live(None, false, None);
    ready.composer.prompt_buffer = "draft".to_string();
    ready.composer.prompt_cursor = ready.composer.prompt_buffer.chars().count();
    let rendered = render_live_lines(&ready, 60, 18);
    let lines = rendered.lines().collect::<Vec<_>>();
    let (composer_first_row, composer_input_row, composer_last_row) =
        live_shell_composer_input_span(&lines);

    assert!(lines[composer_input_row].contains("draft"));
    assert_eq!(composer_input_row, composer_first_row + 1);
    assert!(composer_last_row > composer_input_row);
    assert!(
        find_line_containing_in_range(
            &lines,
            composer_input_row + 1,
            composer_last_row + 1,
            "Current runtime:"
        )
        .is_none(),
        "dense live composer should remove the status row under the draft\n{rendered}"
    );
}

pub(super) fn dense_live_compaction_feedback_uses_toast_when_metadata_row_is_absent() {
    let mut ready = app::AppState::new_live(None, false, None);
    ready.composer.prompt_buffer = "draft".to_string();
    ready.composer.prompt_cursor = ready.composer.prompt_buffer.chars().count();
    ready.set_toast_for_test(
        "manual compaction skipped: need at least two completed turns",
        app::ToastVariant::Info,
    );

    let rendered = render_live_lines(&ready, 60, 18);
    let lines = rendered.lines().collect::<Vec<_>>();
    let (_, composer_input_row, composer_last_row) = live_shell_composer_input_span(&lines);

    assert!(
        find_line_containing_in_range(
            &lines,
            composer_input_row + 1,
            composer_last_row + 1,
            "Current runtime:"
        )
        .is_none(),
        "dense live composer should still omit the metadata row\n{rendered}"
    );
    assert!(
        rendered.contains("manual compaction skipped"),
        "manual compaction feedback should stay visible via toast when the footer metadata row is absent\n{rendered}"
    );
}

pub(super) fn live_composer_disclosure_keeps_compact_summary_and_commands() {
    let mut ready = app::AppState::new_live(None, false, None);
    let mut events = session_view_events();
    events.pop();
    for event in events {
        ready.ingest_event(event);
    }
    let rendered = render_live_lines(&ready, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let disclosure_row = find_line_containing(&lines, "Ctrl+p commands")
        .unwrap_or_else(|| panic!("live composer disclosure row\n{rendered}"));

    assert!(lines[disclosure_row].contains("Ctrl+p commands"));
    assert!(!lines[disclosure_row].contains("Enter send"));
    assert!(!lines[disclosure_row].contains("tool finished"));
    assert!(!lines[disclosure_row].contains("turn 1"));
    assert!(!lines[disclosure_row].contains("ready for next turn"));
    assert!(!rendered.contains("Current runtime:"));
}
