#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use harness_testkit::parity::catalog::CORE_SCENARIOS;
use harness_testkit::parity::{IdentityMaskRegistry, SEMANTIC_FRAME_SCHEMA_VERSION};
use harness_testkit::tui_fidelity::{
    AdapterKind, CaptureMode, CheckpointName, MotionBoundary, MotionObservationRule, Scenario,
    ScenarioAction, ScenarioError, SubstitutionField, SubstitutionKind,
};

const STARTUP_SMOKE: &str = include_str!("fixtures/tui_fidelity/startup-smoke.json");
const PACKET2_SUSTAINED_STREAM: &str =
    include_str!("fixtures/tui_fidelity/packet2-sustained-stream.json");
const CANARY_TERMINAL_QUERY: &str =
    include_str!("../src/tui_fidelity_scenarios/baseline/canary-terminal-query.json");
const BASELINE_STARTUP: &str = include_str!("../src/tui_fidelity_scenarios/baseline/startup.json");

#[test]
fn baseline_current_parity_support_exposes_semantic_frames_and_identity_cells() {
    // arrange: the current Harness parity surface before typed scenarios exist.
    // act: the existing catalog and identity mask API are inspected.
    // assert: the baseline remains semantic-frame based with explicit cell masks.
    assert_eq!(SEMANTIC_FRAME_SCHEMA_VERSION, "semantic-frame-v1");
    assert_eq!(CORE_SCENARIOS.len(), 8);

    let masks = IdentityMaskRegistry::new().with_field("product_title", [(1, 2)]);
    assert_eq!(masks.grapheme_mask_field(1, 2), Some("product_title"));
}

