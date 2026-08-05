use std::collections::BTreeSet;

use harness_testkit::tui_fidelity_dependency_cone::{parse_git_changes, DependencyCones};
use harness_testkit::tui_fidelity_obligation::{
    deduplicate_obligations, ObligationType, VerificationKey,
};
use harness_testkit::tui_fidelity_verify::{build_plan, PlanSelection, VerificationProfile};

use super::fixture::synthetic_fixture;

#[test]
fn all_profile_deduplicates_six_obligations_to_three_verification_keys() {
    // Given: six typed obligations sharing two non-runtime keys and one capture key.
    let (inventory, manifest) = synthetic_fixture();
    let selected = inventory
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();

    // When: obligations are converted into executable verification keys.
    let plan = deduplicate_obligations(&inventory, &manifest, &selected)
        .expect("synthetic obligations must deduplicate");

    // Then: each execution-effective key appears exactly once.
    assert_eq!(plan.obligation_count, 6);
    assert_eq!(plan.keys.len(), 3);
    assert_eq!(
        plan.keys
            .iter()
            .filter(|key| matches!(key, VerificationKey::DualCapture(_)))
            .count(),
        1
    );
}

#[test]
fn obligation_type_is_exhaustively_deserialized_from_inventory() {
    // Given: the synthetic inventory contains every non-reviewer obligation type.
    let (inventory, _) = synthetic_fixture();

    // When: the typed records are inspected.
    let observed = inventory
        .requirements
        .iter()
        .map(|record| record.obligation.obligation_type())
        .collect::<BTreeSet<_>>();

    // Then: no record fell through an untyped compatibility default.
    assert_eq!(
        observed,
        BTreeSet::from([
            ObligationType::DualCapture,
            ObligationType::OwnerTest,
            ObligationType::StaticGate,
        ])
    );
}

#[test]
fn dependency_cone_selects_scenario_obligations_for_known_path() {
    // Given: a path mapped transitively to one scenario family.
    let (inventory, manifest) = synthetic_fixture();
    let cones = DependencyCones::from_json(
        r#"{"schema_version":"harness.tui-fidelity.dependency-cones.v1","cones":[{"paths":["crates/harness-tui/src/motion/**"],"obligations":["scenario:synthetic-motion"]}]}"#,
    )
    .expect("dependency cone fixture");

    // When: a source path in that cone changes.
    let selection = cones
        .select(
            &["crates/harness-tui/src/motion/frame.rs".into()],
            &inventory,
            &manifest,
        )
        .expect("known path selection");

    // Then: all six requirements sharing the affected scenario are selected.
    assert!(!selection.fell_back_to_all);
    assert_eq!(selection.requirement_ids.len(), 6);
}

#[test]
fn dependency_cone_fails_closed_for_unknown_path() {
    // Given: a dependency map with no entry for a new source path.
    let (inventory, manifest) = synthetic_fixture();
    let cones = DependencyCones::from_json(
        r#"{"schema_version":"harness.tui-fidelity.dependency-cones.v1","cones":[{"paths":["known/**"],"obligations":["*"]}]}"#,
    )
    .expect("dependency cone fixture");

    // When: selection sees an unknown path.
    let selection = cones
        .select(&["new/unknown.rs".into()], &inventory, &manifest)
        .expect("fail-closed selection");

    // Then: the full inventory is selected and the path is recorded.
    assert!(selection.fell_back_to_all);
    assert_eq!(selection.requirement_ids.len(), 6);
    assert_eq!(selection.unknown_paths, vec!["new/unknown.rs"]);
}

#[test]
fn git_change_parser_keeps_renames_deletes_additions_and_untracked_paths() {
    // Given: NUL-delimited Git name-status and untracked output.
    let tracked = b"R100\0old.rs\0new.rs\0D\0gone.rs\0A\0added.rs\0";
    let untracked = b"untracked.rs\0";

    // When: changed paths are normalized.
    let paths = parse_git_changes(tracked, untracked).expect("Git output fixture");

    // Then: both sides of renames and every other change class are retained.
    assert_eq!(
        paths,
        vec!["added.rs", "gone.rs", "new.rs", "old.rs", "untracked.rs"]
    );
}

#[test]
fn synthetic_fixture_runs_through_all_changed_and_motion_profiles() {
    // Given: six obligations collapsing to two non-runtime keys and one motion key.
    let (inventory, manifest) = synthetic_fixture();
    let changed = BTreeSet::from([
        "dynamic-a".to_owned(),
        "static-a".to_owned(),
        "owner-a".to_owned(),
    ]);

    // When: every verification profile builds its execution plan.
    let plans = [
        build_plan(
            PlanSelection {
                profile: VerificationProfile::All,
                changed: None,
            },
            &inventory,
            &manifest,
        )
        .expect("all profile"),
        build_plan(
            PlanSelection {
                profile: VerificationProfile::Changed,
                changed: Some(&changed),
            },
            &inventory,
            &manifest,
        )
        .expect("changed profile"),
        build_plan(
            PlanSelection {
                profile: VerificationProfile::Motion,
                changed: None,
            },
            &inventory,
            &manifest,
        )
        .expect("motion profile"),
    ];

    // Then: all/changed retain static work while motion is dynamic-only.
    assert_eq!((plans[0].obligation_count, plans[0].keys.len()), (6, 3));
    assert_eq!((plans[1].obligation_count, plans[1].keys.len()), (3, 3));
    assert_eq!((plans[2].obligation_count, plans[2].keys.len()), (3, 1));
    assert!(matches!(
        plans[2].keys.as_slice(),
        [VerificationKey::DualCapture(_)]
    ));
}
