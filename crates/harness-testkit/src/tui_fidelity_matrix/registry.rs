use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::{CoverageManifest, MatrixError};

const REGISTRY_SCHEMA: &str = "harness.tui-fidelity.baseline-registry.v1";
const REGISTERED_NON_ACCEPTANCE_FAMILIES: [&str; 6] = [
    "baseline-cancel",
    "baseline-fail",
    "canary-resize-wait",
    "canary-terminal-query",
    "canary-terminal-tier",
    "packet6-composer",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    viewports: Vec<RegistryViewport>,
    scenarios: Vec<RegistryScenario>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryViewport {
    id: String,
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryScenario {
    id: String,
    path: String,
    state: String,
    owner_source_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioRegistryReport {
    pub active_families: BTreeSet<String>,
    pub registered_non_acceptance_families: BTreeSet<String>,
}

pub fn validate_scenario_registry(
    registry_json: &str,
    manifest: &CoverageManifest,
) -> Result<ScenarioRegistryReport, MatrixError> {
    let registry: Registry = serde_json::from_str(registry_json)
        .map_err(|error| MatrixError::Json(format!("scenario registry: {error}")))?;
    if registry.schema_version != REGISTRY_SCHEMA {
        return Err(MatrixError::Invalid(
            "unsupported scenario registry schema".to_owned(),
        ));
    }
    let viewports = registry
        .viewports
        .iter()
        .map(|viewport| (viewport.id.as_str(), (viewport.cols, viewport.rows)))
        .collect::<BTreeMap<_, _>>();
    let registered = registry
        .scenarios
        .iter()
        .map(|scenario| scenario.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut active = BTreeSet::new();
    let mut defects = Vec::new();
    for scenario in &registry.scenarios {
        if scenario.path.trim().is_empty()
            || scenario.state.trim().is_empty()
            || scenario.owner_source_paths.is_empty()
        {
            defects.push(format!(
                "registered scenario {} has incomplete ownership",
                scenario.id
            ));
        }
    }
    for row in &manifest.rows {
        let Some((family, viewport_id)) = row.scenario_id.rsplit_once("--") else {
            defects.push(format!("row {} has an unregistered scenario", row.row_id));
            continue;
        };
        if !registered.contains(family) {
            defects.push(format!(
                "row {} names missing scenario family {family}",
                row.row_id
            ));
            continue;
        }
        active.insert(family.to_owned());
        match viewports.get(viewport_id) {
            Some((cols, rows)) if (*cols, *rows) == (row.viewport.cols, row.viewport.rows) => {}
            Some(_) => defects.push(format!(
                "row {} viewport differs from registered scenario viewport",
                row.row_id
            )),
            None => defects.push(format!(
                "row {} names missing scenario viewport {viewport_id}",
                row.row_id
            )),
        }
    }
    let inactive = registered
        .iter()
        .filter(|family| !active.contains(**family))
        .map(|family| (*family).to_owned())
        .collect::<BTreeSet<_>>();
    let expected_inactive = REGISTERED_NON_ACCEPTANCE_FAMILIES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if inactive != expected_inactive {
        defects.push(format!(
            "registered non-acceptance families differ: expected {expected_inactive:?}, found {inactive:?}"
        ));
    }
    if !defects.is_empty() {
        return Err(MatrixError::Invalid(defects.join("; ")));
    }
    Ok(ScenarioRegistryReport {
        active_families: active,
        registered_non_acceptance_families: inactive,
    })
}
