use std::collections::BTreeSet;

use harness_testkit::tui_fidelity_matrix::{CoverageManifest, RequirementInventory};
use harness_testkit::tui_fidelity_obligation::{deduplicate_obligations, VerificationKey};

pub(super) fn synthetic_fixture() -> (RequirementInventory, CoverageManifest) {
    let inventory: RequirementInventory = serde_json::from_value(serde_json::json!({
        "schema_version": "harness.tui-fidelity.requirement-inventory.v1",
        "reviewed_plan_sha256": "a".repeat(64),
        "requirements": [
            requirement("dynamic-a", "dual_capture", None),
            requirement("dynamic-b", "dual_capture", None),
            requirement("dynamic-c", "dual_capture", None),
            requirement("static-a", "static_gate", Some("static-contract")),
            requirement("static-b", "static_gate", Some("static-contract")),
            requirement("owner-a", "owner_test", Some("owner-reducer")),
        ]
    }))
    .expect("inventory fixture");
    let manifest: CoverageManifest = serde_json::from_value(serde_json::json!({
        "schema_version": "harness.tui-fidelity.coverage-manifest.v1",
        "reviewed_plan_sha256": "a".repeat(64),
        "inventory_sha256": "b".repeat(64),
        "rows": inventory.requirements.iter().enumerate().map(|(index, requirement)| {
            serde_json::json!({
                "row_id": format!("row-{index}"),
                "requirement_id": requirement.id,
                "scenario_id": "synthetic-motion",
                "action_path": "advance-frame",
                "path_classification": "native_path",
                "viewport": {"cols": 80, "rows": 24},
                "terminal_tier": "truecolor",
                "persona": "motion-sensitive",
                "theme_mode": "default",
                "media_mode": "none",
                "failure_path": "none",
                "trials": 5
            })
        }).collect::<Vec<_>>()
    }))
    .expect("manifest fixture");
    (inventory, manifest)
}

pub(super) fn synthetic_keys() -> Vec<VerificationKey> {
    let (inventory, manifest) = synthetic_fixture();
    let selected = inventory
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<BTreeSet<_>>();
    deduplicate_obligations(&inventory, &manifest, &selected)
        .expect("synthetic obligation plan")
        .keys
}

fn requirement(id: &str, kind: &str, key: Option<&str>) -> serde_json::Value {
    let obligation = match key {
        Some(key) => serde_json::json!({"type": kind, "key": key}),
        None => serde_json::json!({"type": kind}),
    };
    serde_json::json!({
        "id": id,
        "source_line": 1,
        "title": id,
        "obligation": obligation,
    })
}
