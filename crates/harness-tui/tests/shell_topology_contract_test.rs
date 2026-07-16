#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration test helpers use fail-fast messages for missing layout rects"
)]

use std::path::PathBuf;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, RunStartedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::FrameLayoutPlan;
use ratatui::layout::Rect;

const CONTRACT_VIEWPORTS: [(u16, u16); 3] = [(120, 40), (100, 30), (80, 24)];
const WIDE_VIEWPORTS: [(u16, u16); 3] = [(121, 40), (140, 36), (160, 40)];

#[test]
fn live_session_shell_has_no_right_operator_sidebar_at_contract_viewports() {
    // arrange
    let app = live_session_app();

    for (width, height) in CONTRACT_VIEWPORTS {
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_no_operator_rail_primary_chrome(&plan, width, height);
    }
}

#[test]
fn live_session_transcript_spans_full_shell_width_above_composer() {
    // arrange
    let app = live_session_app();

    for (width, height) in CONTRACT_VIEWPORTS {
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_full_width_transcript_above_composer(&plan, width, height);
    }
}

#[test]
fn live_session_composer_is_bottom_anchored() {
    // arrange
    let app = live_session_app();

    for (width, height) in CONTRACT_VIEWPORTS {
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_composer_bottom_anchored(&plan, width, height);
    }
}

#[test]
fn live_session_sidebar_must_not_reappear_as_primary_chrome_at_width_ge_121() {
    // arrange
    let app = live_session_app();

    for (width, height) in WIDE_VIEWPORTS {
        // assert
        assert!(
            width >= 121,
            "wide-viewport matrix must stay at/above the 121-column threshold"
        );
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_no_operator_rail_primary_chrome(&plan, width, height);
        assert_full_width_transcript_above_composer(&plan, width, height);
        assert_composer_bottom_anchored(&plan, width, height);
    }
}

#[test]
fn live_session_replacement_topology_holds_across_all_named_viewports() {
    // arrange
    let app = live_session_app();
    let mut cases = Vec::with_capacity(CONTRACT_VIEWPORTS.len() + WIDE_VIEWPORTS.len());
    cases.extend(CONTRACT_VIEWPORTS);
    cases.extend(WIDE_VIEWPORTS);

    // act
    for (width, height) in cases {
        let plan = plan_for(&app, width, height);
        // assert
        assert_no_operator_rail_primary_chrome(&plan, width, height);
        assert_full_width_transcript_above_composer(&plan, width, height);
        assert_composer_bottom_anchored(&plan, width, height);
    }
}

fn live_session_app() -> AppState {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/run_topology_contract")),
        false,
        None,
    );
    for event in live_session_events() {
        app.ingest_event(event);
    }
    app
}

fn plan_for(app: &AppState, width: u16, height: u16) -> FrameLayoutPlan {
    FrameLayoutPlan::for_app(app, Rect::new(0, 0, width, height))
}

fn assert_no_operator_rail_primary_chrome(plan: &FrameLayoutPlan, width: u16, height: u16) {
    assert!(
        plan.operator_sidebar.is_none(),
        "replacement topology forbids a dedicated right operator-sidebar rect at {width}x{height}; got {:?}",
        plan.operator_sidebar
    );
    assert!(
        plan.details_overlay.is_none(),
        "replacement topology forbids operator-rail primary chrome via details overlay at {width}x{height}; got {:?}",
        plan.details_overlay
    );
    assert!(
        plan.wheel_hit_areas.inspector.is_none(),
        "replacement topology forbids an operator-rail inspector hit target as primary chrome at {width}x{height}; got {:?}",
        plan.wheel_hit_areas.inspector
    );
    assert!(
        plan.wheel_hit_areas.overlay.is_none(),
        "replacement topology forbids an operator-rail overlay hit target as primary chrome at {width}x{height}; got {:?}",
        plan.wheel_hit_areas.overlay
    );
}

fn assert_full_width_transcript_above_composer(plan: &FrameLayoutPlan, width: u16, height: u16) {
    let transcript = plan.transcript.unwrap_or_else(|| {
        panic!("live session shell must keep a transcript surface at {width}x{height}")
    });
    let composer = plan.composer.unwrap_or_else(|| {
        panic!("live session shell must keep a composer rect at {width}x{height}")
    });

    assert_eq!(
        transcript.width, plan.shell.width,
        "transcript must span full shell content width at {width}x{height} (no right-rail reservation); transcript={transcript:?} shell={:?}",
        plan.shell
    );
    assert_eq!(
        composer.width, plan.shell.width,
        "composer must span full shell content width at {width}x{height}; composer={composer:?} shell={:?}",
        plan.shell
    );
    assert_eq!(
        transcript.x, plan.shell.x,
        "transcript must share the shell left edge at {width}x{height}"
    );
    assert!(
        transcript.y + transcript.height <= composer.y,
        "transcript/scrollback must sit above the composer at {width}x{height}; transcript={transcript:?} composer={composer:?}"
    );
}

fn assert_composer_bottom_anchored(plan: &FrameLayoutPlan, width: u16, height: u16) {
    let composer = plan.composer.unwrap_or_else(|| {
        panic!("live session shell must keep a composer rect at {width}x{height}")
    });

    let dock_bottom = match plan.disclosure {
        Some(disclosure) => disclosure.y + disclosure.height,
        None => composer.y + composer.height,
    };

    assert_eq!(
        dock_bottom,
        plan.shell.y + plan.shell.height,
        "composer dock stack must be bottom-anchored in the live shell at {width}x{height}; composer={composer:?} disclosure={:?} shell={:?}",
        plan.disclosure,
        plan.shell
    );
    assert!(
        composer.y + composer.height <= plan.shell.y + plan.shell.height,
        "composer must not extend past the shell bottom at {width}x{height}; composer={composer:?} shell={:?}",
        plan.shell
    );
    if let Some(transcript) = plan.transcript {
        assert!(
            composer.y >= transcript.y + transcript.height,
            "bottom-anchored composer must remain below transcript/scrollback at {width}x{height}; composer={composer:?} transcript={transcript:?}"
        );
    }
}

fn live_session_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_topology_contract";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "topology-contract".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "Prove replacement shell topology".to_string(),
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "topology-model".to_string(),
                prompt_summary: "Prove replacement shell topology".to_string(),
                request_digest: "digest-topology-contract".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "Transcript content for topology geometry checks.".to_string(),
            }),
        ),
    ]
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-topology-{seq:04}"),
        seq,
        run_id: "run_topology_contract".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(
            ActorKind::System,
            Some("shell-topology-contract".to_string()),
        ),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_topology_contract".to_string()),
        payload,
    }
}
