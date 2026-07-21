//! Fail-closed status rules for the reference-parity manifest (Packet 1.1).
//!
//! The structural rules are wired into [`crate::support::validate_manifest`];
//! [`derive_status`] derives the truthful status a row's owners and evidence
//! support; [`validate_manifest_evidence`] adds disk-backed controls that
//! require declared evidence files, capture digests, and divergence receipts
//! to exist under a workspace root.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};
use sha2::Digest as _;

use crate::support::{
    divergence_policy, divergence_receipt_path, validate_manifest, DivergencePolicy,
    ManifestFailure, ValidateResult, EVIDENCE_LAYERS, FREEZE_PNG_SHA256, FREEZE_TXT_SHA256,
    OWNER_KEYS,
};

const PASS_ARTIFACT_FIELDS: [&str; 3] = [
    "expected_semantic_cell_artifact",
    "expected_png_artifact",
    "expected_frame_sequence",
];

const SHA256_FIELDS: [&str; 4] = [
    "reference_freeze_txt_sha256",
    "reference_freeze_png_sha256",
    "reference_txt_sha256",
    "reference_png_sha256",
];

/// Rows claiming completion must declare every applicable evidence layer.
///
/// `pass` rows additionally require the comparison artifact declarations.
/// A user-approved divergence waives only the exact accepted difference,
/// never the evidence layers.
pub fn validate_claimed_evidence(
    row: &Value,
    path: &str,
    status: &str,
    failures: &mut Vec<ManifestFailure>,
) {
    if status != "pass" && status != "diverged" {
        return;
    }
    for layer in EVIDENCE_LAYERS {
        if row["evidence_paths"][layer]
            .as_str()
            .is_none_or(str::is_empty)
        {
            failures.push(ManifestFailure::new(
                "missing-evidence-layer",
                format!("{path}.evidence_paths.{layer}"),
                format!("evidence layer {layer} must be non-empty for {status} rows"),
            ));
        }
    }
    if status != "pass" {
        return;
    }
    for field in PASS_ARTIFACT_FIELDS {
        if row[field].as_str().is_none_or(str::is_empty) {
            failures.push(ManifestFailure::new(
                "missing-evidence-layer",
                format!("{path}.{field}"),
                format!("{field} must be non-empty for pass rows"),
            ));
        }
    }
}

/// Declared digest fields must be well-formed and freeze digests must match
/// the pinned reference freeze values.
pub fn validate_declared_digests(row: &Value, path: &str, failures: &mut Vec<ManifestFailure>) {
    for field in SHA256_FIELDS {
        if let Some(declared) = row[field].as_str() {
            if !is_sha256_hex(declared) {
                failures.push(ManifestFailure::new(
                    "invalid-evidence-digest",
                    format!("{path}.{field}"),
                    format!("{field} must be a 64-character hex sha256"),
                ));
            }
        }
    }
    for (field, pinned) in [
        ("reference_freeze_txt_sha256", FREEZE_TXT_SHA256),
        ("reference_freeze_png_sha256", FREEZE_PNG_SHA256),
    ] {
        if let Some(declared) = row[field].as_str() {
            if declared != pinned {
                failures.push(ManifestFailure::new(
                    "stale-evidence-digest",
                    format!("{path}.{field}"),
                    format!("{field} does not match the pinned reference freeze digest"),
                ));
            }
        }
    }
}

/// Parse a `<cols>x<rows>` suffix into a positive viewport pair.
pub fn parse_cols_rows(text: &str) -> Option<(u64, u64)> {
    let (cols, rows) = text.split_once('x')?;
    let cols = cols.parse::<u64>().ok()?;
    let rows = rows.parse::<u64>().ok()?;
    (cols > 0 && rows > 0).then_some((cols, rows))
}

