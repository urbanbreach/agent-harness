use std::fs;
use std::path::Path;

use harness_testkit::tui_fidelity::{KeyCode, Scenario, ScenarioAction, SemanticState, Viewport};
use harness_testkit::tui_fidelity_runner::RunnerError;
use serde::Deserialize;

const REGISTRY_RELATIVE: &str =
    "crates/harness-testkit/src/tui_fidelity_scenarios/baseline/registry.json";
const REGISTRY_SCHEMA: &str = "harness.tui-fidelity.baseline-registry.v1";

#[path = "tui_fidelity_baseline_identity.rs"]
mod identity;

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
    identity::add(&mut scenario, viewport);
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
    if scenario_id.starts_with("packet3-baseline-resize--") {
        scenario.viewport = Viewport { cols: 80, rows: 24 };
    }
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
            ScenarioAction::WaitForSemanticState(mut wait)
                if scenario_id.starts_with("packet3-baseline-startup--") =>
            {
                wait.state = SemanticState::StartupReady;
                actions.push(ScenarioAction::WaitForSemanticState(wait));
            }
            action => actions.push(action),
        }
    }
    scenario.actions = actions;
    if let Some(state) = packet3_semantic_probe(scenario_id) {
        if !scenario.actions.iter().any(|action| {
            matches!(action, ScenarioAction::WaitForSemanticState(wait) if wait.state == state)
        }) {
            let at_tick = next_action_tick(&scenario);
            scenario.actions.push(ScenarioAction::WaitForSemanticState(
                harness_testkit::tui_fidelity::WaitForSemanticStateAction { at_tick, state },
            ));
        }
    } else if submits_prompt(&scenario) {
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

fn next_action_tick(scenario: &Scenario) -> harness_testkit::tui_fidelity::Tick {
    scenario
        .actions
        .last()
        .map_or(harness_testkit::tui_fidelity::Tick(1), |action| {
            harness_testkit::tui_fidelity::Tick(action.at_tick().0.saturating_add(1))
        })
}

fn packet3_semantic_probe(scenario_id: &str) -> Option<SemanticState> {
    [
        ("packet3-baseline-startup--", SemanticState::StartupReady),
        ("packet3-baseline-stream--", SemanticState::Streaming),
        ("packet3-baseline-tool--", SemanticState::ToolDone),
        (
            "packet3-baseline-permission--",
            SemanticState::PermissionOpen,
        ),
        ("packet3-baseline-question--", SemanticState::QuestionOpen),
        ("packet3-baseline-resize--", SemanticState::Resized),
    ]
    .into_iter()
    .find_map(|(prefix, state)| scenario_id.starts_with(prefix).then_some(state))
}

fn submits_prompt(scenario: &Scenario) -> bool {
    scenario.actions.iter().any(|action| {
        matches!(
            action,
            ScenarioAction::TimedKey(key) if key.key.code == KeyCode::Enter
        )
    })
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
    use super::{load, load_packet3};
    use harness_testkit::tui_fidelity::ScenarioAction;
    use std::path::Path;

    #[test]
    fn packet3_suite_uses_key_streams_for_native_interaction_receipts() {
        // arrange: the Packet 3 stream scenario resolved through the baseline registry.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // act: the Packet 3 dual-runtime variant is loaded.
        let scenario = load_packet3("packet3-baseline-stream--wide-120x40", &repo_root)
            .expect("Packet 3 baseline");

        // assert: pasted fixture text is expressed as typed keys and terminal replies remain typed separately.
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

    #[test]
    fn packet3_composer_only_scenarios_do_not_wait_for_recovery_text() {
        // arrange: the Packet 3 scroll scenario edits a multiline composer without submitting it.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // act: the Packet 3 dual-runtime variant is loaded.
        let scenario = load_packet3("packet3-baseline-scroll--wide-120x40", &repo_root)
            .expect("Packet 3 scroll baseline");

        // assert: the runner does not require response text that the scenario cannot produce.
        assert!(!scenario
            .actions
            .iter()
            .any(|action| matches!(action, ScenarioAction::WaitForText(_))));
    }

    #[test]
    fn packet3_resize_starts_before_the_selected_viewport_transition() {
        // arrange: the Packet 3 resize scenario selects the minimum viewport.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // act: the dual-runtime variant is loaded.
        let scenario = load_packet3("packet3-baseline-resize--minimum-60x20", &repo_root)
            .expect("Packet 3 resize baseline");

        // assert: the PTY starts at 80x24 and performs a real resize to 60x20.
        assert_eq!((scenario.viewport.cols, scenario.viewport.rows), (80, 24));
        assert!(matches!(
            &scenario.actions[0],
            ScenarioAction::Resize(action)
                if (action.viewport.cols, action.viewport.rows) == (60, 20)
        ));
    }

    #[test]
    fn packet3_named_state_scenarios_end_with_typed_semantic_probes() {
        // arrange: the six baseline scenarios whose captures require named runtime states.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cases = [
            (
                "startup",
                harness_testkit::tui_fidelity::SemanticState::StartupReady,
            ),
            (
                "stream",
                harness_testkit::tui_fidelity::SemanticState::Streaming,
            ),
            (
                "tool",
                harness_testkit::tui_fidelity::SemanticState::ToolDone,
            ),
            (
                "permission",
                harness_testkit::tui_fidelity::SemanticState::PermissionOpen,
            ),
            (
                "question",
                harness_testkit::tui_fidelity::SemanticState::QuestionOpen,
            ),
            (
                "resize",
                harness_testkit::tui_fidelity::SemanticState::Resized,
            ),
        ];

        // act: each scenario is routed through the Packet 3 dual-runtime loader.
        let observed = cases.map(|(name, expected)| {
            let scenario =
                load_packet3(&format!("packet3-baseline-{name}--wide-120x40"), &repo_root)
                    .expect("Packet 3 named-state baseline");
            let state = scenario.actions.iter().find_map(|action| match action {
                ScenarioAction::WaitForSemanticState(wait) => Some(wait.state),
                _ => None,
            });
            (state, expected)
        });

        // assert: no scenario can capture by waiting only for generic frame stability.
        assert!(observed
            .into_iter()
            .all(|(state, expected)| state == Some(expected)));
    }

    #[test]
    fn packet6_composer_loads_all_required_viewports() {
        // arrange
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cases = [
            ("minimum-60x20", (60, 20)),
            ("default-80x24", (80, 24)),
            ("standard-100x30", (100, 30)),
            ("wide-120x40", (120, 40)),
            ("extra-wide-140x40", (140, 40)),
        ];

        for (label, expected) in cases {
            // act
            let scenario = load(&format!("packet6-composer--{label}"), &repo_root)
                .expect("Packet 6 composer viewport");
            // assert
            assert_eq!((scenario.viewport.cols, scenario.viewport.rows), expected);
            assert_eq!(scenario.adapters.len(), 2);
        }
    }
}
