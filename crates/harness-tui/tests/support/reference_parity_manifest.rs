//! Validator for docs/tui-reference-parity-manifest.v1.json (§4.2 / §9).
// allow: SIZE_OK — single-file schema validator (row field matrix + first-slice/scaffold rules + gate/status allowlists)

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

pub const SCHEMA_VERSION: &str = "harness-tui-reference-parity-manifest-v1";
pub const REFERENCE_BINARY_SHA256: &str =
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
pub const REFERENCE_RECEIPT_PATH: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/reference-freeze.receipt.json";
pub const FREEZE_TXT_SHA256: &str =
    "1a5f24dc9be953df160e8d2bcb661f6f2d8dc7845021c3153cd415ab3889ca58";
pub const FREEZE_PNG_SHA256: &str =
    "0830427651ae47645ea3ea49b532ef7ea29a69c3140f140d7df201f5093d6016";

pub const STATUS_VALUES: &[&str] = &["incomplete", "blocked", "pass", "diverged"];
pub const ACCEPTANCE_GATES: &[&str] = &[
    "A-MANIFEST",
    "A-REFERENCE",
    "A-STATE",
    "A-CELLS",
    "A-PIXELS",
    "A-TRACE",
    "A-TIMING",
    "A-PTY",
    "A-INVARIANTS",
    "A-COVERAGE",
    "A-REVIEW",
    "A-NO-RESKIN",
];

pub const FIRST_SLICE_IDS: &[&str] = &[
    "P0-START-01",
    "P0-START-02",
    "P0-START-03",
    "P0-COMP-01",
    "P0-KEY-01",
];

pub const REQUIRED_SCAFFOLD_IDS: &[&str] = &[
    "SHELL-IDLE",
    "SHELL-STREAM",
    "SHELL-PERM",
    "SHELL-QUESTION",
    "SHELL-CANCEL",
    "SHELL-FAIL",
    "SHELL-RECOVER",
    "SHELL-COMPLETE",
    "SHELL-SCROLL",
    "TX-USER",
    "TX-ASSISTANT",
    "TX-TOOL",
    "TX-DIFF",
    "OVL-PALETTE",
    "OVL-SESSION",
    "OVL-HELP",
    "OVL-PERM",
    "OVL-QUESTION",
    "RESP-120x50",
    "RESP-120x40",
    "RESP-100x30",
    "RESP-80x24",
    "RESP-79x24",
    "RESP-60x20",
    "RESP-WIDE",
];

const OWNER_KEYS: &[&str] = &[
    "fixture",
    "state_interaction_test",
    "render_test",
    "pty_test",
    "differential_evaluator",
];

const ROW_REQUIRED_FIELDS: &[&str] = &[
    "behavior_id",
    "requirement_id",
    "priority",
    "surface",
    "state",
    "reference_binary_digest",
    "reference_receipt_id",
    "reference_receipt_path",
    "preconditions",
    "seeded_inputs",
    "viewport",
    "terminal_environment",
    "input_sequence",
    "capture_checkpoints",
    "expected_focus_owner",
    "expected_cursor_state",
    "expected_scroll_and_selection_state",
    "expected_overlays_and_z_order",
    "expected_semantic_cell_artifact",
    "expected_png_artifact",
    "expected_frame_sequence",
    "owners",
    "identity_substitution",
    "acceptance_gate_ids",
    "status",
];

const FIRST_SLICE_EXTRA_FIELDS: &[&str] = &["evidence_paths", "notes", "slice"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFailure {
    pub control: String,
    pub path: String,
    pub message: String,
}

impl ManifestFailure {
    pub fn new(
        control: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            control: control.into(),
            path: path.into(),
            message: message.into(),
        }
    }
}

pub type ValidateResult = Result<(), Vec<ManifestFailure>>;

