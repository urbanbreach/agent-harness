mod bounded;
mod documents;
mod registry;

pub use bounded::{execute_matrix, execute_matrix_bounded};
pub use documents::read_coverage_documents;
pub use registry::{validate_scenario_registry, ScenarioRegistryReport};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use crate::tui_fidelity::Viewport;
use crate::tui_fidelity_obligation::{CaptureKey, Obligation};

const INVENTORY_SCHEMA: &str = "harness.tui-fidelity.requirement-inventory.v1";
const MANIFEST_SCHEMA: &str = "harness.tui-fidelity.coverage-manifest.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementInventory {
    pub schema_version: String,
    pub reviewed_plan_sha256: String,
    pub requirements: Vec<RequirementRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementRecord {
    pub id: String,
    pub source_line: u32,
    pub title: String,
    pub obligation: Obligation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageManifest {
    pub schema_version: String,
    pub reviewed_plan_sha256: String,
    pub inventory_sha256: String,
    pub rows: Vec<CoverageRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageRow {
    pub row_id: String,
    pub requirement_id: String,
    pub scenario_id: String,
    pub action_path: String,
    #[serde(default = "default_path_classification")]
    pub path_classification: String,
    pub viewport: Viewport,
    pub terminal_tier: String,
    pub persona: String,
    pub theme_mode: String,
    pub media_mode: String,
    pub failure_path: String,
    pub trials: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageReport {
    pub requirement_count: usize,
    pub row_count: usize,
    pub capture_key_count: usize,
    pub execution_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixExecutionReceipt {
    pub trial: u8,
    pub capture_key: String,
    pub capture_succeeded: bool,
    pub comparison_passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixRowReceipt {
    pub row_id: String,
    pub requirement_id: String,
    pub executions: Vec<MatrixExecutionReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixReceipt {
    pub schema_version: String,
    pub suite: String,
    pub status: String,
    pub capture_succeeded: bool,
    pub comparison_passed: bool,
    pub report: CoverageReport,
    pub rows: Vec<MatrixRowReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixExecution {
    pub row: CoverageRow,
    pub trial: u8,
    pub evidence_dir: PathBuf,
}

#[derive(Debug)]
pub enum MatrixError {
    Json(String),
    Invalid(String),
    Io { path: PathBuf, detail: String },
    Execution(String),
}

impl fmt::Display for MatrixError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(detail) => write!(formatter, "matrix JSON: {detail}"),
            Self::Invalid(detail) => write!(formatter, "matrix contract: {detail}"),
            Self::Io { path, detail } => {
                write!(formatter, "matrix I/O {}: {detail}", path.display())
            }
            Self::Execution(detail) => write!(formatter, "matrix execution: {detail}"),
        }
    }
}

impl std::error::Error for MatrixError {}

pub fn validate_coverage_documents(
    inventory_json: &str,
    manifest_json: &str,
) -> Result<CoverageReport, MatrixError> {
    let inventory: RequirementInventory = serde_json::from_str(inventory_json)
        .map_err(|error| MatrixError::Json(format!("inventory: {error}")))?;
    let manifest: CoverageManifest = serde_json::from_str(manifest_json)
        .map_err(|error| MatrixError::Json(format!("manifest: {error}")))?;
    validate_coverage(&inventory, &manifest)
}

fn validate_coverage(
    inventory: &RequirementInventory,
    manifest: &CoverageManifest,
) -> Result<CoverageReport, MatrixError> {
    if inventory.schema_version != INVENTORY_SCHEMA || manifest.schema_version != MANIFEST_SCHEMA {
        return Err(MatrixError::Invalid(
            "unsupported inventory or manifest schema".to_owned(),
        ));
    }
    if inventory.reviewed_plan_sha256 != manifest.reviewed_plan_sha256 {
        return Err(MatrixError::Invalid(
            "plan digest differs between inventory and manifest".to_owned(),
        ));
    }
    if inventory.reviewed_plan_sha256.len() != 64 || manifest.inventory_sha256.len() != 64 {
        return Err(MatrixError::Invalid(
            "plan and inventory digests must be SHA-256".to_owned(),
        ));
    }
    let mut defects = Vec::new();
    let mut requirements = BTreeSet::new();
    for requirement in &inventory.requirements {
        if requirement.id.trim().is_empty() {
            defects.push("requirement id is empty".to_owned());
        }
        if !requirements.insert(requirement.id.clone()) {
            defects.push(format!("duplicate requirement_id {}", requirement.id));
        }
    }
    let mut rows = BTreeSet::new();
    let mut mapped = BTreeMap::<String, usize>::new();
    let mut capture_keys = BTreeSet::new();
    for row in &manifest.rows {
        if row.row_id.trim().is_empty() {
            defects.push("row id is empty".to_owned());
        }
        if !rows.insert(row.row_id.clone()) {
            defects.push(format!("duplicate row_id {}", row.row_id));
        }
        if !requirements.contains(&row.requirement_id) {
            defects.push(format!("unmapped requirement_id {}", row.requirement_id));
        }
        match CaptureKey::from_row(row).canonical_json() {
            Ok(key) => {
                capture_keys.insert(key);
            }
            Err(error) => defects.push(format!(
                "row {} has an invalid capture key: {error}",
                row.row_id
            )),
        }
        if row.scenario_id.trim().is_empty() || row.action_path.trim().is_empty() {
            defects.push(format!(
                "row {} has an empty scenario or action path",
                row.row_id
            ));
        }
        if row.scenario_id.contains('*') || row.action_path.contains('*') {
            defects.push(format!(
                "row {} uses a grouped wildcard instead of an exact scenario and action",
                row.row_id
            ));
        }
        if [
            row.terminal_tier.as_str(),
            row.persona.as_str(),
            row.theme_mode.as_str(),
            row.media_mode.as_str(),
            row.failure_path.as_str(),
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        {
            defects.push(format!(
                "row {} has an empty coverage dimension",
                row.row_id
            ));
        }
        if row.trials != 5 {
            defects.push(format!("row {} must declare exactly 5 trials", row.row_id));
        }
        if row.requirement_id.starts_with("global.module-tier.")
            && row.path_classification != "native_path"
            && row.path_classification != "fallback_path"
        {
            defects.push(format!(
                "row {} has an invalid path classification",
                row.row_id
            ));
        }
        let count = mapped.entry(row.requirement_id.clone()).or_insert(0);
        *count += 1;
        if row.viewport.cols == 0 || row.viewport.rows == 0 {
            defects.push(format!("row {} has an empty viewport", row.row_id));
        }
    }
    for requirement in requirements {
        match mapped.get(&requirement).copied() {
            None => defects.push(format!("missing requirement_id {requirement}")),
            Some(1) => {}
            Some(count) => {
                defects.push(format!(
                    "duplicate requirement_id {requirement} ({count} rows)"
                ));
            }
        }
    }
    if !defects.is_empty() {
        return Err(MatrixError::Invalid(defects.join("; ")));
    }
    Ok(CoverageReport {
        requirement_count: inventory.requirements.len(),
        row_count: manifest.rows.len(),
        capture_key_count: capture_keys.len(),
        execution_count: manifest.rows.len() * 5,
    })
}

fn default_path_classification() -> String {
    "native_path".to_owned()
}
