use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[path = "tui_fidelity_packet6_proof.rs"]
mod proof;

use proof::{canonical_evidence_path, hex_digest, require_digest};

const INPUT_SCHEMA: &str = "harness.tui-fidelity.packet6-capability-input.v1";
const RECEIPT_SCHEMA: &str = "harness.tui-fidelity.packet6-capability-receipt.v1";

#[derive(Debug, thiserror::Error)]
pub enum Packet6CapabilityError {
    #[error("Packet 6 capability input: {0}")]
    Input(String),
    #[error("Packet 6 capability proof {path}: {detail}")]
    Proof { path: PathBuf, detail: String },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityInput {
    schema_version: String,
    authority_binary_sha256: String,
    rows: Vec<CapabilityRowInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityRowInput {
    capability: Capability,
    reference: ProcessSupport,
    harness: ProcessSupport,
}

#[derive(Clone, Copy, Debug, Deserialize, Ord, PartialOrd, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Capability {
    Truecolor,
    #[serde(rename = "indexed_256")]
    Indexed256,
    #[serde(rename = "ansi_16")]
    Ansi16,
    ReducedMotion,
    LegacyKeys,
    TmuxSsh,
    CjkEmojiWide,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessSupport {
    availability: Availability,
    #[serde(default)]
    evidence_path: Option<PathBuf>,
    #[serde(default)]
    evidence_sha256: Option<String>,
    observable: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Availability {
    Available,
    Unavailable,
}

#[derive(Serialize)]
struct CapabilityReceipt {
    schema_version: &'static str,
    authority_binary_sha256: String,
    comparison_claimed: bool,
    rows: Vec<CapabilityRowReceipt>,
}

#[derive(Serialize)]
struct CapabilityRowReceipt {
    capability: Capability,
    status: SupportStatus,
    reference: ProcessEvidence,
    harness: ProcessEvidence,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SupportStatus {
    SupportedByBoth,
    ReferenceOnly,
    HarnessOnly,
    Unsupported,
}

#[derive(Serialize)]
struct ProcessEvidence {
    availability: AvailabilityReceipt,
    evidence_path: Option<PathBuf>,
    evidence_sha256: Option<String>,
    observable: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum AvailabilityReceipt {
    Available,
    Unavailable,
}

pub fn build_capability_receipt(
    input_json: &str,
    evidence_root: &Path,
    authority_digest: &str,
) -> Result<String, Packet6CapabilityError> {
    let input: CapabilityInput = serde_json::from_str(input_json)
        .map_err(|error| Packet6CapabilityError::Input(error.to_string()))?;
    if input.schema_version != INPUT_SCHEMA {
        return Err(Packet6CapabilityError::Input(
            "unsupported schema".to_owned(),
        ));
    }
    require_digest(&input.authority_binary_sha256)?;
    if input.authority_binary_sha256 != authority_digest {
        return Err(Packet6CapabilityError::Input(
            "authority binary digest differs".to_owned(),
        ));
    }
    let capabilities = input
        .rows
        .iter()
        .map(|row| row.capability)
        .collect::<BTreeSet<_>>();
    if capabilities.len() != 7 || input.rows.len() != 7 {
        return Err(Packet6CapabilityError::Input(
            "all seven unique capability rows are required".to_owned(),
        ));
    }
    let rows = input
        .rows
        .into_iter()
        .map(|row| {
            let reference = verify_support(row.reference, evidence_root)?;
            let harness = verify_support(row.harness, evidence_root)?;
            let status = support_status(&reference, &harness);
            Ok(CapabilityRowReceipt {
                capability: row.capability,
                status,
                reference,
                harness,
            })
        })
        .collect::<Result<Vec<_>, Packet6CapabilityError>>()?;
    serde_json::to_string_pretty(&CapabilityReceipt {
        schema_version: RECEIPT_SCHEMA,
        authority_binary_sha256: input.authority_binary_sha256,
        comparison_claimed: false,
        rows,
    })
    .map_err(|error| Packet6CapabilityError::Input(error.to_string()))
}

fn verify_support(
    support: ProcessSupport,
    root: &Path,
) -> Result<ProcessEvidence, Packet6CapabilityError> {
    if support.observable.trim().is_empty() {
        return Err(Packet6CapabilityError::Input(
            "capability observable is empty".to_owned(),
        ));
    }
    match support.availability {
        Availability::Unavailable => {
            if support.evidence_path.is_some() || support.evidence_sha256.is_some() {
                return Err(Packet6CapabilityError::Input(
                    "unavailable capability must not cite process evidence".to_owned(),
                ));
            }
            Ok(ProcessEvidence {
                availability: AvailabilityReceipt::Unavailable,
                evidence_path: None,
                evidence_sha256: None,
                observable: support.observable,
            })
        }
        Availability::Available => verify_available(support, root),
    }
}

fn verify_available(
    support: ProcessSupport,
    root: &Path,
) -> Result<ProcessEvidence, Packet6CapabilityError> {
    let path = support.evidence_path.ok_or_else(|| {
        Packet6CapabilityError::Input("available capability has no evidence path".to_owned())
    })?;
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(Packet6CapabilityError::Input(
            "capability evidence path escapes its root".to_owned(),
        ));
    }
    let digest = support.evidence_sha256.ok_or_else(|| {
        Packet6CapabilityError::Input("available capability has no evidence digest".to_owned())
    })?;
    require_digest(&digest)?;
    let canonical_path = canonical_evidence_path(root, &path)?;
    let bytes = std::fs::read(&canonical_path).map_err(|error| Packet6CapabilityError::Proof {
        path: canonical_path,
        detail: error.to_string(),
    })?;
    if hex_digest(&bytes) != digest {
        return Err(Packet6CapabilityError::Proof {
            path,
            detail: "digest differs".to_owned(),
        });
    }
    Ok(ProcessEvidence {
        availability: AvailabilityReceipt::Available,
        evidence_path: Some(path),
        evidence_sha256: Some(digest),
        observable: support.observable,
    })
}

fn support_status(reference: &ProcessEvidence, harness: &ProcessEvidence) -> SupportStatus {
    match (&reference.availability, &harness.availability) {
        (AvailabilityReceipt::Available, AvailabilityReceipt::Available) => {
            SupportStatus::SupportedByBoth
        }
        (AvailabilityReceipt::Available, AvailabilityReceipt::Unavailable) => {
            SupportStatus::ReferenceOnly
        }
        (AvailabilityReceipt::Unavailable, AvailabilityReceipt::Available) => {
            SupportStatus::HarnessOnly
        }
        (AvailabilityReceipt::Unavailable, AvailabilityReceipt::Unavailable) => {
            SupportStatus::Unsupported
        }
    }
}
