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
// P0-GEOM-01
// ---------------------------------------------------------------------------

#[test]
fn canonical_viewports_allocate_full_width_transcript_and_bottom_composer() {
    // arrange
    let app = live_session_app();

    for (width, height) in CANONICAL_VIEWPORTS {
        // act
        let plan = plan_for(&app, width, height);

        // assert
        assert!(
            plan.operator_sidebar.is_none(),
            "P0-GEOM-01: operator_sidebar must stay None at {width}x{height}; got {:?}",
            plan.operator_sidebar
        );

        let transcript = plan.transcript.unwrap_or_else(|| {
            panic!("P0-GEOM-01: transcript surface required at {width}x{height}")
        });
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("P0-GEOM-01: composer rect required at {width}x{height}"));

        assert_eq!(
            transcript.width, plan.shell.width,
            "P0-GEOM-01: transcript must span full shell width at {width}x{height}"
        );
        assert_eq!(
            composer.width, plan.shell.width,
            "P0-GEOM-01: composer must span full shell width at {width}x{height}"
        );
        assert!(
            transcript.y + transcript.height <= composer.y,
            "P0-GEOM-01: transcript must sit above composer at {width}x{height}; transcript={transcript:?} composer={composer:?}"
        );

        let dock_bottom = match plan.disclosure {
            Some(disclosure) => disclosure.y + disclosure.height,
            None => composer.y + composer.height,
        };
        assert_eq!(
            dock_bottom,
            plan.shell.y + plan.shell.height,
            "P0-GEOM-01: composer dock must be bottom-anchored at {width}x{height}"
        );
    }
}


// ---------------------------------------------------------------------------
// P0-START-01
// ---------------------------------------------------------------------------

#[test]
fn startup_welcome_is_compose_first_with_harness_branding_no_reference_identity() {
    // arrange
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    // act
    let rendered = render_text(&app, 120, 40);
    let plan = plan_for(&app, 120, 40);

    // assert
    let has_logo = rendered.contains("██╗  ██╗") || rendered.contains("Harness");
    assert!(
        has_logo,
        "P0-START-01: startup must show Harness branding (logo or title)\n{rendered}"
    );
    assert!(
        !rendered.to_ascii_lowercase().contains("grok"),
        "P0-START-01: must not render Grok reference identity\n{rendered}"
    );
    assert!(
        !rendered.to_ascii_lowercase().contains("spacexai")
            && !rendered.to_ascii_lowercase().contains("spacex"),
        "P0-START-01: must not render SpaceX/SpaceXAI reference identity\n{rendered}"
    );
    assert!(
        plan.composer.is_some(),
        "P0-START-01: compose-first home must allocate a bottom composer"
    );
    assert!(
        plan.operator_sidebar.is_none(),
        "P0-START-01: startup home must not reserve a persistent operator sidebar"
    );
    assert!(
        rendered.contains("Ask anything")
            || rendered.contains("ctrl+p commands")
            || rendered.contains("Ctrl+p")
            || rendered.contains("Type to start"),
        "P0-START-01: composer entry surface must be present\n{rendered}"
    );
}


// ---------------------------------------------------------------------------
// P0-START-02
// ---------------------------------------------------------------------------

#[test]
fn startup_first_run_shows_onboarding_hint_returning_user_does_not() {
    // arrange
    let first_run = AppState::new_startup(Vec::new(), None);
    let returning = AppState::new_startup(startup_session_history_entries(), None);

    // act
    let first_run_render = render_text(&first_run, 120, 40);
    let returning_render = render_text(&returning, 120, 40);

    // assert
    assert!(
        first_run_render.contains("First run?")
            || first_run_render.contains("harness doctor")
            || first_run_render.contains("harness auth login"),
        "P0-START-02: first-run must show onboarding hint\n{first_run_render}"
    );
    assert!(
        !returning_render.contains("First run?"),
        "P0-START-02: returning user must not show first-run onboarding hint\n{returning_render}"
    );

    // Returning users keep compose-first home but gain a session-history path.
    let mut returning_picker = AppState::new_startup(startup_session_history_entries(), None);
    returning_picker.execute_slash_command("sessions", Some(String::new()));
    let picker_render = render_text(&returning_picker, 120, 40);
    assert!(
        picker_render.contains("Continue session")
            || picker_render.contains("alpha-run")
            || picker_render.contains("Search"),
        "P0-START-02: returning user must reach session history path\n{picker_render}"
    );
    assert!(
        !picker_render.contains("First run?"),
        "P0-START-02: session history path must not show first-run onboarding\n{picker_render}"
    );
}


