use harness_core::config::ShellAllowlist;
use harness_tools::UnwrapOrAbort;
use harness_tools::{coordinator_registry, native_tool_catalog_entries};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[path = "baseline_tools_parity_inventory_doctor_support.rs"]
mod doctor_support;

const EXTERNAL_REFERENCE_BUILTINS: &[&str] = &[
    "bash",
    "read",
    "glob",
    "grep",
    "edit",
    "write",
    "task",
    "webfetch",
    "websearch",
    "codesearch",
    "skill",
    "apply_patch",
    "batch",
    "todo",
    "question",
    "invalid",
];

#[derive(Debug, Deserialize)]
struct ParityInventoryRow {
    canonical_id: String,
    source: String,
    phase_scope: String,
    provider_function_name: Option<String>,
    aliases: Vec<String>,
    permission_kind: Option<String>,
    schema_status: Option<String>,
    active_profiles: Vec<String>,
    profile_description_overrides: Vec<String>,
    baseline_mapping_status: String,
    baseline_equivalent_id: Option<String>,
}

#[test]
fn inventory_covers_every_native_catalog_row() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let catalog = native_tool_catalog_entries(&registry);
    let inventory = load_parity_inventory();
    let inventory_by_id = inventory_by_id(&inventory);

    // act
    for entry in catalog {
        let row = inventory_by_id
            .get(entry.canonical_id.as_str())
            .unwrap_or_else(|| panic!("missing inventory row for {}", entry.canonical_id));

        // assert
        assert_eq!(
            row.source, "harness_native",
            "{} source drift",
            entry.canonical_id
        );
        assert_eq!(
            row.provider_function_name.as_deref(),
            Some(entry.provider_function_name.as_str()),
            "{} provider function name drift",
            entry.canonical_id
        );
        assert_eq!(
            row.aliases, entry.aliases,
            "{} alias drift",
            entry.canonical_id
        );
        assert_eq!(
            row.permission_kind.as_deref(),
            entry.permission_kind.as_deref(),
            "{} permission drift",
            entry.canonical_id
        );
        assert_eq!(
            row.schema_status.as_deref(),
            Some(entry.schema_status.as_str()),
            "{} schema drift",
            entry.canonical_id
        );
        assert_eq!(
            row.baseline_mapping_status, entry.baseline_mapping_status,
            "{} mapping status drift",
            entry.canonical_id
        );
        assert_eq!(
            row.baseline_equivalent_id.as_deref(),
            entry.baseline_equivalent_id.as_deref(),
            "{} baseline equivalent drift",
            entry.canonical_id
        );
    }
}

#[test]
fn inventory_covers_current_external_reference_builtins() {
    // arrange
    let inventory = load_parity_inventory();

    // act
    let covered_external_ids = inventory
        .iter()
        .filter_map(|row| {
            row.baseline_equivalent_id.as_deref().or_else(|| {
                (row.source == "external_reference").then_some(row.canonical_id.as_str())
            })
        })
        .collect::<BTreeSet<_>>();

    // assert
    assert_eq!(
        covered_external_ids,
        EXTERNAL_REFERENCE_BUILTINS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
    );
}

#[test]
fn inventory_rejects_unreviewed_status_permission_schema_or_provider_name_drift() {
    // arrange
    let inventory = load_parity_inventory();
    let allowed_statuses = allowed_statuses();

    // act
    for row in inventory {
        // assert
        assert!(
            allowed_statuses.contains(row.baseline_mapping_status.as_str()),
            "{} has invalid status {}",
            row.canonical_id,
            row.baseline_mapping_status
        );

        if matches!(row.phase_scope.as_str(), "P0" | "P1") {
            assert_ne!(
                row.baseline_mapping_status, "needs_work",
                "{} remains needs_work in {} scope",
                row.canonical_id, row.phase_scope
            );
        }
        if row.source == "harness_native" {
            assert!(
                row.provider_function_name.is_some(),
                "{} missing provider name",
                row.canonical_id
            );
            assert!(
                row.schema_status.is_some(),
                "{} missing schema status",
                row.canonical_id
            );
        }
    }
}

fn allowed_statuses() -> BTreeSet<&'static str> {
    [
        "parity_ready",
        "harness_adapted",
        "needs_work",
        "deferred_decision",
        "harness_only",
        "external_only",
        "excluded",
    ]
    .into_iter()
    .collect()
}

fn inventory_by_id(inventory: &[ParityInventoryRow]) -> BTreeMap<&str, &ParityInventoryRow> {
    inventory
        .iter()
        .map(|row| (row.canonical_id.as_str(), row))
        .collect()
}

fn load_parity_inventory() -> Vec<ParityInventoryRow> {
    let fixture_dir = "crates/harness-tools/tests/fixtures";
    let fixture_name: &str = "tools_parity_inventory.v1.json";
    let inventory = std::fs::read_to_string(repo_path(&format!("{fixture_dir}/{fixture_name}")))
        .unwrap_or_abort();
    serde_json::from_str(&inventory).unwrap_or_abort()
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}
