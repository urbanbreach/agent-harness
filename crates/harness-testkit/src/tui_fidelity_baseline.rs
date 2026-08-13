use std::fs;
use std::path::Path;

use harness_testkit::tui_fidelity::{
    CheckpointName, IdentityScope, IdentitySubstitution, Rgb, Scenario, ScenarioAction,
    TextPlacement, TextStyle, Viewport, Wrapping,
};
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
    add_common_identity_substitutions(&mut scenario, viewport);
    Ok(scenario)
}

pub(super) fn load_packet3(scenario_id: &str, repo_root: &Path) -> Result<Scenario, RunnerError> {
    let baseline_id =
        scenario_id
            .strip_prefix("packet3-")
            .ok_or_else(|| RunnerError::UnknownScenario {
                id: scenario_id.to_owned(),
            })?;
    let mut scenario = load(baseline_id, repo_root)?;
    scenario.id.0 = scenario_id.to_owned();
    let mut actions = Vec::with_capacity(scenario.actions.len() + 2);
    for action in std::mem::take(&mut scenario.actions) {
        match action {
            ScenarioAction::Paste(paste) if !paste.text.contains('\n') => {
                actions.push(ScenarioAction::TypeText(
                    harness_testkit::tui_fidelity::TypeTextAction {
                        at_tick: paste.at_tick,
                        text: paste.text.clone(),
                        inter_byte_millis: 1,
                    },
                ));
            }
            ScenarioAction::TerminalReply(_) => {}
            action => actions.push(action),
        }
    }
    scenario.actions = actions;
    if !scenario_id.starts_with("packet3-baseline-stream--") {
        let settle_tick = scenario
            .actions
            .last()
            .map_or(harness_testkit::tui_fidelity::Tick(1), |action| {
                harness_testkit::tui_fidelity::Tick(action.at_tick().0.saturating_add(1))
            });
        scenario.actions.push(ScenarioAction::WaitForText(
            harness_testkit::tui_fidelity::WaitForTextAction {
                at_tick: settle_tick,
                text: "Packet 3 recovery complete".to_owned(),
            },
        ));
    }
    Ok(scenario)
}

fn add_common_identity_substitutions(scenario: &mut Scenario, viewport: Viewport) {
    if scenario.substitutions.is_empty() && viewport.cols >= 12 {
        for checkpoint in [
            CheckpointName::Rest,
            CheckpointName::Mid,
            CheckpointName::Settled,
        ] {
            scenario.substitutions.push(IdentitySubstitution {
                checkpoint,
                scope: IdentityScope::WorkspacePath,
                rectangle: harness_testkit::tui_fidelity::CellRect {
                    col: 2,
                    row: 1,
                    cols: viewport.cols - 2,
                    rows: 1,
                },
                source: workspace_source(viewport.cols - 2),
                target: workspace_target(viewport.cols - 2),
            });
            if viewport.rows > 26 && viewport.cols >= 51 {
                scenario.substitutions.push(IdentitySubstitution {
                    checkpoint,
                    scope: IdentityScope::ProviderName,
                    rectangle: harness_testkit::tui_fidelity::CellRect {
                        col: viewport.cols - 51,
                        row: 26,
                        cols: 46,
                        rows: 1,
                    },
                    source: provider_source(),
                    target: provider_target(),
                });
            }
        }
    }
}

fn identity_style(dim: bool) -> TextStyle {
    TextStyle {
        foreground: Rgb {
            r: 216,
            g: 216,
            b: 216,
        },
        background: Rgb {
            r: 18,
            g: 18,
            b: 18,
        },
        bold: false,
        dim,
        italic: false,
        underline: false,
        inverse: false,
    }
}

fn workspace_source(width: u16) -> TextPlacement {
    TextPlacement {
        text: "<harness-workspace>".to_owned(),
        cell_width: width,
        padding_left: 0,
        padding_right: 0,
        style: identity_style(true),
        wrapping: Wrapping::NoWrap,
    }
}

fn workspace_target(width: u16) -> TextPlacement {
    TextPlacement {
        text: IdentityScope::WorkspacePath.placeholder().to_owned(),
        cell_width: 10,
        padding_left: 0,
        padding_right: width - 10,
        style: identity_style(true),
        wrapping: Wrapping::NoWrap,
    }
}

fn provider_source() -> TextPlacement {
    TextPlacement {
        text: "GPT 5.6 Luna (CLIProxy) (low) · always-approve".to_owned(),
        cell_width: 46,
        padding_left: 0,
        padding_right: 0,
        style: identity_style(false),
        wrapping: Wrapping::NoWrap,
    }
}

fn provider_target() -> TextPlacement {
    TextPlacement {
        text: IdentityScope::ProviderName.placeholder().to_owned(),
        cell_width: 10,
        padding_left: 0,
        padding_right: 36,
        style: identity_style(false),
        wrapping: Wrapping::NoWrap,
    }
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

#[cfg(test)]
mod tests {
    use super::load_packet3;
    use harness_testkit::tui_fidelity::ScenarioAction;
    use std::path::Path;

    #[test]
    fn packet3_suite_uses_key_streams_for_native_interaction_receipts() {
        // Given: the Packet 3 stream scenario resolved through the baseline registry.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // When: the Packet 3 dual-runtime variant is loaded.
        let scenario = load_packet3("packet3-baseline-stream--wide-120x40", &repo_root)
            .expect("Packet 3 baseline");

        // Then: pasted fixture text is expressed as typed keys and terminal replies remain typed separately.
        assert!(matches!(scenario.actions[0], ScenarioAction::TypeText(_)));
        assert!(!scenario
            .actions
            .iter()
            .any(|action| matches!(action, ScenarioAction::TerminalReply(_))));
        assert!(!scenario.actions.iter().any(|action| matches!(
            action,
            ScenarioAction::WaitForText(wait) if wait.text == "Packet 3 recovery complete"
        )));
    }
}
