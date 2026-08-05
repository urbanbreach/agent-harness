use std::fs;
use std::path::Path;

use crate::tui_fidelity_compare::hash_bytes;

use super::{
    validate_coverage, CoverageManifest, CoverageReport, MatrixError, RequirementInventory,
};

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

fn read_text(path: &Path) -> Result<String, MatrixError> {
    fs::read_to_string(path).map_err(|error| MatrixError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}
