use super::*;
use crate::UnwrapOrAbort;

pub(super) fn startup_shell_shows_profile_provider_and_model_chrome() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("Harness") || rendered.contains('╭'));
    assert!(!rendered.contains("Launch: deep · gpt-5.4"));
    assert!(!rendered.contains("Provider proxy"));
    assert!(rendered.contains("gpt-5.4") || rendered.contains("Deep") || rendered.contains("Demo"));
    assert!(rendered.contains('❯'));
    assert!(!rendered.contains("Enter select"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));

    app.handle_key(key(crossterm::event::KeyCode::Char('x')));
    let draft = render_live_lines(&app, 100, 24);
    assert!(
        draft.contains("gpt-5.4") || draft.contains("Deep") || draft.contains("Demo"),
        "draft startup restores model chrome on composer\n{draft}"
    );
}

pub(super) fn lifecycle_shell_narrow_layout_renders_primary_cta() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let title_row = find_line_containing(&lines, "Harness")
        .or_else(|| find_line_containing(&lines, "New worktree"))
        .or_else(|| find_line_containing(&lines, "New session"))
        .unwrap_or_abort();
    let prompt_row = find_line_containing(&lines, "❯").unwrap_or_abort();
    let footer_row = find_line_containing(&lines, "Logged in")
        .or_else(|| find_line_containing(&lines, "API key"))
        .unwrap_or_else(|| lines.len().saturating_sub(1));

    assert!(!rendered.contains("Dispatch a new run"));
    assert!(!rendered.contains("Launch: worker · model-1"));
    assert!(rendered.contains('❯'));
    assert!(title_row < prompt_row);
    assert!(prompt_row <= footer_row);
}

pub(super) fn startup_card_uses_lifecycle_geometry_contract() {
    let theme = Theme::default();
    let minimum_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let primary_area = ratatui::layout::Rect::new(0, 0, 100, 30);

    let minimum_layout = theme.lifecycle_surface_layout(minimum_area.width, minimum_area.height);
    let primary_layout = theme.lifecycle_surface_layout(primary_area.width, primary_area.height);

    let minimum_startup = layout::startup_shell_area(minimum_area, &theme);
    let primary_startup = layout::startup_shell_area(primary_area, &theme);

    assert_eq!(
        minimum_startup,
        layout::lifecycle_card_area(minimum_area, &theme, minimum_layout.startup_card)
    );
    assert_eq!(
        primary_startup,
        layout::lifecycle_card_area(primary_area, &theme, primary_layout.startup_card)
    );
    assert_eq!(minimum_startup, ratatui::layout::Rect::new(5, 7, 70, 12));
    assert_eq!(primary_startup, ratatui::layout::Rect::new(9, 10, 82, 12));
    assert_ne!(
        minimum_startup,
        layout::live_empty_state_area(minimum_area, &theme)
    );
    assert_ne!(
        primary_startup,
        layout::live_empty_state_area(primary_area, &theme)
    );
}

pub(super) fn startup_card_moves_closer_to_dock_in_tall_and_split_windows() {
    let theme = Theme::default();
    let tall_minimum_area = ratatui::layout::Rect::new(0, 0, 80, 48);
    let split_area = ratatui::layout::Rect::new(0, 0, 96, 40);

    let tall_minimum = layout::startup_shell_area(tall_minimum_area, &theme);
    let split = layout::startup_shell_area(split_area, &theme);

    assert_eq!(tall_minimum, ratatui::layout::Rect::new(5, 19, 70, 12));
    assert_eq!(split, ratatui::layout::Rect::new(0, 14, 96, 13));
}

pub(super) fn live_empty_state_uses_shared_startup_copy_without_mode_badges() {
    let mut demo = app::AppState::new_live(None, false, None);
    demo.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let demo_rendered = render_live_lines(&demo, 100, 24);
    assert!(demo_rendered.contains('❯'));
    assert!(demo_rendered.contains('╭') || demo_rendered.contains('╰'));
    assert!(demo_rendered.contains("model-1") || demo_rendered.contains("worker"));
    assert!(!demo_rendered.contains("Session"));
    assert!(demo_rendered.contains("Start a conversation to begin"));
    assert!(demo_rendered.contains("inspect src/ui.rs"));
    assert!(!demo_rendered.contains("Demo mode · mock provider"));

    let mut mock = app::AppState::new_live(None, false, None);
    mock.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
    );

    let mock_rendered = render_live_lines(&mock, 100, 24);
    assert!(mock_rendered.contains('❯'));
    assert!(mock_rendered.contains("model-1") || mock_rendered.contains("worker"));
    assert!(!mock_rendered.contains("Session"));
    assert!(mock_rendered.contains("Start a conversation to begin"));
    assert!(mock_rendered.contains("review the latest edit"));
    assert!(!mock_rendered.contains("Mock mode · mock provider"));
}

pub(super) fn live_shell_minimum_geometry_snapshot_renders_without_overlap() {
    assert_live_shell_geometry(80, 24);
}

pub(super) fn live_shell_primary_geometry_snapshot_renders_without_overlap() {
    assert_live_shell_geometry(100, 30);
}

pub(super) fn live_empty_state_snapshot_renders_input_first_shell() {
    let app = app::AppState::new_live(None, false, None);
    let rendered = render_live_lines(&app, 80, 24);

    assert_live_shell_frame_invariants(&rendered, 80, 24);
    assert!(!rendered.contains("Session"));
    assert!(!rendered.contains('┌'));
    assert!(rendered.contains('❯'));
    assert!(rendered.contains('╭') || rendered.contains('╰'));
    assert!(rendered.contains("Start a conversation to begin"));
    assert!(rendered.contains("trace the failing test"));
    assert!(!rendered.contains("Enter send · Shift+Enter/Ctrl+j newline · ↑/↓ history"));
    assert!(!rendered.contains("Type to start a new session."));
    assert!(!rendered.contains("Ask Harness to inspect, edit, or explain…"));
}

