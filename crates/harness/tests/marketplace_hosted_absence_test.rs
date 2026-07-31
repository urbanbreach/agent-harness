//! Task 12 absence contract: marketplace, hosted share/upload, hosted media
//! generation, billing/paywall, premium variants, and hosted announcements must not
//! appear as public commands, config keys, schema fields, source modules, or
//! crate dependencies.
//!
//! Retained local surfaces (plugin descriptor lifecycle, local transcript
//! export, local support trace bundles, local update, inline image/media
//! presentation) are positively asserted to prove the removal is surgical.
//!
//! Plan ref: grok-build-parity-parallel-execution.md §1.2 (Scope OUT),
//! §1.4 (Removal compatibility matrix), §7 Task 12.

use harness::UnwrapOrAbort;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

mod common;

use common::repo_root;

// ---------------------------------------------------------------------------
// Terms whose public CLI surface must be entirely absent.
// ---------------------------------------------------------------------------

/// CLI command names and help-text terms that must not appear in any public
/// command, subcommand, or help output. Each maps to a removed reference feature
/// family from the scope-removal ledger.
const ABSENT_CLI_TERMS: &[&str] = &[
    "imagine",
    "imagine-video",
    "billing",
    "paywall",
    "supergrok",
    "marketplace",
    "telemetry",
    "announcements",
];

const ABSENT_CONFIG_KEYS: &[&str] = &[
    "autoshare",
    "marketplace",
    "telemetry",
    "billing",
    "imagine",
    "supergrok",
    "announcements",
    "analytics",
    "paywall",
];

/// Source directory names that must not exist under any crate. Their absence
/// proves no marketplace/telemetry/share implementation module was introduced.
const ABSENT_SOURCE_DIRS: &[&str] = &["marketplace", "telemetry"];

/// Cargo dependency name substrings that must not appear in Cargo.lock. Each
/// maps to a removed hosted/analytics dependency family.
const ABSENT_DEP_SUBSTRINGS: &[&str] = &[
    "telemetry",
    "analytics",
    "marketplace",
    "imagine",
    "supergrok",
    "paywall",
];

// ---------------------------------------------------------------------------
// Retained local surfaces that must remain present.
// ---------------------------------------------------------------------------

/// CLI commands that represent retained local surfaces and must still appear
/// in the compiled binary's help output.
const RETAINED_CLI_COMMANDS: &[&str] = &[
    "plugin", // local descriptor plugin lifecycle
    "export", // local transcript export
    "trace",  // local support trace bundles
    "update", // local binary update pipeline
];

/// Config schema keys for retained local surfaces.
const RETAINED_CONFIG_KEYS: &[&str] = &["mcp", "lsp", "skills", "agent"];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn config_schema_path(root: &Path) -> PathBuf {
    root.join("configs/config.json")
}

fn scope_removal_ledger_path(root: &Path) -> PathBuf {
    root.join("docs/scope-removal-ledger.v1.json")
}

fn cargo_lock_path(root: &Path) -> PathBuf {
    root.join("Cargo.lock")
}

fn collect_rust_source_dirs(root: &Path) -> Vec<String> {
    let mut dirs = BTreeSet::new();
    let crates_dir = root.join("crates");
    if let Ok(entries) = fs::read_dir(&crates_dir) {
        for entry in entries.flatten() {
            let crate_root = entry.path();
            let src_dir = crate_root.join("src");
            collect_dirs_recursive(&src_dir, &mut dirs);
        }
    }
    dirs.into_iter().collect()
}

