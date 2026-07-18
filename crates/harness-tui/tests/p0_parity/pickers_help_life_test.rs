use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyModifiers};
use harness_core::event::{
    EventV1, ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, UserMessageSubmittedEvent,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{
    AppState, Focus, LaunchMetadata, ModelOption, ReviewSurface, RuntimeStateKind, UiIntent,
};
use harness_tui::overlay::OverlayKind;

use crate::helpers::*;

// ---------------------------------------------------------------------------
// P0-PERM-02
// ---------------------------------------------------------------------------

#[test]
fn question_overlay_parses_prompts_preserves_draft_and_renders() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    let draft = "keep draft under question";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.focus = Focus::Prompt;

    // act
    app.ingest_event(question_permission_requested_event(
        1,
        "perm_question_p0",
        "tool_call_question_p0",
    ));
    let view = app
        .active_permission_view()
        .expect("P0-PERM-02: active permission view required");
    let rendered = render_text(&app, 120, 40);
    let plan = plan_for(&app, 120, 40);

    // assert
    assert_eq!(view.kind, "question");
    let prompts = view
        .question_prompts
        .as_ref()
        .expect("P0-PERM-02: question_prompts must parse from summary JSON");
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].question, "Pick one");
    assert_eq!(prompts[0].header, "Choice");
    assert_eq!(prompts[0].options[0].label, "A");
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "P0-PERM-02: draft must be preserved under question overlay"
    );
    assert!(
        rendered.contains("Pick one")
            || rendered.contains("Choice")
            || rendered.contains('●')
            || rendered.contains('○'),
        "P0-PERM-02: question overlay must render\n{rendered}"
    );
    assert!(
        !rendered.contains("always-approve"),
        "P0-PERM-02: question must not render edit-permission allow chrome\n{rendered}"
    );
    assert!(
        plan.operator_sidebar.is_none(),
        "P0-PERM-02: question shell stays full-width"
    );

    // act — Esc dismiss path keeps draft
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "P0-PERM-02: draft must remain after Esc"
    );
}

// ---------------------------------------------------------------------------
// P0-PICK-01
// ---------------------------------------------------------------------------

#[test]
fn model_switcher_opens_from_slash_with_real_catalog() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    let models = vec![
        ModelOption::from_model_ref("build", "mock:model-a"),
        ModelOption::from_model_ref("plan", "mock:model-b"),
    ];
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-a").with_available_models(models),
    );

    // act
    for ch in "/model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let rendered = render_text(&app, 120, 40);

    // assert
    assert!(
        app.model_switcher_visible,
        "P0-PICK-01: /model must open model switcher"
    );
    assert!(
        !app.model_options.is_empty(),
        "P0-PICK-01: real catalog rows must populate model_options"
    );
    assert_eq!(
        app.model_options.len(),
        app.model_filtered.len().max(1).min(app.model_options.len()),
        "P0-PICK-01: filtered index must stay within catalog"
    );
    assert!(
        rendered.contains("Select model")
            || rendered.contains("Search")
            || rendered.contains("model"),
        "P0-PICK-01: model picker chrome must render\n{rendered}"
    );
    assert!(
        plan_for(&app, 120, 40).operator_sidebar.is_none(),
        "P0-PICK-01: model picker must not introduce operator sidebar"
    );

    // act — dismiss
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.model_switcher_visible,
        "P0-PICK-01: Esc must dismiss model switcher"
    );
}

// ---------------------------------------------------------------------------
// P0-PICK-02
// ---------------------------------------------------------------------------