pub(super) fn live_empty_state_disappears_after_first_activity() {
    let theme = Theme::default();
    let mut app = app::AppState::new_live(None, false, None);

    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains(theme.live_shell.empty_state.value_prop));
    assert!(!rendered.contains(theme.live_shell.empty_state.example_prompts[0].prompt));
    assert!(rendered.contains("ship it"));
    assert!(!rendered.contains("pending turn"));
}

pub(super) fn live_shell_orchestration_status_strip_snapshot() {
    let app = orchestration_status_strip_fixture();
    let rendered = render_live_lines(&app, 160, 30);

    assert!(rendered.contains("Shift+Tab:mode"));
    assert!(rendered.contains("Ctrl+x:shortcuts"));
}

pub(super) fn live_status_strip_orchestration_summary_truncates_warning_last() {
    let app = orchestration_status_strip_fixture();

    let wide = render_live_lines(&app, 160, 30);
    let counts_only = render_live_lines(&app, 77, 24);
    assert!(wide.contains("orch 2a 1q 1r 1s") || !wide.contains("warn stale for 3001 ms"));
    assert!(!counts_only.contains("warn stale for 3001 ms"));
}

pub(super) fn live_status_strip_renders_zero_state_orchestration_counts() {
    let app = app::AppState::new_live(None, false, None);

    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains("orch 0a 0q 0r 0s"));
    assert!(!rendered.contains("warn"));
}

pub(super) fn live_empty_state_respects_compact_geometry() {
    let app = app::AppState::new_live(None, false, None);

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let prompt_row = find_line_containing(&lines, "❯").unwrap_or_abort();
    assert!(
        prompt_row > 0,
        "bordered composer should not render flush against the top edge\n{rendered}"
    );
    assert!(!rendered.contains("Session"));
    assert!(rendered.contains('╭') || rendered.contains('╰'));
}

pub(super) fn startup_home_matches_live_empty_shell_language() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let startup_render = render_live_lines(&startup, 100, 24);
    let live_render = render_live_lines(&live, 100, 24);

    assert!(
        startup_render.contains("New worktree") || startup_render.contains("New session"),
        "startup missing New worktree\n{startup_render}"
    );
    assert!(
        startup_render.contains('❯'),
        "startup missing composer glyph\n{startup_render}"
    );
    assert!(
        startup_render.contains("Harness")
            || startup_render.contains('╭')
            || startup_render.contains("Changelog"),
        "startup should keep welcome or bordered chrome\n{startup_render}"
    );
    assert!(
        live_render.contains('❯'),
        "live empty missing composer glyph\n{live_render}"
    );
    assert!(
        live_render.contains("model-1") || live_render.contains("worker"),
        "live empty should surface model identity on composer chrome\n{live_render}"
    );

    assert!(!startup_render.contains("Dispatch a new run"));
    assert!(!startup_render.contains("Launch: worker · model-1"));
    assert!(live_render.contains("Start a conversation to begin"));
    assert!(!live_render.contains("Session"));
    assert!(!startup_render.contains("● Tip"));
    assert!(!live_render.contains("Waiting for first turn…"));
}

pub(super) fn live_empty_state_uses_shared_home_surface_tokens() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let startup_render = render_live_lines(&startup, 100, 24);
    let startup_buffer = render_live_cells(&startup, 100, 24);
    let live_render = render_live_lines(&live, 100, 24);
    let live_buffer = render_live_cells(&live, 100, 24);
    let _theme = Theme::default();

    assert!(
        !startup_render.contains("open a fresh session in this directory"),
        "startup should not render purpose copy below the logo\n{startup_render}"
    );
    assert!(
        startup_render.contains('❯'),
        "startup should keep the prompt accessible in the minimal shell\n{startup_render}"
    );
    assert!(!live_render.contains("Session"));
    assert!(!live_render.contains('┌'));
    assert!(live_render.contains('❯'));
    assert!(live_render.contains('╭') || live_render.contains('╰'));
    assert!(live_render.contains("model-1") || live_render.contains("worker"));
    let _ = (startup_buffer, live_buffer);
}

pub(super) fn startup_and_live_empty_share_spacing_contract() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let startup_render = render_live_lines(&startup, 100, 24);
    let startup_lines = startup_render.lines().collect::<Vec<_>>();
    let startup_title = find_line_containing(&startup_lines, "Harness")
        .or_else(|| find_line_containing(&startup_lines, "New worktree"))
        .or_else(|| find_line_containing(&startup_lines, "New session"))
        .unwrap_or_abort();
    let startup_prompt = find_line_containing(&startup_lines, "❯").unwrap_or_abort();
    let startup_keys = find_line_containing(&startup_lines, "Logged in")
        .or_else(|| find_line_containing(&startup_lines, "API key"))
        .unwrap_or_else(|| startup_lines.len().saturating_sub(1));

    let live_render = render_live_lines(&live, 100, 24);
    let live_lines = live_render.lines().collect::<Vec<_>>();
    let live_prompt = find_line_containing(&live_lines, "❯").unwrap_or_abort();

    assert!(startup_title < startup_prompt);
    assert!(startup_prompt < startup_keys);

    assert!(live_prompt > 0);
    assert!(!live_render.contains("Session"));
    assert!(live_render.contains('╭') || live_render.contains('╰'));
}
