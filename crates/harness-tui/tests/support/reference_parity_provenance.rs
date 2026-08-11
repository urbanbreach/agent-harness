//! Disk-backed fail-closed provenance controls for the reference-parity manifest.
//!
//! Extracted from [`super`] so the structural validator and the disk-backed
//! provenance verifier each stay under the file-focus line budget. All public
//! entry points are re-exported from the parent [`status`] module.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::support::{
    divergence_policy, divergence_receipt_path, validate_manifest, DivergencePolicy,
    ManifestFailure, ValidateResult, EVIDENCE_LAYERS, FREEZE_PNG_SHA256, FREEZE_TXT_SHA256,
    REFERENCE_BINARY_SHA256,
};

use super::{
    is_evidence_artifact_declaration, is_sha256_hex, non_empty_str, parse_cols_rows,
    resolve_declared, resolve_evidence_path, sha256_hex,
};

/// Evidence files older than this threshold are considered stale (Contract §5.1).
const STALENESS_THRESHOLD: Duration = Duration::from_secs(3600);

/// Disk-backed fail-closed provenance controls on top of [`validate_manifest`].
///
/// `root` is a fresh evidence root whose layout mirrors the manifest
/// `evidence_root` (the shape `signoff-parity` exports via
/// `HARNESS_TUI_PARITY_ARTIFACT_DIR`). Every claimed (`pass`/`diverged`) row
/// must have its applicable evidence files present, declared capture digests
/// must hash-match the actual artifacts, embedded receipt `path`/`sha256`
/// pairs must hash-match, the freeze receipt must match the pinned reference
/// block, capture metadata must match the owning row, and approved divergences
/// must carry their receipt file. Structural failures are reported alongside
/// disk failures.
pub fn validate_manifest_evidence(manifest: &Value, root: &Path) -> ValidateResult {
    let mut failures = Vec::new();
    if let Err(structural) = validate_manifest(manifest) {
        failures.extend(structural);
    }
    let policy = divergence_policy(manifest);
    verify_freeze_receipt(manifest, root, &mut failures);
    let backup_hashes = collect_backup_hashes(root);
    let now = SystemTime::now();
    if let Some(receipt_path) = non_empty_str(&manifest["reference"]["receipt_path"]) {
        let receipt = resolve_evidence_path(manifest, root, receipt_path);
        check_file_freshness(
            &receipt,
            receipt_path,
            "$.reference.receipt_path",
            now,
            &mut failures,
        );
    }
    let Some(rows) = manifest["rows"].as_array() else {
        return finalize(failures);
    };
    let l3_owners = claimed_l3_owners(rows);
    for (index, row) in rows.iter().enumerate() {
        let status = row["status"].as_str().unwrap_or("");
        if status != "pass" && status != "diverged" {
            continue;
        }
        let path = format!("$.rows[{index}]");
        for layer in EVIDENCE_LAYERS {
            if let Some(declared) = non_empty_str(&row["evidence_paths"][layer]) {
                if !is_evidence_artifact_declaration(manifest, declared) {
                    continue;
                }
                if !path_present(&resolve_evidence_path(manifest, root, declared), declared) {
                    failures.push(ManifestFailure::new(
                        "missing-evidence-file",
                        format!("{path}.evidence_paths.{layer}"),
                        format!(
                            "evidence layer {layer} path {declared} not found \
                             under the fresh evidence root"
                        ),
                    ));
                }
            }
        }
        for artifact_field in ["expected_semantic_cell_artifact", "expected_png_artifact"] {
            if let Some(declared) = non_empty_str(&row[artifact_field]) {
                if !resolve_evidence_path(manifest, root, declared).is_file() {
                    failures.push(ManifestFailure::new(
                        "missing-evidence-file",
                        format!("{path}.{artifact_field}"),
                        format!("artifact {declared} not found under the fresh evidence root"),
                    ));
                }
            }
        }
        verify_capture_digest(
            manifest,
            root,
            row,
            &path,
            "expected_semantic_cell_artifact",
            "reference_txt_sha256",
            &mut failures,
        );
        verify_capture_digest(
            manifest,
            root,
            row,
            &path,
            "expected_png_artifact",
            "reference_png_sha256",
            &mut failures,
        );
        if let Some(declared) = non_empty_str(&row["expected_frame_sequence"]) {
            if !resolve_evidence_path(manifest, root, declared).is_dir() {
                failures.push(ManifestFailure::new(
                    "missing-evidence-file",
                    format!("{path}.expected_frame_sequence"),
                    format!(
                        "frame sequence directory {declared} not found \
                         under the fresh evidence root"
                    ),
                ));
            }
        }
        verify_layer_embedded_digests(manifest, root, row, &path, &mut failures);
        verify_capture_provenance(manifest, root, row, &path, &l3_owners, &mut failures);
        verify_evidence_freshness(manifest, root, row, &path, now, &mut failures);
        verify_provenance_metadata(manifest, root, row, &path, &mut failures);
        verify_no_copied_artifacts(manifest, root, row, &path, &backup_hashes, &mut failures);
        if status == "diverged" {
            verify_divergence_receipt(root, row, &path, &policy, &mut failures);
        }
    }
    finalize(failures)
}

