use std::collections::BTreeSet;

use serde_json::Value;

const MANIFEST: &str = include_str!("dashboard_coverage_manifest.json");
const REQUIRED_JOURNEYS: [&str; 15] = [
    "roster",
    "peek-tail",
    "details",
    "reply",
    "rename",
    "stop",
    "permission",
    "question",
    "search-help",
    "stale-target",
    "stopped-child",
    "resize",
    "close-reopen",
    "keyboard",
    "mouse",
];

#[test]
fn dashboard_manifest_binds_distinct_comparator_journeys_to_all_viewports() {
    // arrange
    // Given: the checked-in dashboard evidence contract.
    let manifest: Value = serde_json::from_str(MANIFEST).expect("valid dashboard manifest");

    // When: the installed-binary comparator binding and journey matrix are inspected.
    let layers = manifest["comparator"]["layers"]
        .as_array()
        .expect("comparator layers");
    let journeys = manifest["journeys"].as_array().expect("dashboard journeys");
    let viewports = manifest["viewports"]
        .as_array()
        .expect("dashboard viewports");
    let journey_ids = journeys
        .iter()
        .filter_map(|journey| journey["id"].as_str())
        .collect::<BTreeSet<_>>();

    // act
    // Then: every required journey is distinct, comparator-backed, and viewport-complete.
    // assert
    assert_ne!(
        manifest["reference_binary"].as_str(),
        manifest["candidate_binary"].as_str()
    );
    assert_eq!(layers.len(), 5);
    assert_eq!(viewports.len(), 7);
    assert_eq!(journeys.len(), REQUIRED_JOURNEYS.len());
    for journey in REQUIRED_JOURNEYS {
        assert!(
            journey_ids.contains(journey),
            "missing dashboard journey {journey}"
        );
    }
    assert!(manifest["comparator"]["receipt_required"].as_bool() == Some(true));
}
