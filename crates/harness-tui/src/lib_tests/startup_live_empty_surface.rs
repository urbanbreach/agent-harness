use super::*;
use crate::UnwrapOrAbort;

pub(super) fn startup_shell_shows_profile_provider_and_model_chrome() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("██╗  ██╗"));
    assert!(!rendered.contains("Launch: deep · gpt-5.4"));
    assert!(!rendered.contains("Provider proxy"));
    assert!(rendered.contains("Deep gpt-5.4 proxy · Demo"));
    assert!(rendered.contains("ctrl+p commands"));
    assert!(!rendered.contains("Enter select"));
    assert!(rendered.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(rendered.contains("commands"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions:"));
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
    let title_row = find_line_containing(&lines, "██╗  ██╗").unwrap_or_abort();
    let prompt_row = find_line_containing(
        &lines,
        "Ask anything... \"What is the tech stack of this project?\"",
    )
    .unwrap_or_abort();
    let footer_row = find_line_containing(&lines, "commands").unwrap_or_abort();

    assert!(!rendered.contains("Actions:"));
    assert!(!rendered.contains("Dispatch a new run"));
    assert!(!rendered.contains("Launch: worker · model-1"));
    assert!(rendered.contains("commands"));
    assert!(title_row < prompt_row);
    assert!(prompt_row < footer_row);
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
    assert!(demo_rendered.contains("Harness"));
    assert!(demo_rendered.contains("Launch: worker · model-1"));
    assert!(demo_rendered.contains("Start a conversation to begin"));
    assert!(!demo_rendered.contains("Demo mode · mock provider"));
    assert!(!demo_rendered.contains("Launch: worker · model-1 · Demo"));

    let mut mock = app::AppState::new_live(None, false, None);
    mock.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
    );

    let mock_rendered = render_live_lines(&mock, 100, 24);
    assert!(mock_rendered.contains("Harness"));
    assert!(mock_rendered.contains("Launch: worker · model-1"));
    assert!(mock_rendered.contains("Start a conversation to begin"));
    assert!(!mock_rendered.contains("Mock mode · mock provider"));
    assert!(!mock_rendered.contains("Launch: worker · model-1 · Mock"));
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
    assert!(rendered.contains("Session"));
    assert!(!rendered.contains('┌'));
    assert!(rendered.contains("Harness"));
    assert!(rendered.contains("Launch: default · -"));
    assert!(rendered.contains("Start a conversation to begin"));
    assert!(rendered.contains("0  Ctrl+p commands"));
    assert!(rendered.contains("Ctrl+p commands"));
    assert!(!rendered.contains("Enter send · Shift+Enter/Ctrl+j newline · ↑/↓ history"));
    assert!(!rendered.contains("Type to start a new session."));

    assert_live_shell_document_composer_contract(&app, 80, 24, None, None, "Ctrl+p commands");
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
    let status_row = live_status_strip_row(&app, 160, 30, "Ctrl+p commands");

    insta::assert_snapshot!(
        status_row,
        @"live 0  Ctrl+p commands"
    );
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
    let theme = Theme::default();
    let app = app::AppState::new_live(None, false, None);

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let title_row = find_line_containing(
        &lines,
        &theme.live_shell.empty_state.title.to_ascii_uppercase(),
    )
    .or_else(|| find_line_containing(&lines, theme.live_shell.empty_state.title))
    .unwrap_or_abort();
    let metadata_row = find_line_containing(&lines, "Launch: default · -").unwrap_or_abort();
    let value_prop_row =
        find_line_containing(&lines, theme.live_shell.empty_state.value_prop).unwrap_or_abort();
    let help_row = find_line_containing(&lines, "Ctrl+p commands").unwrap_or_abort();

    assert!(
        title_row > 0,
        "empty state title should not render flush against the header"
    );
    assert!(title_row <= metadata_row);
    assert!(metadata_row < value_prop_row);
    assert!(value_prop_row < help_row);
    assert!(title_row < value_prop_row);
    assert!(value_prop_row < help_row);
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

    for marker in [
        "██╗  ██╗",
        "Ask anything... \"What is the tech stack of this project?\"",
    ] {
        assert!(
            startup_render.contains(marker),
            "startup missing {marker}\n{startup_render}"
        );
    }
    for marker in ["Harness", "Launch: worker · model-1"] {
        assert!(
            live_render.contains(marker),
            "live empty missing {marker}\n{live_render}"
        );
    }

    assert!(!startup_render.contains("Dispatch a new run"));
    assert!(!startup_render.contains("Launch: worker · model-1"));
    assert!(live_render.contains("Start a conversation to begin"));
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
        startup_render.contains("Ask anything... \"What is the tech stack of this project?\""),
        "startup should keep the prompt accessible in the minimal shell\n{startup_render}"
    );
    assert!(live_render.contains("Session"));
    assert!(!live_render.contains('┌') && !live_render.contains('╭'));
    assert!(live_render.contains("Harness"));
    assert!(live_render.contains("Launch: worker · model-1"));
    assert_row_segment_background(
        &startup_buffer,
        100,
        "Worker model-1 mock",
        ratatui::style::Color::Rgb(0x1E, 0x1E, 0x1E),
    );
    assert_row_segment_background(
        &live_buffer,
        100,
        "Worker model-1 mock",
        ratatui::style::Color::Rgb(0x1E, 0x1E, 0x1E),
    );
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
    let startup_title = find_line_containing(&startup_lines, "██╗  ██╗").unwrap_or_abort();
    let startup_prompt = find_line_containing(
        &startup_lines,
        "Ask anything... \"What is the tech stack of this project?\"",
    )
    .unwrap_or_abort();
    let startup_keys = find_line_containing(&startup_lines, "commands").unwrap_or_abort();

    let live_render = render_live_lines(&live, 100, 24);
    let live_lines = live_render.lines().collect::<Vec<_>>();
    let live_metadata =
        find_line_containing(&live_lines, "Launch: worker · model-1").unwrap_or_abort();
    let live_value =
        find_line_containing(&live_lines, "Start a conversation to begin").unwrap_or_abort();
    let live_keys = find_line_containing(&live_lines, "Ctrl+p commands").unwrap_or_abort();

    assert!(startup_title < startup_prompt);
    assert!(startup_prompt < startup_keys);

    assert!(live_metadata < live_value);
    assert!(live_value < live_keys);
}