// ---------------------------------------------------------------------------
// P0-START-03
// ---------------------------------------------------------------------------

#[test]
fn startup_submit_emits_submit_prompt_and_leaves_startup_shell() {
    // arrange
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap().push(intent);
        })
    };
    let mut app = AppState::new_startup(Vec::new(), Some(sink));

    // act
    for ch in "ship the contract".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    // assert
    assert!(
        app.should_quit || !app.startup_shell_visible(),
        "P0-START-03: submit must leave the startup shell"
    );
    assert!(
        app.composer.prompt_buffer.is_empty(),
        "P0-START-03: composer must clear after startup submit"
    );

    let captured = intents.lock().unwrap();
    let mutates_session = captured.iter().any(|intent| {
        matches!(
            intent,
            UiIntent::NewSession | UiIntent::SubmitPrompt { .. } | UiIntent::ContinueSession { .. }
        )
    });
    assert!(
        mutates_session,
        "P0-START-03: submit must emit NewSession/SubmitPrompt/ContinueSession; got {captured:?}"
    );
    assert_eq!(
        captured.len(),
        1,
        "P0-START-03: exactly one mutating intent at the commit boundary; got {captured:?}"
    );
}


// ---------------------------------------------------------------------------
// P0-COMP-02
// ---------------------------------------------------------------------------

#[test]
fn permission_preempts_composer_and_preserves_draft_under_full_width_shell() {
    // arrange
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    let draft = "keep this draft under permission";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.focus = harness_tui::app::Focus::Prompt;

    // act
    app.ingest_event(permission_requested_event(
        1,
        "perm_comp_draft",
        "tool_call_comp_draft",
    ));
    let plan = plan_for(&app, 120, 40);
    let rendered = render_text(&app, 120, 40);
    app.handle_key(key(KeyCode::Enter));

    // assert
    assert!(
        plan.operator_sidebar.is_none(),
        "P0-COMP-02: permission shell must stay full-width (no operator sidebar)"
    );
    if let (Some(transcript), Some(composer)) = (plan.transcript, plan.composer) {
        assert_eq!(
            transcript.width, plan.shell.width,
            "P0-COMP-02: transcript remains full width under permission"
        );
        assert_eq!(
            composer.width, plan.shell.width,
            "P0-COMP-02: composer remains full width under permission"
        );
    }
    assert!(
        rendered.contains("Permission required"),
        "P0-COMP-02: permission overlay must render\n{rendered}"
    );
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "P0-COMP-02: draft must be preserved under permission overlay"
    );

    let captured = intents.lock().unwrap();
    assert!(
        captured.iter().any(|intent| matches!(
            intent,
            UiIntent::ResolvePermission {
                permission_id,
                decision: PermissionDecision::Allow,
                ..
            } if permission_id == "perm_comp_draft"
        )),
        "P0-COMP-02: Enter must resolve permission, not submit draft; got {captured:?}"
    );
    assert!(
        !captured
            .iter()
            .any(|intent| matches!(intent, UiIntent::SubmitPrompt { .. })),
        "P0-COMP-02: Enter must not submit the preserved draft; got {captured:?}"
    );
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "P0-COMP-02: draft must still be preserved after permission Enter"
    );
}


// ---------------------------------------------------------------------------
// P0-PAL-01
// ---------------------------------------------------------------------------

