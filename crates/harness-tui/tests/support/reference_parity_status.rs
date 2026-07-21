//! Fail-closed status rules for the reference-parity manifest (Packets 1.1/1.3).
//!
//! The structural rules are wired into [`crate::support::validate_manifest`];
//! [`derive_status`] derives the truthful status a row's owners and evidence
//! support; [`validate_manifest_evidence`] adds disk-backed provenance controls
//! against a fresh evidence root whose layout mirrors the manifest
//! `evidence_root`: declared evidence files must exist, capture digests and
//! embedded receipt digests must hash-match, the freeze receipt must match the
//! pinned reference block, and capture metadata must match the owning row.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::Digest as _;

use crate::support::{
    divergence_policy, divergence_receipt_path, validate_manifest, DivergencePolicy,
    ManifestFailure, ValidateResult, EVIDENCE_LAYERS, FREEZE_PNG_SHA256, FREEZE_TXT_SHA256,
    OWNER_KEYS, REFERENCE_BINARY_SHA256,
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

/// Resolve a manifest-declared evidence path under the fresh evidence `root`.
///
/// Lane evidence roots mirror the manifest `evidence_root` content layout, so
/// workspace-relative declarations are rebased below `root`; declarations that
/// do not carry the `evidence_root` prefix resolve unchanged.
pub fn resolve_evidence_path(manifest: &Value, root: &Path, declared: &str) -> PathBuf {
    let evidence_root = manifest["evidence_root"].as_str().unwrap_or("");
    if !evidence_root.is_empty() {
        if let Some(rest) = declared.strip_prefix(evidence_root) {
            if rest.is_empty() || rest == "/" {
                return root.to_path_buf();
            }
            if let Some(stripped) = rest.strip_prefix('/') {
                return root.join(stripped);
            }
        }
    }
    root.join(declared)
}

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
        if status == "diverged" {
            verify_divergence_receipt(root, row, &path, &policy, &mut failures);
        }
    }
    finalize(failures)
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

/// Rebase a declared evidence path under `root` using the `evidence_root`
/// prefix shared by [`crate::reference_parity_evidence_test`] fixtures.
pub fn resolve_declared(evidence_root: &str, root: &Path, declared: &str) -> PathBuf {
    if !evidence_root.is_empty() {
        if let Some(rest) = declared.strip_prefix(evidence_root) {
            if rest.is_empty() || rest == "/" {
                return root.to_path_buf();
            }
            if let Some(stripped) = rest.strip_prefix('/') {
                return root.join(stripped);
            }
        }
    }
    root.join(declared)
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
        .unwrap_or("");
    if binary_digest != REFERENCE_BINARY_SHA256 {
        failures.push(ManifestFailure::new(
            "reference-block-mismatch",
            "$.reference.receipt_path",
            "freeze receipt binary sha256 does not match the pinned reference binary digest",
        ));
    }
    for (field, pinned) in [
        ("freeze_txt_sha256", FREEZE_TXT_SHA256),
        ("freeze_png_sha256", FREEZE_PNG_SHA256),
    ] {
        if let Some(declared) = parsed[field].as_str() {
            if declared != pinned {
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
    if let (Some(cols), Some(rows)) = (
        parsed["viewport"]["cols"].as_u64(),
        parsed["viewport"]["rows"].as_u64(),
    ) {
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
    if let Some(claimed) = parsed["scenario"].as_str() {
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

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Shared helper: strings that are present and non-empty.
pub fn non_empty_str(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

fn path_present(full: &Path, declared: &str) -> bool {
    if declared.ends_with('/') {
        full.is_dir()
    } else {
        full.is_file()
    }
}

/// Shared helper: lowercase hex SHA-256 of raw bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
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