fn verify_freeze_receipt(manifest: &Value, root: &Path, failures: &mut Vec<ManifestFailure>) {
    let reference = &manifest["reference"];
    let Some(receipt_path) = non_empty_str(&reference["receipt_path"]) else {
        return;
    };
    let receipt = resolve_evidence_path(manifest, root, receipt_path);
    if !receipt.is_file() {
        failures.push(ManifestFailure::new(
            "missing-evidence-file",
            "$.reference.receipt_path",
            format!(
                "reference freeze receipt {receipt_path} not found under the fresh evidence root"
            ),
        ));
        return;
    }
    let bytes = std::fs::read(&receipt).unwrap_or_default();
    let Ok(parsed) = serde_json::from_slice::<Value>(&bytes) else {
        failures.push(ManifestFailure::new(
            "reference-block-mismatch",
            "$.reference.receipt_path",
            "reference freeze receipt is not valid JSON",
        ));
        return;
    };
    let binary_digest = parsed["global_pinned_reference"]["binary_sha256"]
        .as_str()
        .or_else(|| parsed["binary"]["sha256"].as_str())
        .or_else(|| parsed["reference_binary"]["sha256"].as_str())
        .or_else(|| parsed["binary_sha256"].as_str())
        .unwrap_or("");
    if binary_digest != REFERENCE_BINARY_SHA256 {
        failures.push(ManifestFailure::new(
            "reference-block-mismatch",
            "$.reference.receipt_path",
            "freeze receipt binary sha256 does not match the pinned reference binary digest",
        ));
    }
    let freeze_txt = parsed["freeze_txt_sha256"]
        .as_str()
        .or_else(|| parsed["ref_vs_ref"]["terminal_txt_sha256"].as_str())
        .or_else(|| parsed["artifact_terminal_txt_sha256"].as_str());
    let freeze_png = parsed["freeze_png_sha256"]
        .as_str()
        .or_else(|| parsed["ref_vs_ref"]["terminal_png_sha256"].as_str());
    for (declared, pinned, field) in [
        (freeze_txt, FREEZE_TXT_SHA256, "freeze_txt_sha256"),
        (freeze_png, FREEZE_PNG_SHA256, "freeze_png_sha256"),
    ] {
        if let Some(value) = declared {
            if value != pinned {
                failures.push(ManifestFailure::new(
                    "reference-block-mismatch",
                    "$.reference.receipt_path",
                    format!(
                        "freeze receipt {field} does not match the pinned reference freeze digest"
                    ),
                ));
            }
        }
    }
    let scenario = reference["freeze_scenario"].as_str().unwrap_or("");
    let viewport = if parsed["viewport"].is_object() {
        &parsed["viewport"]
    } else {
        &parsed["environment"]["viewport"]
    };
    if let (Some(cols), Some(rows)) = (viewport["cols"].as_u64(), viewport["rows"].as_u64()) {
        let freeze = scenario
            .rsplit_once('_')
            .and_then(|(_, suffix)| parse_cols_rows(suffix));
        if freeze != Some((cols, rows)) {
            failures.push(ManifestFailure::new(
                "reference-block-mismatch",
                "$.reference.receipt_path",
                format!(
                    "freeze receipt viewport {cols}x{rows} does not match \
                     the reference freeze scenario {scenario:?}"
                ),
            ));
        }
    }
    let claimed_scenario = parsed["scenario"]
        .as_str()
        .or_else(|| parsed["ref_vs_ref"]["scenario"].as_str())
        .or_else(|| parsed["frame"].as_str());
    if let Some(claimed) = claimed_scenario {
        if claimed != scenario {
            failures.push(ManifestFailure::new(
                "reference-block-mismatch",
                "$.reference.receipt_path",
                format!(
                    "freeze receipt scenario {claimed:?} does not match \
                     the reference freeze scenario {scenario:?}"
                ),
            ));
        }
    }
}

