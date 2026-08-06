use std::collections::BTreeSet;

use serde::Deserialize;

const INVENTORY: &str = include_str!("fixtures/parity-module-reachability-inventory.v1.json");
const EXPECTED_MODULES: &[&str] = &[
    "design_contract",
    "theme_family",
    "welcome_surface",
    "composer_atoms",
    "composer_integration",
    "attachment_lifecycle",
    "transcript_identity",
    "transcript_blocks",
    "transcript_scroll",
    "transcript_integration",
    "dashboard",
    "dashboard_integration",
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

#[derive(Debug, Deserialize)]
struct Inventory {
    schema_version: String,
    scope: String,
    modules: Vec<ModuleEntry>,
}

#[derive(Debug, Deserialize)]
struct ModuleEntry {
    module_root: String,
    classification: String,
    call_chain: String,
    live_entry: String,
    installed_binary_scenario: String,
}

#[test]
fn checked_in_inventory_proves_every_parity_module_reaches_product() {
    // Given: the checked-in machine-readable reachability inventory.
    let inventory: Inventory =
        serde_json::from_str(INVENTORY).expect("inventory must be valid JSON");
    let module_names: BTreeSet<&str> = inventory
        .modules
        .iter()
        .map(|module| module.module_root.as_str())
        .collect();

    // When: the inventory is checked against the parity closure contract.
    assert_eq!(
        inventory.schema_version,
        "harness.tui-fidelity.parity-module-reachability.v1"
    );
    assert!(inventory.scope.contains("todos 8-48"));
    assert_eq!(module_names.len(), EXPECTED_MODULES.len());
    assert_eq!(module_names, EXPECTED_MODULES.iter().copied().collect());

    // Then: every retained root has a real product entry and cannot be test-only
    // or disconnected.
    for module in inventory.modules {
        assert!(
            matches!(
                module.classification.as_str(),
                "live_reachable" | "internal_to_live_owner"
            ),
            "{} has a non-product classification: {}",
            module.module_root,
            module.classification
        );
        assert!(!module.call_chain.trim().is_empty());
        assert!(!module.live_entry.trim().is_empty());
        assert!(module.installed_binary_scenario.contains("HARNESS_BIN"));
    }
}

#[test]
fn production_source_does_not_lose_inventory_reachability_seams() {
    // Given: the production entrypoints, not a test-only worker harness.
    let lib = include_str!("../src/lib.rs");
    let runtime = include_str!("../src/runtime.rs");
    let app = include_str!("../src/app.rs");
    let app_composer = include_str!("../src/app/composer.rs");
    let app_transcript = include_str!("../src/app/transcript_state.rs");
    let fidelity_config = include_str!("../src/fidelity_config/mod.rs");
    let terminal = include_str!("../src/terminal.rs");
    let ui = include_str!("../src/ui.rs");
    let ui_composer = include_str!("../src/ui_composer.rs");
    let ui_lifecycle = include_str!("../src/ui_lifecycle.rs");
    let ui_transcript = include_str!("../src/ui_transcript.rs");
    let status_dialog = include_str!("../src/ui_overlays/status_dialog.rs");
    let integration = include_str!("../src/runtime_integration.rs");

    // When: each inventory root is checked against the live source seams.
    // Then: removing a module from the shipped path breaks this regression guard.
    for module in EXPECTED_MODULES {
        assert!(
            lib.contains(&format!("pub mod {module}"))
                && (runtime.contains(module)
                    || app.contains(module)
                    || app_composer.contains(module)
                    || app_transcript.contains(module)
                    || fidelity_config.contains(module)
                    || terminal.contains(module)
                    || ui.contains(module)
                    || ui_composer.contains(module)
                    || ui_lifecycle.contains(module)
                    || ui_transcript.contains(module)
                    || status_dialog.contains(module)
                    || integration.contains(module)
                    || (*module == "dashboard" && integration.contains("dashboard_integration"))),
            "production source lost reachability seam for {module}"
        );
    }
}
