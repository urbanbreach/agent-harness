//! Fail-closed status rules for the reference-parity manifest (Packets 1.1/1.3).
//!
//! The structural rules are wired into [`crate::support::validate_manifest`];
//! [`derive_status`] derives the truthful status a row's owners and evidence
//! support; disk-backed provenance controls live in the [`provenance`]
//! submodule ([`validate_manifest_evidence`]).

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::Digest as _;

use crate::support::{
    DivergencePolicy, ManifestFailure, EVIDENCE_LAYERS, FREEZE_PNG_SHA256, FREEZE_TXT_SHA256,
    OWNER_KEYS,
};

#[path = "reference_parity_provenance.rs"]
mod provenance;

pub use provenance::validate_manifest_evidence;

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
    // Nonvisual journey rows (row_kind="journey") do not render terminal
    // output, so only L3 (actual capture) and L6 (receipt) are required.
    // L1/L2/L4/L5 are not applicable — journeys test CLI/backend behavior
    // through owner tests, not visual reference captures or pixel diffs.
    let is_nonvisual_journey = row["row_kind"].as_str() == Some("journey");
    // Terminal capability rows (row_kind="terminal_capability") prove terminal
    // mode negotiation parity (escape sequences), not visual rendering. They
    // require L1 (reference modes receipt), L2 (Harness source), and L3 (Harness
    // modes receipt) but not L4/L5/L6 (pixel diffs/masks) or visual artifacts.
    let is_terminal_capability = row["row_kind"].as_str() == Some("terminal_capability");
    let required_layers: &[&str] = if is_nonvisual_journey {
        &["L3", "L6"]
    } else if is_terminal_capability {
        &["L1", "L2", "L3"]
    } else {
        &EVIDENCE_LAYERS
    };
    for layer in required_layers {
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
    if is_nonvisual_journey || is_terminal_capability {
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

/// Shared helper: strings that are present and non-empty.
pub fn non_empty_str(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
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

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}
