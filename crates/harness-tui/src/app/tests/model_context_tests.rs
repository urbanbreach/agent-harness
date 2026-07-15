use super::*;
use crate::view_model;
use crate::UnwrapOrAbort;

fn runtime_context_model_option(
    profile: &str,
    provider: &str,
    model: &str,
    variant: Option<&str>,
    display_label: &str,
) -> ModelOption {
    ModelOption {
        profile: profile.to_string(),
        provider: provider.to_string(),
        provider_display_label: None,
        provider_backend_label: None,
        model: model.to_string(),
        model_display_label: None,
        variant: variant.map(str::to_string),
        variant_display_label: None,
        display_label: Some(display_label.to_string()),
        token_window_label: None,
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: None,
        text_verbosity: None,
        thinking: None,
        recommended_for: None,
    }
}

fn metadata_model_option(
    profile: &str,
    profile_description: Option<&str>,
    provider: &str,
    provider_display_label: Option<&str>,
    model: &str,
    display_label: &str,
) -> ModelOption {
    ModelOption {
        profile: profile.to_string(),
        provider: provider.to_string(),
        provider_display_label: provider_display_label.map(str::to_string),
        provider_backend_label: Some("OpenAI".to_string()),
        model: model.to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant: Some("high".to_string()),
        variant_display_label: Some("High".to_string()),
        display_label: Some(display_label.to_string()),
        token_window_label: None,
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: profile_description.map(str::to_string),
        reasoning_effort: Some("high".to_string()),
        text_verbosity: None,
        thinking: None,
        recommended_for: None,
    }
}

pub(super) fn runtime_context_labels_distinguish_live_continue_and_replay() {
    let launch_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("deterministic"),
        "GPT-5.4 Mini · Deterministic",
    );

    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(LaunchMetadata::from_model_option(&launch_option));
    let startup_dock = startup.control_dock_view_model();
    assert_eq!(
        startup_dock.primary_summary,
        "Launch: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(startup_dock.summary_segment, None);
    assert_eq!(startup_dock.runtime_context.as_deref(), Some("default"));

    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(LaunchMetadata::from_model_option(&launch_option));
    let live_dock = live.control_dock_view_model();
    assert_eq!(
        live_dock.primary_summary,
        "Context: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(live_dock.summary_segment, None);
    assert_eq!(live_dock.runtime_context.as_deref(), Some("default"));

    let mut continued = AppState::new_live(None, false, None);
    continued.set_launch_metadata(
        LaunchMetadata::from_model_option(&launch_option).with_mode_label("Continued"),
    );
    let continued_dock = continued.control_dock_view_model();
    assert_eq!(
        continued_dock.primary_summary,
        "Context: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(continued_dock.summary_segment, None);
    assert_eq!(continued_dock.runtime_context.as_deref(), Some("default"));

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/runtime-context-replay"), Vec::new());
    replay.set_launch_metadata(LaunchMetadata::from_model_option(&launch_option));
    let replay_dock = replay.control_dock_view_model();
    assert_eq!(
        replay_dock.primary_summary,
        "Recorded runtime · read-only: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay_dock.summary_segment, None);
    assert_eq!(replay_dock.runtime_context.as_deref(), Some("default"));
    assert!(replay_dock.composer_disabled);
}

pub(super) fn composer_metadata_prefers_short_agent_name_and_configured_source_label() {
    let option = metadata_model_option(
        "build",
        Some("Deep Agent"),
        "default",
        Some("CLIProxyAPI"),
        "gpt-5.4-mini",
        "GPT-5.4 Mini · High",
    );

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_option(&option));

    assert_eq!(app.current_agent_label().as_deref(), Some("Build"));
    assert_eq!(app.current_source_label().as_deref(), Some("CLIProxyAPI"));
}

pub(super) fn composer_metadata_source_label_uses_provider_display_label_only() {
    let openai_option = metadata_model_option(
        "build",
        Some("Deep Agent"),
        "openai",
        Some("OpenAI"),
        "gpt-5.4-mini",
        "GPT-5.4 Mini · High",
    );
    let configured_suffix_option = metadata_model_option(
        "build",
        Some("Deep Agent"),
        "default",
        Some("CLIProxyAPI (OpenAI)"),
        "gpt-5.4-mini",
        "GPT-5.4 Mini · High",
    );

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_option(&openai_option));
    assert_eq!(app.current_source_label().as_deref(), Some("OpenAI"));

    app.set_launch_metadata(LaunchMetadata::from_model_option(&configured_suffix_option));
    assert_eq!(
        app.current_source_label().as_deref(),
        Some("CLIProxyAPI (OpenAI)")
    );
}

pub(super) fn live_switch_model_labels_next_turn_only() {
    let launch_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("deterministic"),
        "GPT-5.4 Mini · Deterministic",
    );
    let next_turn_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("creative"),
        "GPT-5.4 Mini · Creative",
    );

    let mut live = AppState::new_live(None, false, None);
    live.apply_keybindings(default_navigation_keybindings());
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&launch_option)
            .with_available_models(vec![launch_option.clone(), next_turn_option.clone()]),
    );

    live.handle_key(key(KeyCode::Tab));

    let dock = live.control_dock_view_model();
    assert_eq!(
        dock.primary_summary,
        "Context: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(
        dock.summary_segment,
        Some(view_model::ControlDockSummarySegment {
            kind: view_model::ControlDockSummarySegmentKind::Orchestration,
            text: "Next turns: deep · GPT-5.4 Mini".to_string(),
            tone: view_model::ControlDockSummaryTone::Secondary,
        })
    );

    let mut replay = AppState::new_replay(
        PathBuf::from("/tmp/runtime-context-replay-switch"),
        Vec::new(),
    );
    replay.apply_keybindings(default_navigation_keybindings());
    replay.set_launch_metadata(
        LaunchMetadata::from_model_option(&launch_option)
            .with_available_models(vec![launch_option, next_turn_option]),
    );

    replay.handle_key(key(KeyCode::Tab));

    let replay_dock = replay.control_dock_view_model();
    assert_eq!(
        replay_dock.primary_summary,
        "Recorded runtime · read-only: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay_dock.summary_segment, None);
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
    assert_eq!(replay.active_profile(), "deep");
}