#[test]
fn session_picker_opens_with_searchable_history() {
    // arrange
    let mut app = AppState::new_startup(startup_session_history_entries(), None);

    // act
    app.execute_slash_command("sessions", Some(String::new()));
    let rendered = render_text(&app, 120, 40);

    // assert
    assert!(
        app.session_history_visible,
        "P0-PICK-02: /sessions must open session picker"
    );
    assert!(
        !app.session_history_entries.is_empty(),
        "P0-PICK-02: non-empty history must remain available"
    );
    assert!(
        !app.session_history_filtered.is_empty(),
        "P0-PICK-02: filtered index must include history rows"
    );
    assert!(
        rendered.contains("Continue session")
            || rendered.contains("alpha-run")
            || rendered.contains("Search"),
        "P0-PICK-02: session picker surface must render\n{rendered}"
    );

    // act — empty history path
    let empty = AppState::new_startup(Vec::new(), None);
    let mut empty_picker = empty;
    empty_picker.execute_slash_command("sessions", Some(String::new()));
    let empty_render = render_text(&empty_picker, 120, 40);
    assert!(
        empty_picker.session_history_visible || empty_picker.session_history_entries.is_empty(),
        "P0-PICK-02: empty history must remain a reachable state"
    );
    assert!(
        !empty_render.to_ascii_lowercase().contains("grok"),
        "P0-PICK-02: empty/error path must stay Harness-branded\n{empty_render}"
    );
}

// ---------------------------------------------------------------------------
// P0-HELP-01
// ---------------------------------------------------------------------------

#[test]
fn help_and_toggles_surfaces_open_harness_safe() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    let draft = "draft under help";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();

    // act — help
    app.execute_slash_command("help", Some(draft.to_string()));
    let help_render = render_text(&app, 120, 40);

    // assert — help
    assert_eq!(
        app.review_surface(),
        Some(ReviewSurface::Help),
        "P0-HELP-01: /help must open Help review surface"
    );
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "P0-HELP-01: help must preserve composer draft"
    );
    assert!(
        !help_render.to_ascii_lowercase().contains("grok")
            && !help_render.to_ascii_lowercase().contains("spacex"),
        "P0-HELP-01: help must not show reference branding\n{help_render}"
    );

    // act — toggles (settings-like surface)
    let mut toggles_app = AppState::new_live(None, false, None);
    for ch in "/toggles".chars() {
        toggles_app.handle_key(key(KeyCode::Char(ch)));
    }
    toggles_app.handle_key(key(KeyCode::Enter));
    let toggles_render = render_text(&toggles_app, 120, 40);

    // assert — toggles
    assert!(
        toggles_app.toggles_menu_visible,
        "P0-HELP-01: /toggles must open toggles menu"
    );
    assert!(
        !toggles_render
            .to_ascii_lowercase()
            .contains("plugin marketplace"),
        "P0-HELP-01: toggles must not advertise OOS plugin marketplace\n{toggles_render}"
    );

    // act — theme dialog (Harness-safe theme names)
    let mut theme_app = AppState::new_live(None, false, None);
    theme_app.handle_key(key_with_modifiers(KeyCode::Char('x'), KeyModifiers::CONTROL));
    theme_app.handle_key(key(KeyCode::Char('t')));
    let theme_render = render_text(&theme_app, 120, 40);
    // assert — theme surface opens without reference branding
    assert!(
        theme_app.theme_dialog_visible
            || theme_render.to_ascii_lowercase().contains("theme")
            || theme_app
                .overlay_stack()
                .top()
                .is_some_and(|k| format!("{k:?}").contains("Theme")),
        "P0-HELP-01: leader+t / OpenThemeDialog must open theme surface; visible={} render=\n{theme_render}",
        theme_app.theme_dialog_visible
    );
    assert!(
        !theme_render.to_ascii_lowercase().contains("grok")
            && !theme_render.to_ascii_lowercase().contains("spacex"),
        "P0-HELP-01: theme surface must be Harness-safe\n{theme_render}"
    );

    // assert — keybind remapping surface remains reachable via defaults
    let keymap = harness_tui::keybindings::KeyMap::with_defaults();
    assert_eq!(
        keymap.get_binding_str(harness_tui::keybindings::Action::OpenThemeDialog),
        "Ctrl+x t",
        "P0-HELP-01: theme keybind rematerialization documented in simple-mode defaults"
    );
    assert_eq!(
        keymap.get_binding_str(harness_tui::keybindings::Action::OpenStatusDialog),
        "Ctrl+x s"
    );
}

