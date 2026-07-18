//! Validates docs/tui-reference-module-disposition.v1.json shape and §6 coverage.
//! Presentation implementation is out of scope for this test.

use harness_tui::UnwrapOrAbort;
use serde_json::Value;
use std::collections::BTreeSet;

const DISPOSITION: &str =
    include_str!("../../../docs/tui-reference-module-disposition.v1.json");

const ALLOWED: &[&str] = &[
    "replace",
    "rework",
    "retain-seam-only",
    "retain-with-reference-proof",
];

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn module_disposition_enums_and_contract_section_6_coverage() {
    // arrange
    let doc: Value = serde_json::from_str(DISPOSITION).unwrap_or_abort();
    assert_eq!(
        doc["schema_version"],
        "harness-tui-reference-module-disposition-v1"
    );
    assert_eq!(
        doc["policy"]["div_004_compose_first_as_parity"],
        "invalid"
    );

    let enum_list = doc["disposition_enum"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<BTreeSet<_>>();
    for allowed in ALLOWED {
        assert!(
            enum_list.contains(*allowed),
            "missing disposition enum {allowed}"
        );
    }
    assert_eq!(enum_list.len(), ALLOWED.len());

    let required = doc["contract_section_6_required_modules"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    assert!(
        !required.is_empty(),
        "contract §6 required module list must not be empty"
    );

    let modules = doc["modules"].as_array().unwrap_or_abort();
    assert!(!modules.is_empty(), "modules array must not be empty");

    let mut seen = BTreeSet::new();
    let mut flagged_section_6 = BTreeSet::new();

    // act + assert
    for module in modules {
        let path = module["path"].as_str().unwrap_or_abort();
        assert!(
            seen.insert(path.to_owned()),
            "duplicate module path {path}"
        );

        let disposition = module["disposition"].as_str().unwrap_or_abort();
        assert!(
            ALLOWED.contains(&disposition),
            "invalid disposition {disposition} for {path}"
        );

        if module["contract_section_6"].as_bool() == Some(true) {
            flagged_section_6.insert(path.to_owned());
        }
    }

    for path in &required {
        assert!(
            seen.contains(path.as_str()),
            "contract §6 module missing from modules[]: {path}"
        );
        assert!(
            flagged_section_6.contains(path.as_str()),
            "contract §6 module not flagged contract_section_6=true: {path}"
        );
    }

    for path in &flagged_section_6 {
        assert!(
            required.contains(path.as_str()),
            "contract_section_6=true but not in required list: {path}"
        );
    }
}
