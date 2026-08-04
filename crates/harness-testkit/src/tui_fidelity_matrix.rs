use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tui_fidelity_compare::hash_bytes;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageReport {
    pub requirement_count: usize,
    pub row_count: usize,
    pub trial_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixTrialReceipt {
    pub trial: u8,
    pub capture_succeeded: bool,
    pub comparison_passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixRowReceipt {
    pub row_id: String,
    pub requirement_id: String,
    pub trials: Vec<MatrixTrialReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixReceipt {
    pub schema_version: String,
    pub suite: String,
    pub capture_succeeded: bool,
    pub comparison_passed: bool,
    pub report: CoverageReport,
    pub rows: Vec<MatrixRowReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixTrial {
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

pub fn read_coverage_documents(
    inventory_path: &Path,
    manifest_path: &Path,
) -> Result<(RequirementInventory, CoverageManifest, CoverageReport), MatrixError> {
    let inventory_json = read_text(inventory_path)?;
    let manifest_json = read_text(manifest_path)?;
    let inventory: RequirementInventory = serde_json::from_str(&inventory_json)
        .map_err(|error| MatrixError::Json(format!("inventory: {error}")))?;
    let manifest: CoverageManifest = serde_json::from_str(&manifest_json)
        .map_err(|error| MatrixError::Json(format!("manifest: {error}")))?;
    let inventory_sha256 = hash_bytes(inventory_json.as_bytes())
        .map_err(|error| MatrixError::Invalid(format!("inventory hash: {error}")))?;
    if manifest.inventory_sha256 != inventory_sha256 {
        return Err(MatrixError::Invalid(
            "manifest inventory digest does not match the inventory file".to_owned(),
        ));
    }
    let report = validate_coverage(&inventory, &manifest)?;
    Ok((inventory, manifest, report))
}

pub fn execute_matrix<F>(
    manifest: CoverageManifest,
    report: CoverageReport,
    suite: &str,
    evidence_root: &Path,
    mut execute_trial: F,
) -> Result<MatrixReceipt, MatrixError>
where
    F: FnMut(MatrixTrial) -> Result<(bool, bool, String), MatrixError>,
{
    fs::create_dir_all(evidence_root).map_err(|error| MatrixError::Io {
        path: evidence_root.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut rows = Vec::with_capacity(manifest.rows.len());
    let mut capture_succeeded = true;
    let mut comparison_passed = true;
    for row in manifest.rows {
        let mut trials = Vec::with_capacity(usize::from(row.trials));
        for trial in 1..=row.trials {
            let evidence_dir = evidence_root
                .join(&row.row_id)
                .join(format!("trial-{trial}"));
            let result = execute_trial(MatrixTrial {
                row: row.clone(),
                trial,
                evidence_dir,
            });
            let (captured, compared, detail) = match result {
                Ok(value) => value,
                Err(error) => (false, false, error.to_string()),
            };
            capture_succeeded &= captured;
            comparison_passed &= compared;
            trials.push(MatrixTrialReceipt {
                trial,
                capture_succeeded: captured,
                comparison_passed: compared,
                detail,
            });
        }
        rows.push(MatrixRowReceipt {
            row_id: row.row_id,
            requirement_id: row.requirement_id,
            trials,
        });
    }
    let receipt = MatrixReceipt {
        schema_version: "harness.tui-fidelity.matrix.v1".to_owned(),
        suite: suite.to_owned(),
        capture_succeeded,
        comparison_passed: capture_succeeded && comparison_passed,
        report,
        rows,
    };
    let receipt_path = evidence_root.join("matrix-receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| MatrixError::Json(error.to_string()))?;
    fs::write(&receipt_path, bytes).map_err(|error| MatrixError::Io {
        path: receipt_path,
        detail: error.to_string(),
    })?;
    if receipt.comparison_passed {
        Ok(receipt)
    } else {
        Err(MatrixError::Execution(
            "capture and comparison must both pass for every trial".to_owned(),
        ))
    }
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
    let mut trial_count = 0;
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
        if row.scenario_id.trim().is_empty() || row.action_path.trim().is_empty() {
            defects.push(format!(
                "row {} has an empty scenario or action path",
                row.row_id
            ));
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
        if row.trials != 5 {
            defects.push(format!("row {} must contain five trials", row.row_id));
        }
        if row.viewport.cols == 0 || row.viewport.rows == 0 {
            defects.push(format!("row {} has an empty viewport", row.row_id));
        }
        trial_count += usize::from(row.trials);
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
        trial_count,
    })
}

fn default_path_classification() -> String {
    "native_path".to_owned()
}

fn read_text(path: &Path) -> Result<String, MatrixError> {
    fs::read_to_string(path).map_err(|error| MatrixError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}
