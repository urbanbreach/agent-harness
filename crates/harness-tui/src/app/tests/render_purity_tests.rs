//! T07: view-model projection and render composition must be side-effect free.
//!
//! Contract: repeated projection/render leaves SessionProjection unchanged and
//! does not emit UiIntent. Mutation and intent emission stay in input/orchestration
//! paths (`handle_key`, slash commands, live update drain), not in render.

use super::*;
use harness_core::event::{EventV1, UserMessageSubmittedEvent};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn sample_user_message_event(seq: u64) -> EventEnvelopeV1 {
    let request_id = format!("req_render_purity_{seq}");
    envelope(
        seq,
        request_id.as_str(),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.clone().into(),
            text: format!("render purity prompt {seq}"),
        }),
    )
}

fn projection_fingerprint(app: &AppState) -> (usize, Vec<u64>, Vec<String>, Option<String>) {
    let event_seqs: Vec<u64> = app.events.iter().map(|event| event.seq).collect();
    let activity_ids: Vec<String> = app
        .activities
        .iter()
        .map(|activity| activity.request_id.clone())
        .collect();
    (
        app.events.len(),
        event_seqs,
        activity_ids,
        app.status_banner.clone(),
    )
}

fn intent_sink() -> (Arc<Mutex<Vec<UiIntent>>>, Arc<dyn Fn(UiIntent) + Send + Sync>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    (intents, sink)
}

fn project_and_render(app: &AppState) {
    let _runtime = app.runtime_state();
    let _footer = app.footer_hints_view_model();
    let _dock = app.control_dock_view_model();
    let _disabled = app.composer_disabled();
    let area = Rect::new(0, 0, 120, 40);
    let _plan = FrameLayoutPlan::for_app(app, area);
    let _ = render_debug(app, 120, 40);
    let _ = render_text(app, 100, 30);
}

/// Given a live session with projected events and an intent sink,
/// When projection and render run repeatedly,
/// Then no UiIntent is emitted and SessionProjection stays unchanged.
pub(super) fn repeated_projection_and_render_leaves_intent_queue_empty_and_projection_unchanged() {
    let (intents, sink) = intent_sink();
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/t07-render-purity-live")),
        false,
        Some(sink),
    );
    app.ingest_event(sample_user_message_event(1));
    app.ingest_event(sample_user_message_event(2));

    let before = projection_fingerprint(&app);
    assert_eq!(before.0, 2, "precondition: two projected events");
    assert_eq!(before.2.len(), 2, "precondition: two projected activities");
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "precondition: intent queue empty before projection/render"
    );

    for _ in 0..5 {
        project_and_render(&app);
    }

    let after = projection_fingerprint(&app);
    assert_eq!(
        after, before,
        "repeated projection/render must not mutate SessionProjection or status banner"
    );
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "view-model/render composition must not emit UiIntent"
    );
}

/// Given a replay session with an intent sink,
/// When projection and render run repeatedly,
/// Then no UiIntent is emitted and replay projection stays unchanged.
pub(super) fn repeated_replay_projection_and_render_is_side_effect_free() {
    let (intents, sink) = intent_sink();
    let events = vec![sample_user_message_event(1), sample_user_message_event(2)];
    let mut app = AppState::new_replay(PathBuf::from("/tmp/t07-render-purity-replay"), events);
    app.on_ui_intent = Some(sink);

    let before = projection_fingerprint(&app);
    let first_runtime = app.runtime_state();
    let first_dock = app.control_dock_view_model();
    let first_footer = app.footer_hints_view_model();

    for _ in 0..4 {
        project_and_render(&app);
    }

    assert_eq!(
        app.runtime_state(),
        first_runtime,
        "runtime projection must be stable across repeated reads"
    );
    assert_eq!(
        app.control_dock_view_model(),
        first_dock,
        "control-dock view model must be stable across repeated reads"
    );
    assert_eq!(
        app.footer_hints_view_model(),
        first_footer,
        "footer hints view model must be stable across repeated reads"
    );
    assert_eq!(
        projection_fingerprint(&app),
        before,
        "replay projection must be unchanged by pure composition"
    );
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "replay projection/render must not emit UiIntent"
    );
}

/// Given pure view-model inputs,
/// When the pure adapters are called repeatedly,
/// Then outputs are identical (no hidden mutable caches in view_model.rs).
pub(super) fn pure_view_model_adapters_are_deterministic() {
    let first = crate::view_model::runtime_state(crate::view_model::RuntimeStateInput {
        replay_mode: false,
        lifecycle_shell_state: LifecycleShellState::None,
        continue_disabled_banner: None,
        status_banner: Some("idle"),
        event_count: 2,
        last_event: None,
        latest_activity: None,
        activity_count: 0,
        active_permission: None,
    });
    let second = crate::view_model::runtime_state(crate::view_model::RuntimeStateInput {
        replay_mode: false,
        lifecycle_shell_state: LifecycleShellState::None,
        continue_disabled_banner: None,
        status_banner: Some("idle"),
        event_count: 2,
        last_event: None,
        latest_activity: None,
        activity_count: 0,
        active_permission: None,
    });
    assert_eq!(first, second);

    let footer_a = crate::view_model::footer_hints_view_model(crate::view_model::FooterHintsInput {
        replay_mode: false,
        review_surface_active: false,
        startup_shell_visible: false,
        focus: Focus::Prompt,
        composer_disabled: false,
        completed_session_shell_active: false,
        continued_live_run: false,
    });
    let footer_b = crate::view_model::footer_hints_view_model(crate::view_model::FooterHintsInput {
        replay_mode: false,
        review_surface_active: false,
        startup_shell_visible: false,
        focus: Focus::Prompt,
        composer_disabled: false,
        completed_session_shell_active: false,
        continued_live_run: false,
    });
    assert_eq!(footer_a, footer_b);
}
