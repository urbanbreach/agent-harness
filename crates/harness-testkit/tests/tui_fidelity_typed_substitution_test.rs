#![allow(
    clippy::expect_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use harness_testkit::tui_fidelity::{Scenario, ScenarioError, SubstitutionError};

const TYPED_STARTUP: &str = include_str!("../src/tui_fidelity_scenarios/baseline/startup.json");

#[test]
fn inventory_shaped_truthful_dynamic_release_fields_are_accepted() {
    // arrange: a v2 scenario declares release version, date, and home spans explicitly.
    let mut value: serde_json::Value =
        serde_json::from_str(TYPED_STARTUP).expect("fixture JSON parses");
    value["schema_version"] = serde_json::json!("tui-fidelity-scenario-v2");
    let template = value["substitutions"][0].clone();
    value["substitutions"] = serde_json::json!([
        dynamic_substitution(
            &template,
            "build_version",
            "[VERSION]",
            "1.0.3",
            "0.1.0",
            5,
            5,
        ),
        dynamic_substitution(
            &template,
            "release_date",
            "[DATE]",
            "2026-08-12",
            "2026-08-19",
            10,
            10
        ),
        dynamic_substitution(
            &template,
            "release_history",
            "[RELEASE_HISTORY]",
            "1.0.2 — 2026-08-11",
            "Earlier changes   ",
            18,
            18
        ),
        dynamic_substitution(
            &template,
            "home_path",
            "[HOME]",
            "~/.grok",
            "~/.harness",
            7,
            10,
        ),
    ]);

    // act: the inventory-shaped JSON crosses the typed scenario boundary.
    let result = Scenario::from_json(&value.to_string());

    // assert: all three truthful dynamic field kinds are accepted.
    assert!(result.is_ok(), "typed dynamic scenario failed: {result:?}");
}

#[test]
fn legacy_scope_without_explicit_kind_field_and_provenance_is_rejected() {
    // arrange: the pre-v2 fixture shape is restored without compatibility defaults.
    let mut value = typed_fixture();
    let substitution = value["substitutions"][0]
        .as_object_mut()
        .expect("substitution object exists");
    substitution.remove("kind");
    substitution.remove("field");
    substitution.remove("canonical_placeholder");
    substitution.remove("reference_provenance");
    substitution.remove("candidate_provenance");
    substitution.insert("scope".to_owned(), serde_json::json!("workspace_path"));

    // act: legacy JSON crosses the v2 boundary.
    let result = Scenario::from_json(&value.to_string());

    // assert: no compatibility shim silently classifies the substitution.
    assert!(matches!(result, Err(ScenarioError::Deserialize(_))));
}

#[test]
fn unknown_substitution_kind_is_rejected() {
    // arrange
    let mut value = typed_fixture();
    value["substitutions"][0]["kind"] = serde_json::json!("dynamic_text");

    // act
    let result = Scenario::from_json(&value.to_string());

    // assert
    assert!(matches!(result, Err(ScenarioError::Deserialize(_))));
}

#[test]
fn unknown_substitution_field_is_rejected() {
    // arrange
    let mut value = typed_fixture();
    value["substitutions"][0]["field"] = serde_json::json!("release_label");

    // act
    let result = Scenario::from_json(&value.to_string());

    // assert
    assert!(matches!(result, Err(ScenarioError::Deserialize(_))));
}

#[test]
fn missing_or_empty_reference_provenance_is_rejected() {
    // arrange
    let mut missing = typed_fixture();
    missing["substitutions"][0]
        .as_object_mut()
        .expect("substitution object exists")
        .remove("reference_provenance");
    let mut empty = typed_fixture();
    empty["substitutions"][0]["reference_provenance"] = serde_json::json!("  ");

    // act
    let missing_result = Scenario::from_json(&missing.to_string());
    let empty_result = Scenario::from_json(&empty.to_string());

    // assert
    assert!(matches!(missing_result, Err(ScenarioError::Deserialize(_))));
    assert!(matches!(
        empty_result,
        Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::MissingReferenceProvenance
        ))
    ));
}

#[test]
fn missing_or_empty_candidate_provenance_is_rejected() {
    // arrange
    let mut missing = typed_fixture();
    missing["substitutions"][0]
        .as_object_mut()
        .expect("substitution object exists")
        .remove("candidate_provenance");
    let mut empty = typed_fixture();
    empty["substitutions"][0]["candidate_provenance"] = serde_json::json!("  ");

    // act
    let missing_result = Scenario::from_json(&missing.to_string());
    let empty_result = Scenario::from_json(&empty.to_string());

    // assert
    assert!(matches!(missing_result, Err(ScenarioError::Deserialize(_))));
    assert!(matches!(
        empty_result,
        Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::MissingCandidateProvenance
        ))
    ));
}

#[test]
fn identity_and_dynamic_field_kinds_cannot_be_interchanged() {
    // arrange
    let mut value = typed_fixture();
    value["substitutions"][0]["kind"] = serde_json::json!("identity_text");

    // act
    let result = Scenario::from_json(&value.to_string());

    // assert
    assert!(matches!(
        result,
        Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::FieldKindMismatch
        ))
    ));
}

#[test]
fn duplicate_field_per_checkpoint_is_rejected() {
    // arrange
    let mut value = typed_fixture();
    let duplicate = value["substitutions"][0].clone();
    value["substitutions"]
        .as_array_mut()
        .expect("substitutions array exists")
        .push(duplicate);

    // act
    let result = Scenario::from_json(&value.to_string());

    // assert
    assert!(matches!(
        result,
        Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::DuplicateField { .. }
        ))
    ));
}