fn verify_layer_embedded_digests(
    manifest: &Value,
    root: &Path,
    row: &Value,
    path: &str,
    failures: &mut Vec<ManifestFailure>,
) {
    let evidence_root = manifest["evidence_root"].as_str().unwrap_or("");
    for layer in EVIDENCE_LAYERS {
        let Some(declared) = non_empty_str(&row["evidence_paths"][layer]) else {
            continue;
        };
        if declared.ends_with('/') {
            continue;
        }
        let full = resolve_evidence_path(manifest, root, declared);
        let Ok(bytes) = std::fs::read(&full) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        visit_digest_entries(&parsed, &mut |embedded_path, embedded_sha| {
            let Some(target) = resolve_embedded_capture_path(evidence_root, root, embedded_path)
            else {
                failures.push(ManifestFailure::new(
                    "missing-evidence-file",
                    format!("{path}.evidence_paths.{layer}"),
                    format!(
                        "receipt {declared} references unresolvable artifact path {embedded_path}"
                    ),
                ));
                return;
            };
            let Ok(target_bytes) = std::fs::read(&target) else {
                failures.push(ManifestFailure::new(
                    "missing-evidence-file",
                    format!("{path}.evidence_paths.{layer}"),
                    format!(
                        "receipt {declared} references artifact {embedded_path} \
                         not found under the fresh evidence root"
                    ),
                ));
                return;
            };
            if sha256_hex(&target_bytes) != embedded_sha {
                failures.push(ManifestFailure::new(
                    "stale-evidence-digest",
                    format!("{path}.evidence_paths.{layer}"),
                    format!(
                        "receipt {declared} embedded sha256 for {embedded_path} \
                         does not match the file contents"
                    ),
                ));
            }
        });
    }
}

fn visit_digest_entries(value: &Value, visit: &mut dyn FnMut(&str, &str)) {
    match value {
        Value::Object(entries) => {
            if let (Some(embedded_path), Some(embedded_sha)) = (
                entries.get("path").and_then(Value::as_str),
                entries.get("sha256").and_then(Value::as_str),
            ) {
                if !embedded_path.is_empty() && is_sha256_hex(embedded_sha) {
                    visit(embedded_path, embedded_sha);
                }
            }
            for child in entries.values() {
                visit_digest_entries(child, visit);
            }
        }
        Value::Array(items) => {
            for child in items {
                visit_digest_entries(child, visit);
            }
        }
        _ => {}
    }
}

fn resolve_embedded_capture_path(
    evidence_root: &str,
    root: &Path,
    embedded: &str,
) -> Option<PathBuf> {
    if !evidence_root.is_empty() {
        if let Some((_, suffix)) = embedded.rsplit_once(&format!("{evidence_root}/")) {
            return Some(root.join(suffix));
        }
    }
    if embedded.starts_with('/') {
        return None;
    }
    Some(resolve_declared(evidence_root, root, embedded))
}

/// Claimed rows grouped by L3 capture directory: captures legitimately back
/// more than one behavior, so capture metadata may match any owning row.
fn claimed_l3_owners(rows: &[Value]) -> BTreeMap<String, (BTreeSet<String>, BTreeSet<(u64, u64)>)> {
    let mut owners: BTreeMap<String, (BTreeSet<String>, BTreeSet<(u64, u64)>)> = BTreeMap::new();
    for row in rows {
        let status = row["status"].as_str().unwrap_or("");
        if status != "pass" && status != "diverged" {
            continue;
        }
        let Some(l3) = non_empty_str(&row["evidence_paths"]["L3"]) else {
            continue;
        };
        let behavior_id = row["behavior_id"].as_str().unwrap_or("").to_owned();
        let viewport = (
            row["viewport"]["cols"].as_u64().unwrap_or(0),
            row["viewport"]["rows"].as_u64().unwrap_or(0),
        );
        let entry = owners.entry(l3.to_owned()).or_default();
        entry.0.insert(behavior_id);
        entry.1.insert(viewport);
    }
    owners
}