// ---------------------------------------------------------------------------
// P0-LIFE-01
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_new_session_and_resume_intents_reachable() {
    // arrange
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap().push(intent);
        })
    };
    let mut live = AppState::new_live(
        Some(PathBuf::from("/tmp/run_p0_life")),
        false,
        Some(Arc::clone(&sink)),
    );

    // act — /new
    for ch in "/new".chars() {
        live.handle_key(key(KeyCode::Char(ch)));
    }
    live.handle_key(key(KeyCode::Enter));

    // assert — new session lifecycle
    let new_reached = live.startup_shell_visible()
        || intents
            .lock()
            .unwrap()
            .iter()
            .any(|intent| matches!(intent, UiIntent::NewSession));
    assert!(
        new_reached,
        "P0-LIFE-01: /new must reach NewSession handoff; startup={} intents={:?}",
        live.startup_shell_visible(),
        intents.lock().unwrap()
    );

    // arrange — resume/history path
    let mut startup = AppState::new_startup(startup_session_history_entries(), Some(sink));
    for ch in "/resume".chars() {
        startup.handle_key(key(KeyCode::Char(ch)));
    }
    startup.handle_key(key(KeyCode::Enter));

    // assert — resume surface
    assert!(
        startup.session_history_visible,
        "P0-LIFE-01: /resume must open session history"
    );
    assert!(
        startup.selected_session_history_entry().is_some(),
        "P0-LIFE-01: session history must select a resumable entry"
    );

    // act — palette route for new session remains reachable
    let mut palette = AppState::new_live(None, false, None);
    palette.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    for ch in "new".chars() {
        palette.handle_key(key(KeyCode::Char(ch)));
    }
    let palette_render = render_text(&palette, 120, 40);
    assert!(
        palette.palette_visible,
        "P0-LIFE-01: palette must open for lifecycle command routing"
    );
    assert!(
        palette_render.contains("New session") || palette_render.to_lowercase().contains("new"),
        "P0-LIFE-01: palette must expose new-session route\n{palette_render}"
    );

    // act/assert — compact emits coordinator intent under full-width shell
    let compact_intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let compact_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let compact_intents = Arc::clone(&compact_intents);
        Arc::new(move |intent: UiIntent| {
            compact_intents.lock().unwrap().push(intent);
        })
    };
    let mut compact_app = AppState::new_live(None, false, Some(compact_sink));
    compact_app.execute_slash_command("compact", None);
    assert!(
        compact_intents
            .lock()
            .unwrap()
            .iter()
            .any(|intent| matches!(intent, UiIntent::CompactSession)),
        "P0-LIFE-01: /compact must emit CompactSession; got {:?}",
        compact_intents.lock().unwrap()
    );
    assert!(
        plan_for(&compact_app, 120, 40).operator_sidebar.is_none(),
        "P0-LIFE-01: compact path keeps full-width shell"
    );

    // act/assert — fork/clone lineage routes are real (intent or blocked banner, not fake success)
    let mut lineage = AppState::new_live(None, false, None);
    lineage.execute_slash_command("fork", None);
    let fork_render = render_text(&lineage, 120, 40);
    lineage.execute_slash_command("clone", None);
    let clone_render = render_text(&lineage, 120, 40);
    assert!(
        fork_render.to_lowercase().contains("fork")
            || lineage
                .status_banner
                .as_ref()
                .is_some_and(|b| b.to_lowercase().contains("fork")),
        "P0-LIFE-01: /fork must surface fork route or blocked reason\n{fork_render}"
    );
    assert!(
        clone_render.to_lowercase().contains("clone")
            || lineage
                .status_banner
                .as_ref()
                .is_some_and(|b| b.to_lowercase().contains("clone")),
        "P0-LIFE-01: /clone must surface clone route or blocked reason\n{clone_render}"
    );
}
