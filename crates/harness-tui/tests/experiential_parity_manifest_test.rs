use std::collections::BTreeSet;

use harness_tui::UnwrapOrAbort;
use serde_json::Value;

const MANIFEST: &str = include_str!("fixtures/experiential-parity-manifest.v1.json");
const MODULE_ROOTS: &[&str] = &[
    "inline_image",
    "video_viewer",
    "mermaid_worker",
    "terminal_title",
    "terminal_notifications",
    "contextual_tips",
    "capability_matrix",
    "lifecycle_choreography",
    "perf_budgets",
];
const PATHS: &[&str] = &["native_path", "fallback_path"];

#[test]
fn manifest_covers_every_module_root_and_terminal_tier() {
    // Given: the checked-in cartesian-product coverage contract.
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap_or_abort();
    let modules = manifest["module_roots"].as_array().unwrap_or_abort();
    let tiers = manifest["terminal_tiers"].as_array().unwrap_or_abort();

    // When: the manifest is expanded into its production coverage pairs.
    let module_names: BTreeSet<&str> = modules
        .iter()
        .map(|module| module.as_str().unwrap_or_abort())
        .collect();
    let pair_count = module_names.len() * tiers.len();

    // Then: every established root and tier is present, classified, and has
    // an installed-binary scenario plus a comparator verdict.
    assert_eq!(module_names.len(), MODULE_ROOTS.len());
    assert!(MODULE_ROOTS.iter().all(|module| module_names.contains(module)));
    assert_eq!(tiers.len(), 31);
    for tier in tiers {
        assert!(tier["id"].as_str().is_some_and(|id| !id.is_empty()));
        assert!(PATHS.contains(&tier["path_classification"].as_str().unwrap_or("")));
        assert!(tier["installed_binary_scenario"]
            .as_str()
            .is_some_and(|scenario| scenario.contains("HARNESS_BIN")));
        assert!(matches!(
            tier["comparator_verdict"].as_str(),
            Some("match" | "fallback_match")
        ));
    }
    assert_eq!(pair_count, MODULE_ROOTS.len() * 31);
}

#[test]
fn production_runtime_reaches_every_experiential_module() {
    // Given: production source, not a test-only worker harness.
    let runtime = include_str!("../src/runtime.rs");
    let terminal = include_str!("../src/terminal.rs");
    let integration = include_str!("../src/runtime_integration.rs");

    // When: the source is checked for the live ownership seam.
    // Then: removing any module from run_tui integration breaks this guard.
    assert!(runtime.contains("RuntimeExperience"));
    for module in MODULE_ROOTS {
        assert!(
            runtime.contains(module) || terminal.contains(module) || integration.contains(module),
            "production runtime lost reachability for {module}"
        );
    }
    for token in [
        "drain_live_updates",
        "post_flush",
        "FocusGained",
        "FocusLost",
        "cancel",
        "teardown_terminal_session",
    ] {
        assert!(
            runtime.contains(token) || integration.contains(token),
            "runtime seam lost {token}"
        );
    }
}