fn verify_capture_provenance(
    manifest: &Value,
    root: &Path,
    row: &Value,
    path: &str,
    l3_owners: &BTreeMap<String, (BTreeSet<String>, BTreeSet<(u64, u64)>)>,
    failures: &mut Vec<ManifestFailure>,
) {
    let Some(l3) = non_empty_str(&row["evidence_paths"]["L3"]) else {
        return;
    };
    let metadata_path = resolve_evidence_path(manifest, root, l3).join("metadata.json");
    let Ok(bytes) = std::fs::read(&metadata_path) else {
        return;
    };
    let Ok(metadata) = serde_json::from_slice::<Value>(&bytes) else {
        return;
    };
    let empty_owners: (BTreeSet<String>, BTreeSet<(u64, u64)>) = (BTreeSet::new(), BTreeSet::new());
    let allowed = l3_owners.get(l3).unwrap_or(&empty_owners);
    if let Some(claimed) = metadata["behavior_id"].as_str() {
        if !allowed.0.contains(claimed) {
            failures.push(ManifestFailure::new(
                "copied-evidence-artifact",
                format!("{path}.evidence_paths.L3"),
                format!(
                    "L3 metadata behavior_id {claimed:?} is not captured by \
                     any row that owns L3 directory {l3}"
                ),
            ));
        }
    }
    if let (Some(cols), Some(rows)) = (
        metadata["viewport"]["cols"].as_u64(),
        metadata["viewport"]["rows"].as_u64(),
    ) {
        if !allowed.1.contains(&(cols, rows)) {
            failures.push(ManifestFailure::new(
                "copied-evidence-artifact",
                format!("{path}.evidence_paths.L3"),
                format!(
                    "L3 metadata viewport {cols}x{rows} is not captured by \
                     any row that owns L3 directory {l3}"
                ),
            ));
        }
    }
}

fn verify_capture_digest(
    manifest: &Value,
    root: &Path,
    row: &Value,
    path: &str,
    artifact_field: &str,
    digest_field: &str,
    failures: &mut Vec<ManifestFailure>,
) {
    let Some(declared) = non_empty_str(&row[digest_field]) else {
        return;
    };
    let Some(artifact) = non_empty_str(&row[artifact_field]) else {
        return;
    };
    let Ok(bytes) = std::fs::read(resolve_evidence_path(manifest, root, artifact)) else {
        return;
    };
    let actual = sha256_hex(&bytes);
    if actual != declared {
        failures.push(ManifestFailure::new(
            "stale-evidence-digest",
            format!("{path}.{digest_field}"),
            format!("{digest_field} does not match the sha256 of {artifact}"),
        ));
    }
}

fn verify_divergence_receipt(
    root: &Path,
    row: &Value,
    path: &str,
    policy: &DivergencePolicy<'_>,
    failures: &mut Vec<ManifestFailure>,
) {
    let Some(id) = non_empty_str(&row["deliberate_divergence_id"]) else {
        return;
    };
    let Some(receipt_rel) = policy
        .notes
        .get(id)
        .copied()
        .and_then(divergence_receipt_path)
    else {
        return;
    };
    let receipt = root.join(receipt_rel);
    if !receipt.is_file() {
        failures.push(ManifestFailure::new(
            "missing-divergence-receipt",
            format!("{path}.deliberate_divergence_id"),
            format!(
                "divergence receipt {} not found under the fresh evidence root",
                receipt.display()
            ),
        ));
    }
}

fn path_present(full: &Path, declared: &str) -> bool {
    if declared.ends_with('/') {
        full.is_dir()
    } else {
        full.is_file()
    }
}

fn check_file_freshness(
    full: &Path,
    declared: &str,
    failure_path: &str,
    now: SystemTime,
    failures: &mut Vec<ManifestFailure>,
) {
    let Ok(metadata) = std::fs::metadata(full) else {
        return;
    };
    let Ok(modified) = metadata.modified() else {
        return;
    };
    if now
        .duration_since(modified)
        .is_ok_and(|elapsed| elapsed > STALENESS_THRESHOLD)
    {
        failures.push(ManifestFailure::new(
            "stale-evidence-timestamp",
            failure_path,
            format!("evidence file {declared} modification time exceeds the 1-hour run window"),
        ));
    }
}