#[test]
fn row_shaped_substitution_scope_is_rejected() {
    // arrange
    let mut value = typed_fixture();
    value["substitutions"][0]["rectangle"] =
        serde_json::json!({"col": 0, "row": 1, "cols": 75, "rows": 1});
    value["substitutions"][0]["reference"]["cell_width"] = serde_json::json!(75);
    value["substitutions"][0]["reference"]["padding_right"] = serde_json::json!(0);
    value["substitutions"][0]["candidate"]["cell_width"] = serde_json::json!(9);
    value["substitutions"][0]["candidate"]["padding_right"] = serde_json::json!(66);

    // act
    let result = Scenario::from_json(&value.to_string());

    // assert
    assert!(matches!(
        result,
        Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::BroadRegion
        ))
    ));
}

#[test]
fn identity_text_preserves_canonical_product_placeholder_behavior() {
    // arrange
    let mut value = typed_fixture();
    value["substitutions"][0]["kind"] = serde_json::json!("identity_text");
    value["substitutions"][0]["field"] = serde_json::json!("product_title");
    value["substitutions"][0]["canonical_placeholder"] = serde_json::json!("[PRODUCT]");
    value["substitutions"][0]["rectangle"]["col"] = serde_json::json!(23);
    value["substitutions"][0]["rectangle"]["row"] = serde_json::json!(6);
    value["substitutions"][0]["rectangle"]["cols"] = serde_json::json!(10);
    value["substitutions"][0]["reference"]["text"] = serde_json::json!("Grok Build");
    value["substitutions"][0]["reference"]["cell_width"] = serde_json::json!(10);
    value["substitutions"][0]["reference"]["padding_right"] = serde_json::json!(0);
    value["substitutions"][0]["candidate"]["text"] = serde_json::json!("Harness");
    value["substitutions"][0]["candidate"]["cell_width"] = serde_json::json!(7);
    value["substitutions"][0]["candidate"]["padding_right"] = serde_json::json!(3);

    // act
    let result = Scenario::from_json(&value.to_string());

    // assert
    assert!(result.is_ok(), "identity substitution failed: {result:?}");
}

#[test]
fn all_twenty_six_baseline_scenarios_use_the_explicit_v2_contract() {
    // arrange
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/tui_fidelity_scenarios/baseline");
    let mut paths = std::fs::read_dir(directory)
        .expect("baseline directory exists")
        .map(|entry| entry.expect("baseline entry exists").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter(|path| path.file_name().is_some_and(|name| name != "registry.json"))
        .collect::<Vec<_>>();
    paths.sort();

    // act
    let scenarios = paths
        .iter()
        .map(|path| {
            let input = std::fs::read_to_string(path).expect("baseline scenario is readable");
            Scenario::from_json(&input).expect("baseline scenario satisfies v2")
        })
        .collect::<Vec<_>>();

    // assert
    assert_eq!(scenarios.len(), 26);
    assert!(scenarios.iter().all(|scenario| {
        scenario.schema_version == "tui-fidelity-scenario-v2"
            && scenario.substitutions.iter().all(|substitution| {
                !substitution.reference_provenance.trim().is_empty()
                    && !substitution.candidate_provenance.trim().is_empty()
                    && substitution.reference.text != substitution.candidate.text
            })
    }));
}

fn typed_fixture() -> serde_json::Value {
    let mut value: serde_json::Value =
        serde_json::from_str(TYPED_STARTUP).expect("fixture JSON parses");
    value["substitutions"] = serde_json::json!([value["substitutions"][0].clone()]);
    value
}

fn dynamic_substitution(
    template: &serde_json::Value,
    field: &str,
    canonical_placeholder: &str,
    reference_text: &str,
    candidate_text: &str,
    reference_width: u16,
    candidate_width: u16,
) -> serde_json::Value {
    let mut substitution = template.clone();
    let object = substitution
        .as_object_mut()
        .expect("substitution object exists");
    object.remove("scope");
    object.insert(
        "kind".to_owned(),
        serde_json::json!("truthful_dynamic_text"),
    );
    object.insert("field".to_owned(), serde_json::json!(field));
    object.insert(
        "canonical_placeholder".to_owned(),
        serde_json::json!(canonical_placeholder),
    );
    object.insert(
        "reference_provenance".to_owned(),
        serde_json::json!(format!("reference-runtime:release-notes:{field}")),
    );
    object.insert(
        "candidate_provenance".to_owned(),
        serde_json::json!(format!("candidate-runtime:release-notes:{field}")),
    );
    let rectangle_width = reference_width.max(candidate_width);
    substitution["rectangle"] =
        serde_json::json!({"col": 2, "row": 1, "cols": rectangle_width, "rows": 1});
    substitution["reference"]["text"] = serde_json::json!(reference_text);
    substitution["reference"]["cell_width"] = serde_json::json!(reference_width);
    substitution["reference"]["padding_left"] = serde_json::json!(0);
    substitution["reference"]["padding_right"] =
        serde_json::json!(rectangle_width - reference_width);
    substitution["candidate"]["text"] = serde_json::json!(candidate_text);
    substitution["candidate"]["cell_width"] = serde_json::json!(candidate_width);
    substitution["candidate"]["padding_left"] = serde_json::json!(0);
    substitution["candidate"]["padding_right"] =
        serde_json::json!(rectangle_width - candidate_width);
    substitution
}