#[test]
fn packet2_sustained_stream_preserves_input_and_resize_contract() {
    // arrange
    let scenario = Scenario::from_json(PACKET2_SUSTAINED_STREAM).expect("packet2 parses");

    let typed = scenario.actions.iter().find_map(|action| match action {
        ScenarioAction::TypeText(action) => Some((action.text.as_str(), action.inter_byte_millis)),
        _ => None,
    });
    // act
    let resizes = scenario
        .actions
        .iter()
        .filter_map(|action| match action {
            ScenarioAction::Resize(action) => Some((
                action.viewport.cols,
                action.viewport.rows,
                action.dwell_millis,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();

    // assert
    assert_eq!(typed, Some(("typed-while-streaming", 45)));
    assert_eq!("typed-while-streaming".len(), 21);
    assert_eq!(
        resizes,
        vec![
            (100, 35, 120),
            (160, 55, 120),
            (100, 35, 120),
            (160, 55, 120),
            (120, 40, 120)
        ]
    );
    assert_eq!(
        scenario
            .actions
            .iter()
            .filter(|action| matches!(action, ScenarioAction::Wheel(action) if action.amount == 8))
            .count(),
        1
    );
    assert_eq!(scenario.actions.iter().filter(|action| matches!(action, ScenarioAction::TimedKey(action) if action.key.modifiers.ctrl && action.key.code == harness_testkit::tui_fidelity::KeyCode::Char('c'))).count(), 1);
}

#[test]
fn startup_smoke_fixture_validates_for_both_adapter_kinds() {
    // arrange: one independently authored startup-smoke scenario.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // act: the same scenario is checked against each supported adapter.
    let grok = scenario.validate_for_adapter(AdapterKind::Grok);
    let harness = scenario.validate_for_adapter(AdapterKind::Harness);

    // assert: both adapter paths accept the typed contract.
    assert!(grok.is_ok(), "grok validation failed: {grok:?}");
    assert!(harness.is_ok(), "harness validation failed: {harness:?}");
}

#[test]
fn canary_terminal_query_uses_action_tail_capture_without_changing_defaults() {
    // arrange: the focused terminal-query canary and the existing startup fixture.
    let canary = Scenario::from_json(CANARY_TERMINAL_QUERY).expect("canary parses");
    let default = Scenario::from_json(STARTUP_SMOKE).expect("startup parses");

    // act: their capture contracts are inspected.
    // assert: only the canary opts into the narrow action-tail surface.
    assert_eq!(canary.capture_mode, CaptureMode::ActionTail);
    assert_eq!(default.capture_mode, CaptureMode::FullSession);
}

#[test]
fn startup_smoke_fixture_round_trips_byte_stably() {
    // arrange: a valid typed scenario.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // act: it is serialized, parsed, and serialized again canonically.
    let first = scenario.to_json().expect("scenario serializes");
    let second = Scenario::from_json(&first)
        .expect("canonical scenario parses")
        .to_json()
        .expect("canonical scenario serializes");

    // assert: serde round-trip output is stable.
    assert_eq!(first, second);
}

#[test]
fn startup_smoke_fixture_covers_every_action_variant() {
    // arrange: the independently authored action sequence.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // act: action variants are classified by the closed enum.
    let kinds: Vec<&str> = scenario
        .actions
        .iter()
        .map(ScenarioAction::kind_name)
        .collect();

    // assert: each supported action appears exactly once.
    assert_eq!(
        kinds,
        vec![
            "timed_key",
            "paste",
            "mouse",
            "drag",
            "wheel",
            "resize",
            "wait_for_semantic_state",
            "terminal_reply"
        ]
    );
}

#[test]
fn startup_smoke_fixture_covers_ordered_checkpoints_without_synthetic_masks() {
    // arrange: identical synthetic runtimes with ordered captures.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // act: checkpoint names and substitutions are collected.
    let checkpoints: Vec<CheckpointName> = scenario
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.name)
        .collect();
    // assert: the exact capture order is present without masking identical fixture cells.
    assert_eq!(
        checkpoints,
        vec![
            CheckpointName::Rest,
            CheckpointName::Mid,
            CheckpointName::Settled
        ]
    );
    assert!(scenario.substitutions.is_empty());
}

#[test]
fn startup_smoke_fixture_preserves_terminal_data_as_data() {
    // arrange: a valid scenario containing paste and terminal-reply payloads.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // act: the actions are inspected without executing them.
    let paste = scenario.actions.iter().find_map(|action| match action {
        ScenarioAction::Paste(action) => Some(action.text.as_str()),
        _ => None,
    });
    let reply = scenario.actions.iter().find_map(|action| match action {
        ScenarioAction::TerminalReply(action) => Some(action.response.as_str()),
        _ => None,
    });

    // assert: payloads remain literal typed data.
    assert_eq!(paste, Some("arness"));
    assert_eq!(reply, Some("\u{1b}[?1;2c"));
}

#[test]
fn home_path_substitution_field_round_trips_with_canonical_placeholder() {
    // arrange
    let encoded = serde_json::to_string(&SubstitutionField::HomePath).expect("field serializes");
    // act
    let decoded: SubstitutionField = serde_json::from_str(&encoded).expect("field deserializes");

    // assert
    assert_eq!(encoded, "\"home_path\"");
    assert_eq!(decoded, SubstitutionField::HomePath);
    assert_eq!(decoded.placeholder(), "[HOME]");
}

#[test]
fn baseline_startup_uses_field_scoped_truthful_release_and_home_substitutions() {
    // arrange
    let scenario = Scenario::from_json(BASELINE_STARTUP).expect("baseline startup parses");
    // act
    let mid_scopes = scenario
        .substitutions
        .iter()
        .filter(|substitution| substitution.checkpoint == CheckpointName::Mid)
        .map(|substitution| {
            (
                substitution.kind,
                substitution.field,
                substitution.rectangle,
            )
        })
        .collect::<Vec<_>>();

    // assert
    assert!(mid_scopes.contains(&(
        SubstitutionKind::TruthfulDynamicText,
        SubstitutionField::BuildVersion,
        harness_testkit::tui_fidelity::CellRect {
            col: 13,
            row: 6,
            cols: 5,
            rows: 1,
        }
    )));
    assert!(mid_scopes.contains(&(
        SubstitutionKind::TruthfulDynamicText,
        SubstitutionField::HomePath,
        harness_testkit::tui_fidelity::CellRect {
            col: 13,
            row: 16,
            cols: 10,
            rows: 1,
        }
    )));
    assert!(mid_scopes.iter().any(|(kind, field, rectangle)| {
        *kind == SubstitutionKind::IdentityText
            && *field == SubstitutionField::ProductLogo
            && *rectangle
                == harness_testkit::tui_fidelity::CellRect {
                    col: 6,
                    row: 6,
                    cols: 4,
                    rows: 7,
                }
    }));
    assert!(mid_scopes.iter().any(|(kind, field, rectangle)| {
        *kind == SubstitutionKind::TruthfulDynamicText
            && *field == SubstitutionField::ReleaseDate
            && *rectangle
                == harness_testkit::tui_fidelity::CellRect {
                    col: 21,
                    row: 6,
                    cols: 10,
                    rows: 1,
                }
    }));
    assert!(mid_scopes.iter().any(|(kind, field, rectangle)| {
        *kind == SubstitutionKind::TruthfulDynamicText
            && *field == SubstitutionField::ReleaseHistory
            && *rectangle
                == harness_testkit::tui_fidelity::CellRect {
                    col: 13,
                    row: 21,
                    cols: 18,
                    rows: 1,
                }
    }));
}

#[test]
fn baseline_startup_motion_begins_from_the_semantic_ready_frame() {
    // arrange: the startup packet waits for the expanded Changelog target before clicking it.
    let scenario = Scenario::from_json(BASELINE_STARTUP).expect("baseline startup parses");

    // act: the first ordered-motion marker is inspected.
    let marker = scenario
        .motion_capture
        .markers
        .first()
        .expect("startup motion marker");

    // assert: motion samples the last ready frame before the physical mouse-down action.
    assert_eq!(marker.boundary, MotionBoundary::BeforeAction { ordinal: 1 });
    assert_eq!(marker.observation, MotionObservationRule::FirstChanged);
}

#[test]
fn unsupported_adapter_selection_is_typed_error() {
    // arrange: a valid scenario that explicitly selects only Harness.
    let mut value: serde_json::Value = serde_json::from_str(STARTUP_SMOKE).expect("fixture JSON");
    value["adapters"] = serde_json::json!(["harness"]);
    let scenario = Scenario::from_json(&value.to_string()).expect("scenario parses");

    // act: the unselected adapter path is requested.
    let error = scenario
        .validate_for_adapter(AdapterKind::Grok)
        .expect_err("grok must be rejected");

    // assert: selection failure is represented by a typed scenario error.
    assert!(matches!(
        error,
        ScenarioError::AdapterNotSelected(AdapterKind::Grok)
    ));
}
