#![allow(clippy::expect_used, reason = "baseline owner fixtures fail fast")]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use harness_testkit::tui_fidelity::Scenario;
use serde::Deserialize;

const REGISTRY_RELATIVE: &str = "src/tui_fidelity_scenarios/baseline/registry.json";
const REQUIRED_STATES: [&str; 26] = [
    "natural-composer-draft",
    "canary-static-terminal-query",
    "canary-dynamic-resize",
    "canary-reduced-terminal",
    "startup",
    "draft",
    "idle",
    "stream",
    "tool",
    "diff",
    "permission",
    "question",
    "queue",
    "cancel",
    "fail",
    "recover",
    "complete",
    "scroll",
    "modal_surfaces",
    "dashboard",
    "media",
    "themes",
    "mouse",
    "resize",
    "cjk",
    "reduced_capabilities",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    viewports: Vec<ViewportSpec>,
    scenarios: Vec<ScenarioSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewportSpec {
    id: String,
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioSpec {
    id: String,
    path: String,
    state: String,
    owner_source_paths: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registry() -> Registry {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(REGISTRY_RELATIVE);
    let input = fs::read_to_string(&path).expect("baseline registry exists");
    serde_json::from_str(&input).expect("baseline registry has no unknown fields")
}

fn source_path_is_owned(path: &str) -> bool {
    ["crates/", "configs/", "docs/", "scripts/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        && !path.starts_with("inspirations/")
}

#[test]
fn baseline_registry_parses_every_strict_scenario() {
    // arrange
    let registry = registry();
    // act
    let states = registry
        .scenarios
        .iter()
        .map(|scenario| scenario.state.as_str())
        .collect::<BTreeSet<_>>();
    // assert
    assert_eq!(
        registry.schema_version,
        "harness.tui-fidelity.baseline-registry.v1"
    );
    assert_eq!(registry.viewports.len(), 21);
    assert_eq!(registry.scenarios.len(), REQUIRED_STATES.len());
    assert_eq!(states.len(), REQUIRED_STATES.len());
    assert!(REQUIRED_STATES.iter().all(|state| states.contains(state)));
    for scenario_spec in &registry.scenarios {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&scenario_spec.path);
        let input = fs::read_to_string(path).expect("scenario fixture exists");
        let scenario = Scenario::from_json(&input).expect("scenario satisfies strict contract");
        assert_eq!(scenario.id.0, scenario_spec.id);
        assert_eq!(scenario.adapters.len(), 2);
        assert_eq!(scenario.checkpoints.len(), 3);
    }
}

#[test]
fn baseline_registry_declares_unique_viewports_and_scenario_capture_keys() {
    // arrange
    let registry = registry();
    // act
    let viewports = registry
        .viewports
        .iter()
        .map(|viewport| (viewport.id.as_str(), viewport.cols, viewport.rows))
        .collect::<BTreeSet<_>>();
    let capture_keys = registry
        .scenarios
        .iter()
        .flat_map(|scenario| {
            registry
                .viewports
                .iter()
                .map(move |viewport| (scenario.id.as_str(), viewport.id.as_str()))
        })
        .collect::<BTreeSet<_>>();
    // assert
    assert_eq!(viewports.len(), registry.viewports.len());
    assert_eq!(
        capture_keys.len(),
        registry.scenarios.len() * registry.viewports.len()
    );
}

#[test]
fn baseline_registry_binds_existing_first_party_owners() {
    // arrange
    let registry = registry();
    // act
    let owner_paths = registry
        .scenarios
        .iter()
        .flat_map(|scenario| scenario.owner_source_paths.iter())
        .collect::<Vec<_>>();
    // assert
    assert!(!owner_paths.is_empty());
    assert!(owner_paths.iter().all(|path| source_path_is_owned(path)));
    assert!(owner_paths
        .iter()
        .all(|path| repo_root().join(path).is_file()));
}

#[test]
fn baseline_registry_contains_no_acceptance_or_evidence_claims() {
    // arrange
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(REGISTRY_RELATIVE);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(path).expect("baseline registry exists"))
            .expect("registry JSON");
    // act
    let scenarios = value["scenarios"].as_array().expect("scenario array");
    // assert
    for scenario in scenarios {
        for forbidden in [
            "status",
            "receipt",
            "reference_artifacts",
            "candidate_artifacts",
        ] {
            assert!(
                scenario.get(forbidden).is_none(),
                "unexpected field {forbidden}"
            );
        }
    }
}
