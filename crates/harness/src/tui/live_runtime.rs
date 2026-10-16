use super::new_live::NewLiveRuntime;
use super::profile_log::profile_handoff;
use super::{
    continue_launch_metadata, forward_events_to_tui, handle_ui_intents,
    latest_request_id_for_agent, latest_run_name, load_events_from_run_dir,
    load_recorded_runtime_context, recover_mutex_lock, select_resume_agent_id, user_actor,
    LaunchSelection, LiveAgentTarget, LiveCoordinatorConfigWarmup, LiveSettings,
    TuiAuthBackendContext,
};
use crate::cli_config::apply_runtime_metadata;
use crate::logging;
use crate::scenarios::supervisor_actor;
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::coord::spawn_coordinator;
use harness_core::proj::SessionModeSource;
use harness_core::redact::DefaultRedactor;
use harness_core::session_title::create_default_title;
use harness_tui::app::LaunchMetadata;
use harness_tui::{LiveUpdate, LiveUpdateSender, UiIntent};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
#[expect(
    clippy::too_many_arguments,
    reason = "shared bootstrap takes explicit session channels"
)]
pub(super) async fn bootstrap(
    settings: &LiveSettings,
    demo_mode: bool,
    run_id: String,
    run_dir: PathBuf,
    launch_selection: LaunchSelection,
    warmup: LiveCoordinatorConfigWarmup,
    updates: LiveUpdateSender,
    intents: mpsc::UnboundedReceiver<UiIntent>,
) -> Result<
    (
        NewLiveRuntime,
        LaunchMetadata,
        Vec<harness_core::event::EventEnvelopeV1>,
    ),
    String,
> {
    let plan = harness_core::proj::inspect_resume_plan(&run_dir);
    if !plan.is_resumable {
        return Err(format!(
            "continue session is disabled for {run_id}: {}",
            plan.resume_disabled_reason
                .unwrap_or_else(|| crate::defaults::RESUME_UNAVAILABLE_FALLBACK_REASON.into())
        ));
    }
    let events = load_events_from_run_dir(&run_dir)?;
    let agent_id = select_resume_agent_id(&plan, &events, &run_id)?;
    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };
    let mut config = warmup.coordinator_config(settings, demo_mode).await?;
    crate::cli_config::apply_runtime_metadata(
        &mut config,
        settings.deterministic,
        &settings.config_digest,
    );
    let coordinator = spawn_coordinator(config, clock, Arc::new(DefaultRedactor::default()));
    let run = coordinator
        .resume_run(
            run_id,
            latest_run_name(&events)
                .unwrap_or_else(|| crate::defaults::DEFAULT_INTERACTIVE_RUN_NAME.into()),
        )
        .await
        .map_err(|e| e.to_string())?;
    if let Some(config) = settings.config.as_ref() {
        let _ = crate::logging::init_logging(config, &run.run_dir)?;
    }
    let store = coordinator.event_store().await.map_err(|e| e.to_string())?;
    let selected = recover_mutex_lock(&launch_selection).metadata.clone();
    let recorded = load_recorded_runtime_context(&run_dir);
    let metadata = continue_launch_metadata(
        run.run_id.as_str(),
        recorded.as_ref(),
        &events,
        &agent_id,
        plan.known_agents.get(&agent_id).map(String::as_str),
    )
    .with_available_models(selected.available_models().to_vec())
    .with_resolved_models(selected.resolved_models().to_vec());
    let _ = updates.send(LiveUpdate::YoloModeChanged {
        enabled: coordinator.yolo_mode().await.map_err(|e| e.to_string())?,
    });
    let target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id.clone()),
        profile: metadata.profile().into(),
        last_request_id: latest_request_id_for_agent(&events, &agent_id),
    }));
    let forwarder = tokio::spawn(forward_events_to_tui(
        store,
        updates.clone(),
        events.last().map_or(1, |e| e.seq.saturating_add(1)),
        Some(Arc::clone(&target)),
        false,
    ));
    let auth =
        TuiAuthBackendContext::from_settings(settings).with_login_refresh(warmup, launch_selection);
    if let Some(notice) = auth.model_prompt_notice(&metadata) {
        let _ = updates.send(notice);
    }
    let handler = tokio::spawn(handle_ui_intents(
        coordinator.clone(),
        intents,
        user_actor(),
        Some(target),
        updates,
        auth,
    ));
    Ok((
        NewLiveRuntime {
            coordinator,
            event_forwarder_task: forwarder,
            ui_intent_task: handler,
        },
        metadata,
        events,
    ))
}
#[expect(
    clippy::too_many_arguments,
    reason = "bootstrap receives explicit workspace and runtime channels"
)]
pub(super) async fn bootstrap_new_live_runtime(
    settings: &LiveSettings,
    demo_mode: bool,
    workspace: PathBuf,
    run_id_override: String,
    launch_metadata: LaunchMetadata,
    coordinator_config_warmup: LiveCoordinatorConfigWarmup,
    launch_selection: LaunchSelection,
    live_update_tx: LiveUpdateSender,
    intent_rx: mpsc::UnboundedReceiver<UiIntent>,
) -> Result<NewLiveRuntime, String> {
    profile_handoff("new_live.workspace_ready");

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let mut coordinator_config = coordinator_config_warmup
        .coordinator_config(settings, demo_mode)
        .await?;
    profile_handoff("new_live.coordinator_ready");
    coordinator_config.run_id_override = Some(run_id_override);
    coordinator_config.session_mode_source = Some(if demo_mode {
        SessionModeSource::InteractiveMock
    } else {
        SessionModeSource::InteractiveLive
    });
    apply_runtime_metadata(
        &mut coordinator_config,
        settings.deterministic,
        &settings.config_digest,
    );

    let run_name = create_default_title(clock.as_ref(), false);
    let yolo_on_start = coordinator_config.yolo_on_start;
    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );
    profile_handoff("new_live.coordinator_spawned");

    let run = coordinator
        .start_run(run_name, &workspace)
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("new_live.start_run_done");
    let _ = live_update_tx.send(LiveUpdate::YoloModeChanged {
        enabled: yolo_on_start,
    });
    if let Some(config) = settings.config.as_ref() {
        let _ = logging::init_logging(config, &run.run_dir)?;
    }
    let store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("new_live.event_store_done");

    let agent_id = coordinator
        .spawn_agent_idle(
            supervisor_actor(),
            launch_metadata.profile().to_string(),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    profile_handoff("new_live.spawn_agent_idle_done");

    let live_agent_target = Arc::new(Mutex::new(LiveAgentTarget {
        agent_id: Some(agent_id),
        profile: launch_metadata.profile().to_string(),
        last_request_id: None,
    }));
    let event_forwarder_task = tokio::spawn({
        let live_update_tx = live_update_tx.clone();
        let forwarder_live_agent_target = Arc::clone(&live_agent_target);
        async move {
            forward_events_to_tui(
                store,
                live_update_tx,
                1,
                Some(forwarder_live_agent_target),
                false,
            )
            .await
        }
    });

    let intent_coordinator = coordinator.clone();
    let intent_live_agent_target = Arc::clone(&live_agent_target);
    let auth_backend = TuiAuthBackendContext::from_settings(settings)
        .with_login_refresh(coordinator_config_warmup, launch_selection);
    let ui_intent_task = tokio::spawn(async move {
        handle_ui_intents(
            intent_coordinator,
            intent_rx,
            user_actor(),
            Some(intent_live_agent_target),
            live_update_tx,
            auth_backend,
        )
        .await
    });

    Ok(NewLiveRuntime {
        coordinator,
        event_forwarder_task,
        ui_intent_task,
    })
}