pub fn validate_manifest(manifest: &Value) -> ValidateResult {
    let mut failures = Vec::new();

    if manifest["schema_version"].as_str() != Some(SCHEMA_VERSION) {
        failures.push(ManifestFailure::new(
            "schema-version",
            "$.schema_version",
            format!(
                "expected {SCHEMA_VERSION}, got {:?}",
                manifest["schema_version"]
            ),
        ));
    }

    let reference = &manifest["reference"];
    if reference["binary_sha256"].as_str() != Some(REFERENCE_BINARY_SHA256) {
        failures.push(ManifestFailure::new(
            "reference-binary",
            "$.reference.binary_sha256",
            "pinned reference binary sha256 mismatch",
        ));
    }
    if reference["receipt_path"].as_str() != Some(REFERENCE_RECEIPT_PATH) {
        failures.push(ManifestFailure::new(
            "reference-receipt",
            "$.reference.receipt_path",
            "pinned reference receipt path mismatch",
        ));
    }
    if reference["freeze_txt_sha256"].as_str() != Some(FREEZE_TXT_SHA256) {
        failures.push(ManifestFailure::new(
            "reference-freeze-txt",
            "$.reference.freeze_txt_sha256",
            "startup freeze terminal.txt sha256 mismatch",
        ));
    }
    if reference["freeze_png_sha256"].as_str() != Some(FREEZE_PNG_SHA256) {
        failures.push(ManifestFailure::new(
            "reference-freeze-png",
            "$.reference.freeze_png_sha256",
            "startup freeze terminal.png sha256 mismatch",
        ));
    }

    let rejected = manifest["identity_policy"]["rejected_divergences"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    if !rejected.contains("DIV-004") {
        failures.push(ManifestFailure::new(
            "div-004-rejected",
            "$.identity_policy.rejected_divergences",
            "DIV-004 compose-first divergence must be explicitly rejected; welcome panel is required",
        ));
    }

    let status_allow = STATUS_VALUES.iter().copied().collect::<BTreeSet<_>>();
    let gate_allow = ACCEPTANCE_GATES.iter().copied().collect::<BTreeSet<_>>();

    let Some(rows) = manifest["rows"].as_array() else {
        failures.push(ManifestFailure::new(
            "missing-rows",
            "$.rows",
            "rows must be a non-empty array",
        ));
        return Err(failures);
    };
    if rows.is_empty() {
        failures.push(ManifestFailure::new(
            "empty-rows",
            "$.rows",
            "rows must not be empty",
        ));
        return Err(failures);
    }

    let mut seen_ids = BTreeMap::<String, usize>::new();
    for (index, row) in rows.iter().enumerate() {
        let path = format!("$.rows[{index}]");
        validate_row(row, &path, &status_allow, &gate_allow, &mut failures);
        if let Some(behavior_id) = row["behavior_id"].as_str() {
            if let Some(first_index) = seen_ids.insert(behavior_id.to_owned(), index) {
                failures.push(ManifestFailure::new(
                    "duplicate-id",
                    path,
                    format!("duplicate behavior_id {behavior_id} (first at index {first_index})"),
                ));
            }
        }
    }

    for required_id in FIRST_SLICE_IDS
        .iter()
        .chain(REQUIRED_SCAFFOLD_IDS.iter())
    {
        if !seen_ids.contains_key(*required_id) {
            failures.push(ManifestFailure::new(
                "missing-required-row",
                "$.rows",
                format!("missing required behavior_id {required_id}"),
            ));
        }
    }

    for first_id in FIRST_SLICE_IDS {
        let Some((_, index)) = seen_ids.iter().find(|(id, _)| id.as_str() == *first_id) else {
            continue;
        };
        let row = &rows[*index];
        let path = format!("$.rows[{index}]");
        validate_first_slice_row(row, &path, &mut failures);
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn validate_row(
    row: &Value,
    path: &str,
    status_allow: &BTreeSet<&str>,
    gate_allow: &BTreeSet<&str>,
    failures: &mut Vec<ManifestFailure>,
) {
    for field in ROW_REQUIRED_FIELDS {
        if row.get(*field).is_none() || row[*field].is_null() {
            failures.push(ManifestFailure::new(
                "missing-required-field",
                format!("{path}.{field}"),
                format!("required field {field} is missing or null"),
            ));
        }
    }

    let behavior_id = row["behavior_id"].as_str().unwrap_or("");
    if behavior_id.is_empty() {
        failures.push(ManifestFailure::new(
            "missing-required-field",
            format!("{path}.behavior_id"),
            "behavior_id must be a non-empty string",
        ));
    }

    for string_field in [
        "requirement_id",
        "priority",
        "surface",
        "state",
        "reference_binary_digest",
        "reference_receipt_id",
        "reference_receipt_path",
        "preconditions",
        "expected_focus_owner",
        "expected_semantic_cell_artifact",
        "expected_png_artifact",
        "expected_frame_sequence",
    ] {
        if row.get(string_field).is_some()
            && !row[string_field].is_null()
            && row[string_field].as_str().is_none()
        {
            failures.push(ManifestFailure::new(
                "invalid-field-type",
                format!("{path}.{string_field}"),
                format!("{string_field} must be a string"),
            ));
        }
    }

    if FIRST_SLICE_IDS.contains(&behavior_id) {
        for artifact_field in [
            "expected_semantic_cell_artifact",
            "expected_png_artifact",
            "expected_frame_sequence",
        ] {
            if row[artifact_field].as_str().is_none_or(str::is_empty) {
                failures.push(ManifestFailure::new(
                    "missing-required-field",
                    format!("{path}.{artifact_field}"),
                    format!("first-slice row requires non-empty {artifact_field}"),
                ));
            }
        }
    }

    match row["status"].as_str() {
        Some(status) if status_allow.contains(status) => {}
        Some(status) => failures.push(ManifestFailure::new(
            "invalid-status",
            format!("{path}.status"),
            format!("invalid status {status:?}; allowed {STATUS_VALUES:?}"),
        )),
        None if row.get("status").is_some() => failures.push(ManifestFailure::new(
            "invalid-status",
            format!("{path}.status"),
            "status must be a string",
        )),
        None => {}
    }

    if let Some(gates) = row["acceptance_gate_ids"].as_array() {
        if gates.is_empty() {
            failures.push(ManifestFailure::new(
                "invalid-gates",
                format!("{path}.acceptance_gate_ids"),
                "acceptance_gate_ids must be non-empty",
            ));
        }
        for (gate_index, gate) in gates.iter().enumerate() {
            match gate.as_str() {
                Some(gate_id) if gate_allow.contains(gate_id) => {}
                Some(gate_id) => failures.push(ManifestFailure::new(
                    "invalid-gates",
                    format!("{path}.acceptance_gate_ids[{gate_index}]"),
                    format!("invalid acceptance_gate_id {gate_id:?}"),
                )),
                None => failures.push(ManifestFailure::new(
                    "invalid-gates",
                    format!("{path}.acceptance_gate_ids[{gate_index}]"),
                    "acceptance_gate_id must be a string",
                )),
            }
        }
    } else if row.get("acceptance_gate_ids").is_some() {
        failures.push(ManifestFailure::new(
            "invalid-gates",
            format!("{path}.acceptance_gate_ids"),
            "acceptance_gate_ids must be an array",
        ));
    }

    validate_owners(row, path, failures);

    if let Some(divergence) = row.get("deliberate_divergence_id") {
        if let Some(id) = divergence.as_str() {
            if id == "DIV-004" {
                failures.push(ManifestFailure::new(
                    "div-004-rejected",
                    format!("{path}.deliberate_divergence_id"),
                    "DIV-004 is rejected; welcome panel is required",
                ));
            }
        } else if !divergence.is_null() {
            failures.push(ManifestFailure::new(
                "invalid-field-type",
                format!("{path}.deliberate_divergence_id"),
                "deliberate_divergence_id must be string or null",
            ));
        }
    }

    if let Some(identity) = row.get("identity_substitution") {
        if identity
            .get("policy")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            failures.push(ManifestFailure::new(
                "missing-required-field",
                format!("{path}.identity_substitution.policy"),
                "identity_substitution.policy is required",
            ));
        }
        if identity
            .get("fields")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            failures.push(ManifestFailure::new(
                "missing-required-field",
                format!("{path}.identity_substitution.fields"),
                "identity_substitution.fields must be a non-empty array",
            ));
        }
    }
}

fn validate_owners(row: &Value, path: &str, failures: &mut Vec<ManifestFailure>) {
    let Some(owners) = row.get("owners") else {
        return;
    };
    if !owners.is_object() {
        failures.push(ManifestFailure::new(
            "missing-owners",
            format!("{path}.owners"),
            "owners must be an object",
        ));
        return;
    }
    for owner_key in OWNER_KEYS {
        match owners.get(*owner_key).and_then(Value::as_str) {
            Some(value) if !value.is_empty() => {}
            Some(_) => failures.push(ManifestFailure::new(
                "missing-owners",
                format!("{path}.owners.{owner_key}"),
                format!("owner {owner_key} must be non-empty"),
            )),
            None => failures.push(ManifestFailure::new(
                "missing-owners",
                format!("{path}.owners.{owner_key}"),
                format!("owner {owner_key} is missing or not a string"),
            )),
        }
    }
}

fn validate_first_slice_row(row: &Value, path: &str, failures: &mut Vec<ManifestFailure>) {
    for field in FIRST_SLICE_EXTRA_FIELDS {
        if row.get(*field).is_none() || row[*field].is_null() {
            failures.push(ManifestFailure::new(
                "missing-required-field",
                format!("{path}.{field}"),
                format!("first-slice row requires {field}"),
            ));
        }
    }

    if row["slice"].as_str() != Some("first") {
        failures.push(ManifestFailure::new(
            "first-slice-incomplete",
            format!("{path}.slice"),
            "first-slice rows must set slice=\"first\"",
        ));
    }

    if row["reference_binary_digest"].as_str() != Some(REFERENCE_BINARY_SHA256) {
        failures.push(ManifestFailure::new(
            "reference-binary",
            format!("{path}.reference_binary_digest"),
            "first-slice row must pin the freeze binary digest",
        ));
    }
    if row["reference_receipt_path"].as_str() != Some(REFERENCE_RECEIPT_PATH) {
        failures.push(ManifestFailure::new(
            "reference-receipt",
            format!("{path}.reference_receipt_path"),
            "first-slice row must pin the freeze receipt path",
        ));
    }

    let evidence = &row["evidence_paths"];
    for layer in ["L1", "L2", "L3", "L4", "L5", "L6"] {
        if evidence[layer].as_str().is_none_or(str::is_empty) {
            failures.push(ManifestFailure::new(
                "missing-required-field",
                format!("{path}.evidence_paths.{layer}"),
                format!("first-slice evidence_paths.{layer} must be non-empty"),
            ));
        }
    }

    let policy = row["identity_substitution"]["policy"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !policy.contains("harness") || !policy.contains("geometry") {
        failures.push(ManifestFailure::new(
            "identity-policy",
            format!("{path}.identity_substitution.policy"),
            "identity substitution must state Harness logo/text only with geometry preserved",
        ));
    }
}
