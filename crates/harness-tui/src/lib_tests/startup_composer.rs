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
    assert!(
        rendered.contains('╭') && rendered.contains('╰'),
        "startup shell uses rounded welcome/composer borders\n{rendered}"
    );
    assert!(
        rendered.contains("New session") || rendered.contains("New worktree"),
        "welcome action rows present\n{rendered}"
    );
    assert!(rendered.contains('❯'), "composer glyph present\n{rendered}");
    assert!(
        rendered.contains("gpt-5.4") && rendered.contains("Demo"),
        "compose-first startup keeps runtime metadata visible\n{rendered}"
    );
    assert!(!rendered.contains("Launch: deep · gpt-5.4"));
    assert!(!rendered.contains("Provider proxy"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions: New session · Continue session · Replay session"));

    app.handle_key(key(crossterm::event::KeyCode::Char('x')));
    let draft = render_live_lines(&app, 160, 48);
    assert!(
        draft.contains("gpt-5.4") || draft.contains("Deep") || draft.contains("Demo"),
        "draft startup keeps model chrome on composer\n{draft}"
    );
}

pub(super) fn startup_home_screen_uses_minimal_compat_shell() {
    let app = app::AppState::new_startup(Vec::new(), None);

    let rendered = render_live_lines(&app, 100, 24);
    assert!(
        rendered.contains('╭') || rendered.contains("Harness") || rendered.contains('❯'),
        "compact startup still paints shell chrome\n{rendered}"
    );
    assert!(rendered.contains('❯'));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
}

pub(super) fn startup_composer_keeps_inset_input_then_metadata_row_order() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    for (width, height) in [(100, 30), (80, 24), (160, 48)] {
        let rendered = render_live_lines(&app, width, height);
        let lines = rendered.lines().collect::<Vec<_>>();
        let glyph_row = find_line_containing(&lines, "❯")
            .unwrap_or_else(|| panic!("startup composer glyph at {width}x{height}\n{rendered}"));
        let top = lines[..glyph_row]
            .iter()
            .rposition(|line| line.contains('╭'))
            .unwrap_or_else(|| panic!("composer top border at {width}x{height}\n{rendered}"));
        let bottom = lines[glyph_row + 1..]
            .iter()
            .position(|line| line.contains('╰'))
            .map(|i| glyph_row + 1 + i)
            .unwrap_or_else(|| panic!("composer bottom border at {width}x{height}\n{rendered}"));
        assert_eq!(
            bottom - top,
            2,
            "bordered single-line composer is 3 rows at {width}x{height}\n{rendered}"
        );
    }
}

pub(super) fn startup_composer_width_stays_capped_for_shell() {
    let app = app::AppState::new_startup(Vec::new(), None);

    for (width, height) in [(80, 24), (100, 30), (160, 48)] {
        let plan = FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, width, height));
        let dock = plan.dock.unwrap_or_abort();

        assert!(
            dock.shell.width + 4 >= plan.shell.width.min(width),
            "startup composer should be near full shell width (inset ~2) at {width}x{height}: dock={} shell={}",
            dock.shell.width,
            plan.shell.width
        );
        assert!(
            dock.shell.x >= plan.shell.x,
            "startup composer stays within shell at {width}x{height}"
        );
        assert_eq!(dock.composer.width, dock.shell.width);
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
    assert!(
        composer_input_row == composer_first_row || composer_input_row == composer_first_row + 1,
        "draft should sit on the glyph row or immediately under the top border\n{rendered}"
    );
    assert!(composer_last_row >= composer_input_row);
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
    assert!(
        rendered.contains('╭') || rendered.contains('╰'),
        "dense live composer should keep bordered chrome\n{rendered}"
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
    assert!(!rendered.contains("Shift+Tab:mode"));
    assert!(!rendered.contains("Ctrl+x:shortcuts"));
    assert!(!rendered.contains("live ctx"));
    assert!(!rendered.contains("Enter send"));
    assert!(!rendered.contains("tool finished"));
    assert!(!rendered.contains("turn 1"));
    assert!(!rendered.contains("ready for next turn"));
    assert!(!rendered.contains("Current runtime:"));
}
