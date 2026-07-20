//! Fail-closed validator for docs/core-subsystem-disposition.v1.json.
//!
//! Gate A-CORE-AUDIT (matrix authoring only): every affected first-party
//! product subsystem must be deliberately classified. This test does **not**
//! mark A-CORE-AUDIT as overall pass.

use harness::UnwrapOrAbort;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

mod common;

use common::repo_root;

const MATRIX_REL: &str = "docs/core-subsystem-disposition.v1.json";
const SCHEMA_VERSION: &str = "harness-core-subsystem-disposition-v1";

const ALLOWED_DISPOSITIONS: &[&str] = &[
    "replace",
    "rework",
    "retain-seam-only",
    "retain-with-reference-proof",
];

const ALLOWED_COMPARISON_STATUS: &[&str] = &["not-started", "partial", "complete"];

/// Floor inventory from contract §4.5 / task T2. Each id exactly once.
const REQUIRED_SUBSYSTEM_IDS: &[&str] = &[
    "coordinator",
    "event_schema",
    "replay_projection",
    "config_loader",
    "permissions",
    "sessions_lineage",
    "session_store",
    "worktree",
    "workspace",
    "providers",
    "native_tools",
    "mcp",
    "lsp",
    "background_tasks",
    "compaction",
    "auth",
    "redaction",
    "tui_app_state",
    "tui_render",
    "cli_handoff",
    "extensions_descriptor",
    "doctor",
    "support_export",
    // Missing product surfaces called out by §4.5 / §12 when no first-party module exists.
    "plugins",
    "acp",
    "sandbox",
];

/// Hard authority/safety seams that must stay retain-seam-only (not full behavior claims).
const REQUIRED_SEAM_ONLY: &[&str] = &[
    "coordinator",
    "event_schema",
    "replay_projection",
    "permissions",
    "redaction",
];

/// Subsystems that keep honest `partial` comparison_status even with a receipt.
const PARTIAL_COMPARISON_SUBSYSTEMS: &[&str] = &[
    "worktree",
    "sessions_lineage",
    "workspace",
    "mcp",
    "lsp",
    "background_tasks",
    "auth",
    "tui_app_state",
    "cli_handoff",
    "doctor",
    "support_export",
    "plugins",
    "acp",
    "sandbox",
];

const CORE_AUDIT_RECEIPTS_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/core-audit";

fn matrix_path() -> PathBuf {
    repo_root().join(MATRIX_REL)
}

fn load_matrix() -> Value {
    let path = matrix_path();
    assert!(
        path.is_file(),
        "missing core subsystem disposition matrix: {} (A-CORE-AUDIT inventory required)",
        path.display()
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_abort();
    serde_json::from_str(&raw).unwrap_or_abort()
}

#[allow(
    clippy::panic,
    reason = "fail-closed subsystem matrix validation helper"
)]
fn string_field<'a>(row: &'a Value, key: &str, subsystem_id: &str) -> &'a str {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("subsystem {subsystem_id}: missing or non-string field `{key}`"))
}