#[test]
fn command_palette_opens_filters_and_dismisses_without_sidebar() {
    // arrange
    let mut app = AppState::new_live(None, false, None);

    // act — open
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    let open_plan = plan_for(&app, 120, 40);
    let open_render = render_text(&app, 120, 40);

    // assert — open
    assert!(app.palette_visible, "P0-PAL-01: Ctrl+P must open palette");
    assert!(
        open_plan.operator_sidebar.is_none(),
        "P0-PAL-01: palette open must not introduce operator sidebar"
    );
    assert!(
        open_render.contains("Commands"),
        "P0-PAL-01: palette must render Commands header\n{open_render}"
    );

    // act — filter
    for ch in "new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let filtered_render = render_text(&app, 120, 40);

    // assert — filter
    assert!(
        app.palette_visible,
        "P0-PAL-01: typing must keep palette open while filtering"
    );
    assert!(
        filtered_render.contains("New session") || filtered_render.to_lowercase().contains("new"),
        "P0-PAL-01: filter 'new' must keep matching commands\n{filtered_render}"
    );
    assert!(
        plan_for(&app, 120, 40).operator_sidebar.is_none(),
        "P0-PAL-01: filter state must keep operator_sidebar None"
    );

    // act — dismiss
    app.handle_key(key(KeyCode::Esc));
    let closed_plan = plan_for(&app, 120, 40);

    // assert — dismiss
    assert!(
        !app.palette_visible,
        "P0-PAL-01: Esc must dismiss the command palette"
    );
    assert!(
        closed_plan.operator_sidebar.is_none(),
        "P0-PAL-01: dismiss must leave operator_sidebar None"
    );
}


// ---------------------------------------------------------------------------
// P0-PERM-01
// ---------------------------------------------------------------------------

#[test]
fn permission_overlay_preempts_palette_and_slash() {
    // arrange
    let mut palette_app = AppState::new_live(None, false, None);
    palette_app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    palette_app.handle_key(key(KeyCode::Char('d')));
    assert!(
        palette_app.palette_visible,
        "P0-PERM-01 pre: palette must be open before permission"
    );

    // act
    palette_app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_palette",
        "tool_call_preempt_palette",
    ));
    palette_app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    let palette_plan = plan_for(&palette_app, 120, 40);
    let palette_render = render_text(&palette_app, 120, 40);

    // assert
    assert!(
        !palette_app.palette_visible,
        "P0-PERM-01: permission must close/preempt palette"
    );
    assert!(
        palette_render.contains("Permission required"),
        "P0-PERM-01: permission overlay must render over palette path\n{palette_render}"
    );
    assert!(
        !palette_render.contains("Commands"),
        "P0-PERM-01: palette chrome must not remain visible under permission\n{palette_render}"
    );
    assert!(
        palette_plan.operator_sidebar.is_none(),
        "P0-PERM-01: full-width shell — no operator sidebar under permission"
    );
    if let Some(transcript) = palette_plan.transcript {
        assert_eq!(
            transcript.width, palette_plan.shell.width,
            "P0-PERM-01: transcript stays full width under permission"
        );
    }

    // arrange
    let mut slash_app = AppState::new_live(None, false, None);
    slash_app.handle_key(key(KeyCode::Char('/')));
    assert!(
        slash_app.slash_visible,
        "P0-PERM-01 pre: slash menu must be open before permission"
    );

    // act
    slash_app.ingest_event(permission_requested_event(
        2,
        "perm_preempt_slash",
        "tool_call_preempt_slash",
    ));
    slash_app.handle_key(key(KeyCode::Char('/')));
    let slash_plan = plan_for(&slash_app, 120, 40);
    let slash_render = render_text(&slash_app, 120, 40);

    // assert
    assert!(
        !slash_app.slash_visible,
        "P0-PERM-01: permission must close/preempt slash menu"
    );
    assert_eq!(
        slash_app.composer.prompt_buffer, "/",
        "P0-PERM-01: slash draft character must remain in composer"
    );
    assert!(
        slash_render.contains("Permission required"),
        "P0-PERM-01: permission overlay must render over slash path\n{slash_render}"
    );
    assert!(
        !slash_render.contains("Slash commands"),
        "P0-PERM-01: slash chrome must not remain visible under permission\n{slash_render}"
    );
    assert!(
        slash_plan.operator_sidebar.is_none(),
        "P0-PERM-01: full-width shell — no operator sidebar on slash path"
    );
}

