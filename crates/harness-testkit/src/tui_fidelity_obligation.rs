use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::tui_fidelity::Viewport;
use crate::tui_fidelity_matrix::{CoverageManifest, CoverageRow, RequirementInventory};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationType {
    DualCapture,
    OwnerTest,
    StaticGate,
    ReviewerReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Obligation {
    DualCapture,
    OwnerTest { key: String },
    StaticGate { key: String },
    ReviewerReceipt { key: String },
}

impl Obligation {
    pub const fn obligation_type(&self) -> ObligationType {
        match self {
            Self::DualCapture => ObligationType::DualCapture,
            Self::OwnerTest { .. } => ObligationType::OwnerTest,
            Self::StaticGate { .. } => ObligationType::StaticGate,
            Self::ReviewerReceipt { .. } => ObligationType::ReviewerReceipt,
        }
    }

    pub fn non_runtime_key(&self) -> Option<&str> {
        match self {
            Self::DualCapture => None,
            Self::OwnerTest { key } | Self::StaticGate { key } | Self::ReviewerReceipt { key } => {
                Some(key)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureKey {
    pub scenario: String,
    pub action: String,
    pub viewport: Viewport,
    pub terminal_tier: String,
    pub persona: String,
    pub theme: String,
    pub media_mode: String,
    pub failure_path: String,
}

impl CaptureKey {
    pub fn from_row(row: &CoverageRow) -> Self {
        Self {
            scenario: row.scenario_id.clone(),
            action: row.action_path.clone(),
            viewport: row.viewport,
            terminal_tier: row.terminal_tier.clone(),
            persona: row.persona.clone(),
            theme: row.theme_mode.clone(),
            media_mode: row.media_mode.clone(),
            failure_path: row.failure_path.clone(),
        }
    }

    pub fn canonical_json(&self) -> Result<String, ObligationError> {
        serde_json::to_string(self).map_err(|error| ObligationError::Json(error.to_string()))
    }

    pub fn is_motion(&self) -> bool {
        self.persona == "motion-sensitive"
            || self.action.contains("motion")
            || self.scenario.contains("stream")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VerificationKey {
    DualCapture(CaptureKey),
    OwnerTest { key: String },
    StaticGate { key: String },
    ReviewerReceipt { key: String },
}

impl VerificationKey {
    pub fn stable_id(&self) -> Result<String, ObligationError> {
        serde_json::to_string(self).map_err(|error| ObligationError::Json(error.to_string()))
    }

    pub const fn obligation_type(&self) -> ObligationType {
        match self {
            Self::DualCapture(_) => ObligationType::DualCapture,
            Self::OwnerTest { .. } => ObligationType::OwnerTest,
            Self::StaticGate { .. } => ObligationType::StaticGate,
            Self::ReviewerReceipt { .. } => ObligationType::ReviewerReceipt,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeduplicatedObligations {
    pub obligation_count: usize,
    pub keys: Vec<VerificationKey>,
    pub requirements_by_key: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObligationError {
    Invalid(String),
    Json(String),
}

impl fmt::Display for ObligationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "obligation contract: {detail}"),
            Self::Json(detail) => write!(formatter, "obligation JSON: {detail}"),
        }
    }
}

impl std::error::Error for ObligationError {}

pub fn deduplicate_obligations(
    inventory: &RequirementInventory,
    manifest: &CoverageManifest,
    selected: &BTreeSet<String>,
) -> Result<DeduplicatedObligations, ObligationError> {
    let requirements = inventory
        .requirements
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let rows = manifest
        .rows
        .iter()
        .map(|row| (row.requirement_id.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    let mut keyed = BTreeMap::<String, (VerificationKey, Vec<String>)>::new();
    for requirement_id in selected {
        let requirement = requirements.get(requirement_id.as_str()).ok_or_else(|| {
            ObligationError::Invalid(format!("selected requirement {requirement_id} is absent"))
        })?;
        let key = verification_key(requirement_id, &requirement.obligation, &rows)?;
        let stable_id = key.stable_id()?;
        keyed
            .entry(stable_id)
            .and_modify(|(_, ids)| ids.push(requirement_id.clone()))
            .or_insert_with(|| (key, vec![requirement_id.clone()]));
    }
    let requirements_by_key = keyed
        .iter()
        .map(|(id, (_, requirements))| (id.clone(), requirements.clone()))
        .collect();
    Ok(DeduplicatedObligations {
        obligation_count: selected.len(),
        keys: keyed.into_values().map(|(key, _)| key).collect(),
        requirements_by_key,
    })
}

fn verification_key(
    requirement_id: &str,
    obligation: &Obligation,
    rows: &BTreeMap<&str, &CoverageRow>,
) -> Result<VerificationKey, ObligationError> {
    match obligation {
        Obligation::DualCapture => rows
            .get(requirement_id)
            .map(|row| VerificationKey::DualCapture(CaptureKey::from_row(row)))
            .ok_or_else(|| {
                ObligationError::Invalid(format!(
                    "dual_capture requirement {requirement_id} has no coverage row"
                ))
            }),
        Obligation::OwnerTest { key } => non_runtime_key(key, ObligationType::OwnerTest),
        Obligation::StaticGate { key } => non_runtime_key(key, ObligationType::StaticGate),
        Obligation::ReviewerReceipt { key } => {
            non_runtime_key(key, ObligationType::ReviewerReceipt)
        }
    }
}

fn non_runtime_key(
    key: &str,
    obligation_type: ObligationType,
) -> Result<VerificationKey, ObligationError> {
    if key.trim().is_empty() {
        return Err(ObligationError::Invalid(
            "non-runtime obligation key is empty".to_owned(),
        ));
    }
    Ok(match obligation_type {
        ObligationType::DualCapture => {
            return Err(ObligationError::Invalid(
                "dual_capture cannot use a non-runtime key".to_owned(),
            ));
        }
        ObligationType::OwnerTest => VerificationKey::OwnerTest {
            key: key.to_owned(),
        },
        ObligationType::StaticGate => VerificationKey::StaticGate {
            key: key.to_owned(),
        },
        ObligationType::ReviewerReceipt => VerificationKey::ReviewerReceipt {
            key: key.to_owned(),
        },
    })
}