pub(super) fn tab_cycles_build_and_plan_primary_agents() {
    let build_option =
        runtime_context_model_option("build", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");
    let plan_option =
        runtime_context_model_option("plan", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, None);
    app.on_ui_intent = Some(sink);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&build_option)
            .with_available_models(vec![build_option.clone(), plan_option])
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()]),
    );

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.active_profile(), "plan");
    assert_eq!(app.current_agent_label().as_deref(), Some("Plan"));
    {
        let intents = intents.lock().unwrap_or_abort();
        let [UiIntent::SwitchModel {
            profile,
            launch_metadata,
        }] = intents.as_slice()
        else {
            panic!("expected one switch-model intent: {intents:?}");
        };
        assert_eq!(profile, "plan");
        assert_eq!(launch_metadata.profile(), "plan");
        assert_eq!(launch_metadata.switchable_profiles(), &["build", "plan"]);
    }

    app.handle_key(key(KeyCode::BackTab));

    assert_eq!(app.active_profile(), "build");
    let intents = intents.lock().unwrap_or_abort();
    let Some(UiIntent::SwitchModel {
        profile,
        launch_metadata,
    }) = intents.get(1)
    else {
        panic!("expected second switch-model intent: {intents:?}");
    };
    assert_eq!(profile, "build");
    assert_eq!(launch_metadata.profile(), "build");
}

pub(super) fn agent_cycle_preserves_user_selected_provider_model_across_profiles() {
    let build_codex = runtime_context_model_option(
        "build",
        "openai-codex",
        "gpt-5.5",
        Some("high"),
        "GPT 5.5 · High",
    );
    let build_default = runtime_context_model_option(
        "build",
        "default",
        "gpt-5.5",
        Some("high"),
        "GPT 5.5 · High",
    );
    let plan_default = runtime_context_model_option(
        "plan",
        "default",
        "gpt-5.5",
        Some("xhigh"),
        "GPT 5.5 · XHigh",
    );
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&build_codex)
            .with_available_models(vec![build_codex, build_default, plan_default])
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()]),
    );

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.active_profile(), "plan");
    assert_eq!(app.active_provider(), "openai-codex");
    assert_eq!(app.launch_metadata().model(), Some("gpt-5.5"));
    assert_eq!(app.launch_metadata().variant(), Some("high"));
    let intents = intents.lock().unwrap_or_abort();
    let [UiIntent::SwitchModel {
        profile,
        launch_metadata,
    }] = intents.as_slice()
    else {
        panic!("expected one switch-model intent: {intents:?}");
    };
    assert_eq!(profile, "plan");
    assert_eq!(launch_metadata.profile(), "plan");
    assert_eq!(launch_metadata.provider(), "openai-codex");
    assert_eq!(launch_metadata.model(), Some("gpt-5.5"));
    assert_eq!(launch_metadata.variant(), Some("high"));
}

pub(super) fn current_context_window_tokens_uses_runtime_context_after_model_switch() {
    let mut runtime_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("deterministic"),
        "GPT-5.4 Mini · Deterministic",
    );
    runtime_option.context_window_tokens = Some(64000);

    let next_turn_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("creative"),
        "GPT-5.4 Mini · Creative",
    );

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_option(&runtime_option));
    app.ingest_event(envelope(
        1,
        "req_ctx_window",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_ctx_window".into(),
            text: "hello".to_string(),
        }),
    ));
    app.set_launch_metadata(LaunchMetadata::from_model_option(&next_turn_option));

    assert_eq!(
        app.current_context_window_tokens(),
        Some(64000),
        "context window limit should follow the current runtime context, \
         not the next-turn launch metadata"
    );
    assert_eq!(
        app.launch_metadata().context_window_tokens(),
        None,
        "sanity: next-turn launch metadata has no context window in this test"
    );
}

pub(super) fn switching_agent_after_submit_keeps_existing_turn_footer_agent() {
    let build_option =
        runtime_context_model_option("build", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");
    let plan_option =
        runtime_context_model_option("plan", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&build_option)
            .with_available_models(vec![build_option, plan_option])
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()]),
    );

    for ch in "keep footer agent".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.active_profile(), "plan");
    let rendered = render_debug(&app, 100, 32);
    assert!(
        rendered.contains("Build · active"),
        "submitted turn footer should keep its original agent after switching\n{rendered}"
    );
    assert!(
        !rendered.contains("Plan · active"),
        "submitted turn footer must not follow the newly selected agent\n{rendered}"
    );
}
