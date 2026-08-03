use std::fs;
use std::path::Path;

use harness_testkit::tui_fidelity::{Scenario, ScenarioAction, Viewport};
use harness_testkit::tui_fidelity_runner::RunnerError;
use serde::Deserialize;

const REGISTRY_RELATIVE: &str =
    "crates/harness-testkit/src/tui_fidelity_scenarios/baseline/registry.json";
const REGISTRY_SCHEMA: &str = "harness.tui-fidelity.baseline-registry.v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    viewports: Vec<ViewportSpec>,
    scenarios: Vec<ScenarioSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewportSpec {
    id: String,
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioSpec {
    id: String,
    path: String,
    state: String,
    owner_source_paths: Vec<String>,
}

pub(super) fn load(scenario_id: &str, repo_root: &Path) -> Result<Scenario, RunnerError> {
    let registry_path = repo_root.join(REGISTRY_RELATIVE);
    let registry = read_registry(&registry_path)?;
    let (scenario_ref, viewport_ref) =
        scenario_id
            .rsplit_once("--")
            .ok_or_else(|| RunnerError::UnknownScenario {
                id: scenario_id.to_owned(),
            })?;
    let scenario_spec = registry
        .scenarios
        .iter()
        .find(|scenario| scenario.id == scenario_ref)
        .ok_or_else(|| RunnerError::UnknownScenario {
            id: scenario_id.to_owned(),
        })?;
    if scenario_spec.state.is_empty() || scenario_spec.owner_source_paths.is_empty() {
        return Err(RunnerError::Arguments {
            detail: format!("baseline registry owner metadata is empty for {scenario_ref}"),
        });
    }
    let viewport = registry
        .viewports
        .iter()
        .find(|viewport| viewport.id == viewport_ref)
        .map(|viewport| Viewport {
            cols: viewport.cols,
            rows: viewport.rows,
        })
        .ok_or_else(|| RunnerError::UnknownScenario {
            id: scenario_id.to_owned(),
        })?;
    let scenario_path = repo_root
        .join("crates/harness-testkit")
        .join(&scenario_spec.path);
    let input = fs::read_to_string(&scenario_path).map_err(|error| RunnerError::Io {
        path: scenario_path.clone(),
        detail: error.to_string(),
    })?;
    let mut scenario = Scenario::from_json(&input)?;
    scenario.id.0 = scenario_id.to_owned();
    scenario.viewport = viewport;
    for action in &mut scenario.actions {
        if let ScenarioAction::Resize(action) = action {
            action.viewport = viewport;
        }
    }
    for checkpoint in &mut scenario.checkpoints {
        checkpoint.frame.viewport = viewport;
    }
    Ok(scenario)
}

fn read_registry(path: &Path) -> Result<Registry, RunnerError> {
    let input = fs::read_to_string(path).map_err(|error| RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let registry: Registry =
        serde_json::from_str(&input).map_err(|error| RunnerError::Arguments {
            detail: format!("baseline registry: {error}"),
        })?;
    if registry.schema_version != REGISTRY_SCHEMA {
        return Err(RunnerError::Arguments {
            detail: format!(
                "unsupported baseline registry schema {}",
                registry.schema_version
            ),
        });
    }
    Ok(registry)
}