/// The viewport must be well-formed and consistent with the row identity and
/// the reference freeze: `RESP-<cols>x<rows>` ids pin viewport and state, and
/// welcome_panel rows joined to the reference freeze pin the freeze viewport.
pub fn validate_state_viewport(
    row: &Value,
    path: &str,
    freeze_viewport: Option<(u64, u64)>,
    failures: &mut Vec<ManifestFailure>,
) {
    let cols = row["viewport"]["cols"].as_u64();
    let rows = row["viewport"]["rows"].as_u64();
    let (Some(cols), Some(rows)) = (cols, rows) else {
        failures.push(ManifestFailure::new(
            "invalid-viewport",
            format!("{path}.viewport"),
            "viewport must be an object with integer cols and rows",
        ));
        return;
    };
    if cols == 0 || rows == 0 {
        failures.push(ManifestFailure::new(
            "invalid-viewport",
            format!("{path}.viewport"),
            "viewport cols and rows must be positive",
        ));
        return;
    }
    let behavior_id = row["behavior_id"].as_str().unwrap_or("");
    if let Some((expected_cols, expected_rows, expected_state)) =
        responsive_expectation(behavior_id)
    {
        if cols != expected_cols
            || rows != expected_rows
            || row["state"].as_str() != Some(expected_state.as_str())
        {
            failures.push(ManifestFailure::new(
                "state-viewport-mismatch",
                format!("{path}.viewport"),
                format!(
                    "behavior_id {behavior_id} requires viewport \
                     {expected_cols}x{expected_rows} and state {expected_state:?}"
                ),
            ));
        }
    }
    let welcome_freeze = row["surface"].as_str() == Some("startup")
        && row["state"].as_str() == Some("welcome_panel")
        && row["reference_receipt_id"].as_str() == Some("reference-freeze");
    if welcome_freeze {
        if let Some((freeze_cols, freeze_rows)) = freeze_viewport {
            if cols != freeze_cols || rows != freeze_rows {
                failures.push(ManifestFailure::new(
                    "state-viewport-mismatch",
                    format!("{path}.viewport"),
                    format!(
                        "welcome_panel rows joined to the reference freeze must \
                         use the freeze viewport {freeze_cols}x{freeze_rows}"
                    ),
                ));
            }
        }
    }
}