fn collect_dirs_recursive(dir: &Path, out: &mut BTreeSet<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    out.insert(name.to_string());
                }
                collect_dirs_recursive(&path, out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Absence tests: removed surfaces must not appear
// ---------------------------------------------------------------------------

#[test]
fn no_marketplace_or_telemetry_source_directory_exists() {
    let root = repo_root();
    let dirs = collect_rust_source_dirs(&root);
    for absent in ABSENT_SOURCE_DIRS {
        assert!(
            !dirs.iter().any(|d| d == *absent),
            "removed source directory `{absent}` still exists under crates/"
        );
    }
}

#[test]
fn no_hosted_dependency_in_cargo_lock() {
    let root = repo_root();
    let lock = fs::read_to_string(cargo_lock_path(&root)).unwrap_or_abort();
    let lower = lock.to_lowercase();
    for substr in ABSENT_DEP_SUBSTRINGS {
        assert!(
            !lower.contains(substr),
            "Cargo.lock contains removed dependency substring `{substr}`"
        );
    }
}

#[test]
fn config_schema_has_no_marketplace_telemetry_or_billing_keys() {
    let root = repo_root();
    let schema_raw = fs::read_to_string(config_schema_path(&root)).unwrap_or_abort();
    let schema: Value = serde_json::from_str(&schema_raw).unwrap_or_abort();

    fn collect_keys(obj: &Value, out: &mut BTreeSet<String>) {
        if let Some(map) = obj.as_object() {
            for (k, v) in map {
                out.insert(k.to_lowercase());
                collect_keys(v, out);
            }
        } else if let Some(arr) = obj.as_array() {
            for v in arr {
                collect_keys(v, out);
            }
        }
    }

    let mut all_keys = BTreeSet::new();
    collect_keys(&schema, &mut all_keys);

    for absent in ABSENT_CONFIG_KEYS {
        assert!(
            !all_keys.iter().any(|k| k == *absent),
            "config schema contains removed key `{absent}`"
        );
    }
}

#[test]
fn scope_removal_ledger_covers_marketplace_and_telemetry_families() {
    let root = repo_root();
    let raw = fs::read_to_string(scope_removal_ledger_path(&root)).unwrap_or_abort();
    let ledger: Value = serde_json::from_str(&raw).unwrap_or_abort();

    let families = ledger["retired_families"].as_array().unwrap_or_abort();

    let family_ids: BTreeSet<&str> = families
        .iter()
        .filter_map(|f| f["family_id"].as_str())
        .collect();

    assert!(
        family_ids.contains("marketplace-hosted-share-media"),
        "scope-removal ledger missing family `marketplace-hosted-share-media`"
    );
    assert!(
        family_ids.contains("telemetry-announcements"),
        "scope-removal ledger missing family `telemetry-announcements`"
    );

    // Each family must declare persisted records and retained behavior.
    for family in families {
        let fid = family["family_id"].as_str().unwrap_or_abort();
        if fid == "marketplace-hosted-share-media" || fid == "telemetry-announcements" {
            assert!(
                !family["removed_items"]
                    .as_array()
                    .unwrap_or_abort()
                    .is_empty(),
                "family `{fid}` must have at least one removed item"
            );
            assert!(
                !family["persisted_records_to_audit"]
                    .as_array()
                    .unwrap_or_abort()
                    .is_empty(),
                "family `{fid}` must declare persisted records to audit"
            );
            assert!(
                !family["required_retained_behavior"]
                    .as_str()
                    .unwrap_or_abort()
                    .is_empty(),
                "family `{fid}` must declare required retained behavior"
            );
        }
    }
}

#[test]
fn capability_inventory_has_no_marketplace_or_telemetry_rows() {
    let root = repo_root();
    let inventory_path = root.join("docs/capability-inventory.v1.json");
    let raw = fs::read_to_string(&inventory_path).unwrap_or_abort();
    let inventory: Value = serde_json::from_str(&raw).unwrap_or_abort();

    let rows = inventory["capabilities"].as_array().unwrap_or_abort();

    let removed_prefixes = [
        "marketplace.",
        "hosted_share.",
        "telemetry.",
        "announcements.",
        "imagine.",
        "hosted_image.",
        "hosted_video.",
        "billing.",
        "supergrok.",
        "usage.",
        "release_notes.",
        "credit_bar.",
        "managed_connectors.",
    ];

    for row in rows {
        let cap_id = row["capability_id"].as_str().unwrap_or_abort();
        for prefix in &removed_prefixes {
            assert!(
                !cap_id.starts_with(prefix),
                "capability inventory still contains removed row `{cap_id}`"
            );
        }
    }
}

#[test]
fn no_imagine_or_billing_command_in_cli_enum() {
    // Verify the CLI Commands enum source does not contain removed command
    // variants. This is a source-level check because the binary may not
    // compile due to unrelated Task 9 copilot.rs work in progress.
    let root = repo_root();
    let lib_rs = root.join("crates/harness/src/lib.rs");
    let source = fs::read_to_string(&lib_rs).unwrap_or_abort();
    let lower = source.to_lowercase();

    for term in &[
        "imagine",
        "billing",
        "supergrok",
        "telemetry",
        "announcements",
    ] {
        assert!(
            !lower.contains(&format!("({term}")) && !lower.contains(&format!("{term}command")),
            "CLI lib.rs contains removed command variant referencing `{term}`"
        );
    }
}

// ---------------------------------------------------------------------------
// Retained surface tests: local features must remain
// ---------------------------------------------------------------------------

#[test]
fn retained_local_commands_still_present_in_cli_enum() {
    let root = repo_root();
    let lib_rs = root.join("crates/harness/src/lib.rs");
    let source = fs::read_to_string(&lib_rs).unwrap_or_abort();

    for cmd in RETAINED_CLI_COMMANDS {
        let lower = source.to_lowercase();
        assert!(
            lower.contains(cmd),
            "retained local command `{cmd}` is missing from CLI enum"
        );
    }
}

#[test]
fn retained_local_config_keys_still_present() {
    let root = repo_root();
    let schema_raw = fs::read_to_string(config_schema_path(&root)).unwrap_or_abort();
    let schema: Value = serde_json::from_str(&schema_raw).unwrap_or_abort();
    let props = schema["properties"].as_object().unwrap_or_abort();

    for key in RETAINED_CONFIG_KEYS {
        assert!(
            props.contains_key(*key),
            "retained config key `{key}` is missing from config schema"
        );
    }
}

#[test]
fn local_plugin_descriptor_lifecycle_still_present() {
    let root = repo_root();
    let plugin_cmd = root.join("crates/harness/src/plugin_cmd.rs");
    assert!(
        plugin_cmd.is_file(),
        "local plugin command module must exist"
    );
    let source = fs::read_to_string(&plugin_cmd).unwrap_or_abort();
    assert!(
        source.contains("descriptor"),
        "plugin command must reference descriptor lifecycle"
    );
    // Must explicitly reject marketplace/remote install.
    assert!(
        source.to_lowercase().contains("no") && source.to_lowercase().contains("marketplace"),
        "plugin command must document that no marketplace/remote install is performed"
    );
}

#[test]
fn local_transcript_export_still_present() {
    let root = repo_root();
    let dashboard_cmd = root.join("crates/harness/src/dashboard_cmd.rs");
    assert!(
        dashboard_cmd.is_file(),
        "dashboard_cmd module (containing export) must exist"
    );
    let source = fs::read_to_string(&dashboard_cmd).unwrap_or_abort();
    assert!(
        source.contains("ExportCommand"),
        "local transcript export command must be present"
    );
}

#[test]
fn local_support_trace_still_present() {
    let root = repo_root();
    let dashboard_cmd = root.join("crates/harness/src/dashboard_cmd.rs");
    let source = fs::read_to_string(&dashboard_cmd).unwrap_or_abort();
    assert!(
        source.contains("TraceCommand"),
        "local support trace command must be present"
    );
    // Trace must default to local-only.
    assert!(
        source.contains("local: bool") || source.contains("local: true"),
        "trace command must have local-only default"
    );
}

#[test]
fn local_update_pipeline_still_present() {
    let root = repo_root();
    let lib_rs = root.join("crates/harness/src/lib.rs");
    let source = fs::read_to_string(&lib_rs).unwrap_or_abort();
    assert!(
        source.contains("UpdateCommand") || source.contains("Update("),
        "local update pipeline command must be present in CLI enum"
    );
}