fn verify_evidence_freshness(
    manifest: &Value,
    root: &Path,
    row: &Value,
    path: &str,
    now: SystemTime,
    failures: &mut Vec<ManifestFailure>,
) {
    let evidence_root = manifest["evidence_root"].as_str().unwrap_or("");
    for layer in EVIDENCE_LAYERS {
        let Some(declared) = non_empty_str(&row["evidence_paths"][layer]) else {
            continue;
        };
        if !evidence_root.is_empty() && !declared.starts_with(evidence_root) {
            continue;
        }
        if declared.ends_with('/') {
            continue;
        }
        let full = resolve_evidence_path(manifest, root, declared);
        check_file_freshness(
            &full,
            declared,
            &format!("{path}.evidence_paths.{layer}"),
            now,
            failures,
        );
    }
    for artifact_field in ["expected_semantic_cell_artifact", "expected_png_artifact"] {
        let Some(declared) = non_empty_str(&row[artifact_field]) else {
            continue;
        };
        let full = resolve_evidence_path(manifest, root, declared);
        check_file_freshness(
            &full,
            declared,
            &format!("{path}.{artifact_field}"),
            now,
            failures,
        );
    }
}

fn verify_provenance_metadata(
    manifest: &Value,
    root: &Path,
    row: &Value,
    path: &str,
    failures: &mut Vec<ManifestFailure>,
) {
    let Some(l3) = non_empty_str(&row["evidence_paths"]["L3"]) else {
        return;
    };
    let metadata_path = resolve_evidence_path(manifest, root, l3).join("metadata.json");
    let Ok(bytes) = std::fs::read(&metadata_path) else {
        return;
    };
    let Ok(metadata) = serde_json::from_slice::<Value>(&bytes) else {
        return;
    };
    let generating_command = metadata["generating_command"].as_str().unwrap_or("");
    if generating_command.is_empty() {
        failures.push(ManifestFailure::new(
            "missing-provenance-metadata",
            format!("{path}.evidence_paths.L3"),
            "L3 capture metadata.json must record the generating_command that produced the evidence",
        ));
    }
}

fn collect_backup_hashes(root: &Path) -> BTreeSet<String> {
    let mut hashes = BTreeSet::new();
    let backup_dir = root.join("artifacts_bak");
    if backup_dir.is_dir() {
        collect_file_hashes(&backup_dir, &mut hashes);
    }
    hashes
}

fn collect_file_hashes(dir: &Path, hashes: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_file_hashes(&entry_path, hashes);
        } else if let Ok(bytes) = std::fs::read(&entry_path) {
            hashes.insert(sha256_hex(&bytes));
        }
    }
}

fn verify_no_copied_artifacts(
    manifest: &Value,
    root: &Path,
    row: &Value,
    path: &str,
    backup_hashes: &BTreeSet<String>,
    failures: &mut Vec<ManifestFailure>,
) {
    if backup_hashes.is_empty() {
        return;
    }
    let evidence_root = manifest["evidence_root"].as_str().unwrap_or("");
    for layer in EVIDENCE_LAYERS {
        let Some(declared) = non_empty_str(&row["evidence_paths"][layer]) else {
            continue;
        };
        if !evidence_root.is_empty() && !declared.starts_with(evidence_root) {
            continue;
        }
        if declared.ends_with('/') {
            continue;
        }
        let full = resolve_evidence_path(manifest, root, declared);
        if let Ok(bytes) = std::fs::read(&full) {
            if backup_hashes.contains(&sha256_hex(&bytes)) {
                failures.push(ManifestFailure::new(
                    "copied-evidence-backup",
                    format!("{path}.evidence_paths.{layer}"),
                    format!(
                        "evidence file {declared} content is identical to a file in artifacts_bak/"
                    ),
                ));
            }
        }
    }
    for artifact_field in ["expected_semantic_cell_artifact", "expected_png_artifact"] {
        let Some(declared) = non_empty_str(&row[artifact_field]) else {
            continue;
        };
        let full = resolve_evidence_path(manifest, root, declared);
        if let Ok(bytes) = std::fs::read(&full) {
            if backup_hashes.contains(&sha256_hex(&bytes)) {
                failures.push(ManifestFailure::new(
                    "copied-evidence-backup",
                    format!("{path}.{artifact_field}"),
                    format!("artifact {declared} content is identical to a file in artifacts_bak/"),
                ));
            }
        }
    }
}

fn finalize(failures: Vec<ManifestFailure>) -> ValidateResult {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}