/// Derive the truthful status a row's owners and evidence support (fail-closed).
///
/// A non-empty unapproved divergence id blocks the row; missing owners or
/// missing applicable evidence/artifact declarations keep it incomplete.
pub fn derive_status(row: &Value, policy: &DivergencePolicy<'_>) -> &'static str {
    let divergence_id = row["deliberate_divergence_id"].as_str().unwrap_or("");
    if !divergence_id.is_empty() {
        return if policy.approved.contains(divergence_id) {
            "diverged"
        } else {
            "blocked"
        };
    }
    if claimed_evidence_complete(row) {
        "pass"
    } else {
        "incomplete"
    }
}

/// Disk-backed fail-closed controls on top of [`validate_manifest`].
///
/// Every claimed (`pass`/`diverged`) row must have its applicable evidence
/// paths present under `root`, declared capture digests must match the actual
/// artifact hashes, and approved divergences must carry their receipt file.
/// Structural failures are reported alongside disk failures.
pub fn validate_manifest_evidence(manifest: &Value, root: &Path) -> ValidateResult {
    let mut failures = Vec::new();
    if let Err(structural) = validate_manifest(manifest) {
        failures.extend(structural);
    }
    let evidence_root = manifest["evidence_root"].as_str().unwrap_or("");
    let policy = divergence_policy(manifest);
    let Some(rows) = manifest["rows"].as_array() else {
        return finalize(failures);
    };
    for (index, row) in rows.iter().enumerate() {
        let status = row["status"].as_str().unwrap_or("");
        if status != "pass" && status != "diverged" {
            continue;
        }
        let path = format!("$.rows[{index}]");
        for layer in EVIDENCE_LAYERS {
            if let Some(declared) = non_empty_str(&row["evidence_paths"][layer]) {
                if !fixture_present(root, declared) {
                    failures.push(ManifestFailure::new(
                        "missing-evidence-file",
                        format!("{path}.evidence_paths.{layer}"),
                        format!(
                            "evidence layer {layer} path {declared} not found \
                             under the workspace root"
                        ),
                    ));
                }
            }
        }
        for artifact_field in ["expected_semantic_cell_artifact", "expected_png_artifact"] {
            if let Some(declared) = non_empty_str(&row[artifact_field]) {
                if !root.join(declared).is_file() {
                    failures.push(ManifestFailure::new(
                        "missing-evidence-file",
                        format!("{path}.{artifact_field}"),
                        format!("artifact {declared} not found under the workspace root"),
                    ));
                }
            }
        }
        verify_capture_digest(
            root,
            row,
            &path,
            "expected_semantic_cell_artifact",
            "reference_txt_sha256",
            &mut failures,
        );
        verify_capture_digest(
            root,
            row,
            &path,
            "expected_png_artifact",
            "reference_png_sha256",
            &mut failures,
        );
        if let Some(declared) = non_empty_str(&row["expected_frame_sequence"]) {
            if !root.join(declared).is_dir() {
                failures.push(ManifestFailure::new(
                    "missing-evidence-file",
                    format!("{path}.expected_frame_sequence"),
                    format!(
                        "frame sequence directory {declared} not found \
                         under the workspace root"
                    ),
                ));
            }
        }
        if status == "diverged" {
            verify_divergence_receipt(root, evidence_root, row, &path, &policy, &mut failures);
        }
    }
    finalize(failures)
}

/// Create fixture evidence under `root` for every claimed row so disk-backed
/// controls have a truthful tree to verify. Capture digests declared on rows
/// are rewritten to match the seeded fixture artifacts.
pub fn seed_claimed_row_evidence(root: &Path, manifest: &mut Value) {
    let evidence_root = manifest["evidence_root"].as_str().unwrap_or("").to_owned();
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
                write_fixture(root, declared);
            }
        }
        for (artifact_field, digest_field) in [
            ("expected_semantic_cell_artifact", "reference_txt_sha256"),
            ("expected_png_artifact", "reference_png_sha256"),
        ] {
            if let Some(declared) = non_empty_str(&row[artifact_field]).map(str::to_owned) {
                write_fixture(root, &declared);
                let bytes = std::fs::read(root.join(&declared)).unwrap_or_default();
                row[digest_field] = json!(sha256_hex(&bytes));
            }
        }
        if let Some(declared) = non_empty_str(&row["expected_frame_sequence"]) {
            write_fixture(root, declared);
        }
        if row["status"].as_str() == Some("diverged") {
            if let Some(id) = non_empty_str(&row["deliberate_divergence_id"]) {
                if let Some(receipt_rel) =
                    notes.get(id).and_then(|note| divergence_receipt_path(note))
                {
                    let receipt = root.join(&evidence_root).join(receipt_rel);
                    write_fixture_content(&receipt, b"approved divergence receipt");
                }
            }
        }
    }
}

fn responsive_expectation(behavior_id: &str) -> Option<(u64, u64, String)> {
    let suffix = behavior_id.strip_prefix("RESP-")?;
    let (cols, rows) = parse_cols_rows(suffix)?;
    Some((cols, rows, format!("viewport_{cols}x{rows}")))
}

fn claimed_evidence_complete(row: &Value) -> bool {
    let layers_present = EVIDENCE_LAYERS.iter().all(|layer| {
        row["evidence_paths"][layer]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    });
    let artifacts_present = PASS_ARTIFACT_FIELDS
        .iter()
        .all(|field| row[field].as_str().is_some_and(|value| !value.is_empty()));
    let owners_present = OWNER_KEYS
        .iter()
        .all(|owner| matches!(row["owners"][owner].as_str(), Some(value) if !value.is_empty() && value != "pending"));
    layers_present && artifacts_present && owners_present
}

fn verify_capture_digest(
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
    let Ok(bytes) = std::fs::read(root.join(artifact)) else {
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
    evidence_root: &str,
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
    let receipt = root.join(evidence_root).join(receipt_rel);
    if !receipt.is_file() {
        failures.push(ManifestFailure::new(
            "missing-divergence-receipt",
            format!("{path}.deliberate_divergence_id"),
            format!(
                "divergence receipt {} not found under the workspace root",
                receipt.display()
            ),
        ));
    }
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn non_empty_str(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

fn fixture_present(root: &Path, declared: &str) -> bool {
    let full = root.join(declared);
    if declared.ends_with('/') {
        full.is_dir()
    } else {
        full.is_file()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = sha2::Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest.as_slice() {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn finalize(failures: Vec<ManifestFailure>) -> ValidateResult {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn write_fixture(root: &Path, declared: &str) {
    let full = root.join(declared);
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
