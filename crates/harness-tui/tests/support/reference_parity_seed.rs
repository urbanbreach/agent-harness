//! Fresh-root fixture seeding for the strict reference-parity evidence
//! validator. Produces a truthful evidence tree (L1-L6 layers, matching
//! capture digests, freeze receipt, L3 capture metadata, embedded receipt
//! digests, and divergence receipts) so `validate_manifest_evidence` has real
//! files to hash-match in tests; the `signoff-parity` lane's fresh root is
//! populated by the capture flow instead.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::status::{non_empty_str, parse_cols_rows, resolve_declared, sha256_hex};
use crate::support::{
    divergence_receipt_path, EVIDENCE_LAYERS, FREEZE_PNG_SHA256, FREEZE_TXT_SHA256,
    REFERENCE_BINARY_SHA256,
};

/// Create fixture evidence under `root` for every claimed row so disk-backed
/// controls have a truthful tree to verify. Capture digests declared on rows
/// are rewritten to match the seeded fixture artifacts, and the freeze receipt,
/// L3 capture metadata, and L4/L6 receipts carry provenance (binary digest
/// pins, behavior_id, viewport, embedded `path`/`sha256` pairs) that matches
/// the owning rows.
pub fn seed_claimed_row_evidence(root: &Path, manifest: &mut Value) {
    let evidence_root = manifest["evidence_root"].as_str().unwrap_or("").to_owned();
    seed_freeze_receipt(&evidence_root, root, manifest);
    let notes: BTreeMap<String, String> = manifest["identity_policy"]["approved_divergence_notes"]
        .as_object()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(id, note)| Some((id.clone(), note.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let Some(rows) = manifest["rows"].as_array_mut() else {
        return;
    };
    for row in rows
        .iter_mut()
        .filter(|row| matches!(row["status"].as_str(), Some("pass" | "diverged")))
    {
        for layer in EVIDENCE_LAYERS {
            if let Some(declared) = non_empty_str(&row["evidence_paths"][layer]) {
                write_fixture(&evidence_root, root, declared);
            }
        }
        let (capture_txt, capture_png) = seed_capture_pair(&evidence_root, root, row);
        seed_capture_metadata(&evidence_root, root, row);
        seed_layer_receipts(&evidence_root, root, row, &capture_txt, &capture_png);
        if let Some(declared) = non_empty_str(&row["expected_frame_sequence"]) {
            write_fixture(&evidence_root, root, declared);
        }
        if row["status"].as_str() == Some("diverged") {
            if let Some(id) = non_empty_str(&row["deliberate_divergence_id"]) {
                if let Some(receipt_rel) =
                    notes.get(id).and_then(|note| divergence_receipt_path(note))
                {
                    write_fixture_content(&root.join(receipt_rel), b"approved divergence receipt");
                }
            }
        }
    }
}

fn seed_freeze_receipt(evidence_root: &str, root: &Path, manifest: &Value) {
    let reference = &manifest["reference"];
    let Some(receipt_path) = non_empty_str(&reference["receipt_path"]) else {
        return;
    };
    let scenario = reference["freeze_scenario"].as_str().unwrap_or("");
    let freeze_viewport = scenario
        .rsplit_once('_')
        .and_then(|(_, suffix)| parse_cols_rows(suffix));
    let receipt = json!({
        "schema_version": "harness-tui-reference-freeze-receipt-v1",
        "receipt_id": reference["receipt_id"],
        "scenario": scenario,
        "viewport": {
            "cols": freeze_viewport.map(|(cols, _)| cols).unwrap_or(0),
            "rows": freeze_viewport.map(|(_, rows)| rows).unwrap_or(0),
        },
        "global_pinned_reference": { "binary_sha256": REFERENCE_BINARY_SHA256 },
        "freeze_txt_sha256": FREEZE_TXT_SHA256,
        "freeze_png_sha256": FREEZE_PNG_SHA256,
    });
    let body = serde_json::to_string_pretty(&receipt).unwrap_or_default();
    write_fixture_content(
        &resolve_declared(evidence_root, root, receipt_path),
        body.as_bytes(),
    );
}

fn seed_capture_pair(evidence_root: &str, root: &Path, row: &mut Value) -> (String, String) {
    let mut capture_txt = String::new();
    let mut capture_png = String::new();
    for (artifact_field, digest_field, slot) in [
        (
            "expected_semantic_cell_artifact",
            "reference_txt_sha256",
            &mut capture_txt,
        ),
        (
            "expected_png_artifact",
            "reference_png_sha256",
            &mut capture_png,
        ),
    ] {
        if let Some(declared) = non_empty_str(&row[artifact_field]).map(str::to_owned) {
            write_fixture(evidence_root, root, &declared);
            let bytes =
                std::fs::read(resolve_declared(evidence_root, root, &declared)).unwrap_or_default();
            row[digest_field] = json!(sha256_hex(&bytes));
            *slot = declared;
        }
    }
    if capture_txt.is_empty() || capture_png.is_empty() {
        // Rows without declared comparison artifacts keep capture evidence
        // beside their L3 capture (for example user-approved divergences).
        if let Some(l3) = non_empty_str(&row["evidence_paths"]["L3"]).map(str::to_owned) {
            let prefix = if l3.ends_with('/') {
                l3
            } else {
                format!("{l3}/")
            };
            if capture_txt.is_empty() {
                write_fixture(evidence_root, root, &format!("{prefix}terminal.txt"));
                capture_txt = format!("{prefix}terminal.txt");
            }
            if capture_png.is_empty() {
                write_fixture(evidence_root, root, &format!("{prefix}terminal.png"));
                capture_png = format!("{prefix}terminal.png");
            }
        }
    }
    (capture_txt, capture_png)
}

fn seed_capture_metadata(evidence_root: &str, root: &Path, row: &Value) {
    let Some(l3) = non_empty_str(&row["evidence_paths"]["L3"]) else {
        return;
    };
    let metadata_path = resolve_declared(evidence_root, root, l3).join("metadata.json");
    if metadata_path.exists() {
        return;
    }
    let metadata = json!({
        "behavior_id": row["behavior_id"],
        "viewport": row["viewport"],
        "generating_command": "fixture-seed: reference_parity_seed.rs",
    });
    let body = serde_json::to_string_pretty(&metadata).unwrap_or_default();
    write_fixture_content(&metadata_path, body.as_bytes());
}

fn seed_layer_receipts(
    evidence_root: &str,
    root: &Path,
    row: &Value,
    capture_txt: &str,
    capture_png: &str,
) {
    if capture_txt.is_empty() || capture_png.is_empty() {
        return;
    }
    let txt_sha = sha256_hex(
        &std::fs::read(resolve_declared(evidence_root, root, capture_txt)).unwrap_or_default(),
    );
    let png_sha = sha256_hex(
        &std::fs::read(resolve_declared(evidence_root, root, capture_png)).unwrap_or_default(),
    );
    for layer in EVIDENCE_LAYERS {
        let Some(declared) = non_empty_str(&row["evidence_paths"][layer]) else {
            continue;
        };
        if declared.ends_with('/') || !declared.ends_with(".json") {
            continue;
        }
        let receipt = json!({
            "schema_version": "tui-parity-pixel-diff-v1",
            "reference": { "path": capture_txt, "sha256": txt_sha },
            "actual": { "path": capture_png, "sha256": png_sha },
        });
        let body = serde_json::to_string_pretty(&receipt).unwrap_or_default();
        write_fixture_content(
            &resolve_declared(evidence_root, root, declared),
            body.as_bytes(),
        );
    }
}

fn write_fixture(evidence_root: &str, root: &Path, declared: &str) {
    let full = resolve_declared(evidence_root, root, declared);
    if declared.ends_with('/') {
        std::fs::create_dir_all(&full).expect("seed fixture directory");
        return;
    }
    write_fixture_content(&full, declared.as_bytes());
}

fn write_fixture_content(full: &Path, content: &[u8]) {
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("seed fixture parent directory");
    }
    std::fs::write(full, content).expect("seed fixture file");
}