#[allow(
    clippy::panic,
    reason = "fail-closed subsystem matrix validation helper"
)]
fn string_array_field(row: &Value, key: &str, subsystem_id: &str) -> Vec<String> {
    let arr = row[key]
        .as_array()
        .unwrap_or_else(|| panic!("subsystem {subsystem_id}: missing or non-array field `{key}`"));
    assert!(
        !arr.is_empty(),
        "subsystem {subsystem_id}: `{key}` must be non-empty"
    );
    arr.iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| {
                    panic!("subsystem {subsystem_id}: `{key}` entries must be strings")
                })
                .to_owned()
        })
        .collect()
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn core_subsystem_disposition_matrix_is_fail_closed() {
    // arrange
    // act
    // assert
    // arrange
    let doc = load_matrix();

    assert_eq!(
        doc["schema_version"].as_str(),
        Some(SCHEMA_VERSION),
        "schema_version must be {SCHEMA_VERSION}"
    );
    assert_eq!(
        doc["document_id"].as_str(),
        Some("core-subsystem-disposition"),
        "document_id must identify the core audit matrix"
    );

    let enum_list = doc["disposition_enum"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<BTreeSet<_>>();
    for allowed in ALLOWED_DISPOSITIONS {
        assert!(
            enum_list.contains(*allowed),
            "missing disposition enum {allowed}"
        );
    }
    assert_eq!(enum_list.len(), ALLOWED_DISPOSITIONS.len());

    let meanings = doc["disposition_meanings"].as_object().unwrap_or_abort();
    for allowed in ALLOWED_DISPOSITIONS {
        assert!(
            meanings.contains_key(*allowed),
            "disposition_meanings missing {allowed}"
        );
    }

    let subsystems = doc["subsystems"].as_array().unwrap_or_abort();
    assert!(!subsystems.is_empty(), "subsystems array must not be empty");

    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut disposition_counts: BTreeMap<String, usize> = BTreeMap::new();

    // act + assert
    for (index, row) in subsystems.iter().enumerate() {
        let subsystem_id = string_field(row, "subsystem_id", &format!("index={index}"));
        assert!(
            seen.insert(subsystem_id.to_owned(), index).is_none(),
            "duplicate subsystem_id {subsystem_id}"
        );

        let disposition = string_field(row, "disposition", subsystem_id);
        assert!(
            ALLOWED_DISPOSITIONS.contains(&disposition),
            "subsystem {subsystem_id}: invalid disposition {disposition}"
        );
        *disposition_counts
            .entry(disposition.to_owned())
            .or_insert(0) += 1;

        // Core modules must not claim retain-with-reference-proof without L2–L5 visual
        // proof; this inventory deliberately forbids the claim until such evidence exists.
        assert_ne!(
            disposition, "retain-with-reference-proof",
            "subsystem {subsystem_id}: retain-with-reference-proof forbidden for core inventory without L2–L5 proof"
        );

        let comparison_status = string_field(row, "comparison_status", subsystem_id);
        assert!(
            ALLOWED_COMPARISON_STATUS.contains(&comparison_status),
            "subsystem {subsystem_id}: invalid comparison_status {comparison_status}"
        );

        let rationale = string_field(row, "rationale", subsystem_id);
        assert!(
            !rationale.trim().is_empty(),
            "subsystem {subsystem_id}: rationale must be non-empty"
        );

        let _path_globs = string_array_field(row, "path_globs", subsystem_id);
        let _invariant_owners = string_array_field(row, "invariant_owners", subsystem_id);
        let _evidence_owners = string_array_field(row, "evidence_owners", subsystem_id);
        let _hard_invariants = string_array_field(row, "hard_invariants_preserved", subsystem_id);
    }

    let required: BTreeSet<&str> = REQUIRED_SUBSYSTEM_IDS.iter().copied().collect();
    let seen_ids: BTreeSet<&str> = seen.keys().map(String::as_str).collect();

    for id in &required {
        assert!(
            seen_ids.contains(id),
            "required subsystem_id missing from matrix: {id}"
        );
    }

    // Extra rows are allowed (inventory floor, not ceiling) but every required id is once.
    for id in REQUIRED_SUBSYSTEM_IDS {
        assert_eq!(
            seen.get(*id).map(|_| 1).unwrap_or(0),
            1,
            "required subsystem_id must appear exactly once: {id}"
        );
    }

    for seam in REQUIRED_SEAM_ONLY {
        let row = subsystems
            .iter()
            .find(|r| r["subsystem_id"].as_str() == Some(*seam))
            .unwrap_or_abort();
        assert_eq!(
            row["disposition"].as_str(),
            Some("retain-seam-only"),
            "hard seam {seam} must be retain-seam-only (authority/safety boundary, not full-behavior retain)"
        );
    }

    let compaction = subsystems
        .iter()
        .find(|r| r["subsystem_id"].as_str() == Some("compaction"))
        .unwrap_or_abort();
    assert_eq!(
        compaction["disposition"].as_str(),
        Some("rework"),
        "compaction is explicitly redesignable (rework) while preserving append-only events + replay purity"
    );
    let compaction_rationale = compaction["rationale"].as_str().unwrap_or_abort();
    assert!(
        compaction_rationale
            .to_ascii_lowercase()
            .contains("append-only")
            || compaction_rationale.to_ascii_lowercase().contains("replay"),
        "compaction rationale must note append-only event truth and/or replay purity preservation"
    );

    let worktree = subsystems
        .iter()
        .find(|r| r["subsystem_id"].as_str() == Some("worktree"))
        .unwrap_or_abort();
    assert_eq!(
        worktree["disposition"].as_str(),
        Some("rework"),
        "worktree MVP create exists but lifecycle is incomplete → rework"
    );
    assert_eq!(
        worktree["comparison_status"].as_str(),
        Some("partial"),
        "worktree comparison_status must be partial"
    );

    // All 26 A-CORE-AUDIT comparison receipts: no longer not-started; on-disk receipts required.
    let receipts_dir = repo_root().join(CORE_AUDIT_RECEIPTS_REL);
    assert!(
        receipts_dir.is_dir(),
        "missing core-audit receipts directory: {}",
        receipts_dir.display()
    );
    let partial_set: BTreeSet<&str> = PARTIAL_COMPARISON_SUBSYSTEMS.iter().copied().collect();
    for id in REQUIRED_SUBSYSTEM_IDS {
        let row = subsystems
            .iter()
            .find(|r| r["subsystem_id"].as_str() == Some(*id))
            .unwrap_or_abort();
        let status = row["comparison_status"].as_str().unwrap_or_abort();
        assert_ne!(
            status, "not-started",
            "subsystem {id} must leave not-started after comparison receipt"
        );
        assert!(
            matches!(status, "partial" | "complete"),
            "subsystem {id}: comparison_status must be partial|complete, got {status}"
        );
        let receipt = receipts_dir.join(format!("{id}.md"));
        assert!(
            receipt.is_file(),
            "missing comparison receipt for {id}: {}",
            receipt.display()
        );
        let evidence = row["evidence_owners"]
            .as_array()
            .unwrap_or_abort()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();
        let receipt_rel = format!("{CORE_AUDIT_RECEIPTS_REL}/{id}.md");
        assert!(
            evidence
                .iter()
                .any(|e| e.contains(&format!("core-audit/{id}.md"))
                    || *e == receipt_rel
                    || e.ends_with(&format!("/{id}.md"))),
            "subsystem {id}: evidence_owners must reference core-audit/{id}.md receipt"
        );
        if partial_set.contains(id) {
            assert_eq!(
                status, "partial",
                "subsystem {id} must keep honest partial comparison_status"
            );
        } else {
            assert_eq!(
                status, "complete",
                "subsystem {id} comparison receipt is complete (not in partial residual set)"
            );
        }
    }

    for rework_partial in ["plugins", "acp", "sandbox"] {
        let row = subsystems
            .iter()
            .find(|r| r["subsystem_id"].as_str() == Some(rework_partial))
            .unwrap_or_abort();
        assert_eq!(
            row["disposition"].as_str(),
            Some("rework"),
            "{rework_partial} has first-party product → rework (not empty replace)"
        );
        assert_eq!(
            row["comparison_status"].as_str(),
            Some("partial"),
            "{rework_partial} must keep honest partial comparison_status"
        );
    }

    // Config separation is a hard seam; config_loader must preserve it in hard_invariants.
    let config_loader = subsystems
        .iter()
        .find(|r| r["subsystem_id"].as_str() == Some("config_loader"))
        .unwrap_or_abort();
    let config_invariants = config_loader["hard_invariants_preserved"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    assert!(
        config_invariants.iter().any(|s| {
            let lower = s.to_ascii_lowercase();
            lower.contains("runtime") && lower.contains("tui")
        }),
        "config_loader must preserve runtime/TUI config separation in hard_invariants_preserved"
    );

    // Smoke: disposition counts non-zero inventory (matrix authoring evidence for receipt).
    assert!(
        disposition_counts.values().sum::<usize>() >= REQUIRED_SUBSYSTEM_IDS.len(),
        "disposition row count below required floor"
    );
}
