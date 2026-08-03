#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use harness_testkit::tui_fidelity::Scenario;
use serde::Deserialize;

const REGISTRY_RELATIVE: &str = "src/tui_fidelity_scenarios/baseline/registry.json";
const EVIDENCE_RELATIVE: &str = ".omo/evidence/task-6-grok-build-tui-experiential-parity";
const REQUIRED_STATES: [&str; 22] = [
    "startup",
    "draft",
    "idle",
    "stream",
    "tool",
    "diff",
    "permission",
    "question",
    "queue",
    "cancel",
    "fail",
    "recover",
    "complete",
    "scroll",
    "modal_surfaces",
    "dashboard",
    "media",
    "themes",
    "mouse",
    "resize",
    "cjk",
    "reduced_capabilities",
];

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

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LedgerRow {
    scenario_id: String,
    viewport: Viewport,
    state: String,
    status: LedgerStatus,
    owner_source_paths: Vec<String>,
    reference_artifacts: Vec<ArtifactRef>,
    candidate_artifacts: Vec<ArtifactRef>,
    exact_diff_summary: String,
    receipt: LedgerReceipt,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LedgerStatus {
    Pass,
    Different,
    Missing,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Viewport {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ArtifactRef {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LedgerReceipt {
    schema_version: String,
    path: String,
    reference_binary: ArtifactRef,
    candidate_binary: ArtifactRef,
    reference_artifacts: Vec<ArtifactRef>,
    candidate_artifacts: Vec<ArtifactRef>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn registry() -> Registry {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(REGISTRY_RELATIVE);
    let input = fs::read_to_string(&path).expect("baseline registry exists");
    serde_json::from_str(&input).expect("baseline registry has no unknown fields")
}

fn ledger() -> Vec<LedgerRow> {
    let path = repo_root().join(EVIDENCE_RELATIVE).join("ledger.jsonl");
    let input = fs::read_to_string(&path).expect("baseline ledger exists");
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("ledger row has no unknown fields"))
        .collect()
}

fn source_path_is_owned(path: &str) -> bool {
    ["crates/", "configs/", "docs/", "scripts/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        && !path.starts_with("inspirations/")
}

#[test]
fn baseline_registry_parses_scenarios_without_contract_bypasses() {
    let registry = registry();

    assert_eq!(
        registry.schema_version,
        "harness.tui-fidelity.baseline-registry.v1"
    );
    assert_eq!(registry.viewports.len(), 7);
    assert_eq!(registry.scenarios.len(), REQUIRED_STATES.len());

    let mut states = BTreeSet::new();
    for scenario_spec in &registry.scenarios {
        assert!(
            states.insert(scenario_spec.state.as_str()),
            "duplicate state"
        );
        assert!(REQUIRED_STATES.contains(&scenario_spec.state.as_str()));
        assert!(!scenario_spec.owner_source_paths.is_empty());
        assert!(scenario_spec
            .owner_source_paths
            .iter()
            .all(|path| source_path_is_owned(path)));

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&scenario_spec.path);
        let input = fs::read_to_string(path).expect("scenario fixture exists");
        let scenario = Scenario::from_json(&input).expect("scenario satisfies strict contract");
        assert_eq!(scenario.id.0, scenario_spec.id);
        assert_eq!(scenario.adapters.len(), 2);
        assert_eq!(scenario.checkpoints.len(), 3);
    }
    assert_eq!(states.len(), REQUIRED_STATES.len());
}

#[test]
fn baseline_ledger_covers_every_state_and_viewport() {
    let registry = registry();
    let rows = ledger();
    let viewports: BTreeSet<(u16, u16)> = registry
        .viewports
        .iter()
        .map(|viewport| (viewport.cols, viewport.rows))
        .collect();
    assert_eq!(viewports.len(), 7);

    let expected: BTreeSet<(String, (u16, u16))> = registry
        .scenarios
        .iter()
        .flat_map(|scenario| {
            registry.viewports.iter().map(|viewport| {
                (
                    format!("{}--{}", scenario.id, viewport.id),
                    (viewport.cols, viewport.rows),
                )
            })
        })
        .collect();
    let actual: BTreeSet<(String, (u16, u16))> = rows
        .iter()
        .map(|row| {
            (
                row.scenario_id.clone(),
                (row.viewport.cols, row.viewport.rows),
            )
        })
        .collect();

    assert_eq!(rows.len(), expected.len());
    assert_eq!(actual, expected);
    for state in REQUIRED_STATES {
        assert!(rows.iter().any(|row| row.state == state));
        for viewport in &viewports {
            assert!(rows.iter().any(|row| {
                row.state == state && (row.viewport.cols, row.viewport.rows) == *viewport
            }));
        }
    }
}

#[test]
fn baseline_ledger_binds_harness_owners_and_exact_diffs() {
    let registry = registry();
    let owners: BTreeMap<&str, &Vec<String>> = registry
        .scenarios
        .iter()
        .map(|scenario| (scenario.id.as_str(), &scenario.owner_source_paths))
        .collect();

    for row in ledger() {
        assert!(!row.owner_source_paths.is_empty());
        assert!(row
            .owner_source_paths
            .iter()
            .all(|path| source_path_is_owned(path)));
        assert!(row
            .owner_source_paths
            .iter()
            .all(|path| repo_root().join(path).is_file()));
        let scenario_ref = row
            .scenario_id
            .rsplit_once("--")
            .map_or(row.scenario_id.as_str(), |(base, _)| base);
        let declared = owners.get(scenario_ref).expect("ledger scenario owner");
        assert_eq!(&row.owner_source_paths, *declared);
        assert!(!row.exact_diff_summary.trim().is_empty());
        assert!(Path::new(&row.receipt.path).is_file());
        assert_eq!(row.receipt.reference_binary.sha256.len(), 64);
        assert_eq!(row.receipt.candidate_binary.sha256.len(), 64);
        match row.status {
            LedgerStatus::Pass | LedgerStatus::Different => {
                assert_eq!(row.receipt.schema_version, "harness.tui-fidelity.runner.v1");
                assert!(!row.reference_artifacts.is_empty());
                assert!(!row.candidate_artifacts.is_empty());
                assert_eq!(row.reference_artifacts, row.receipt.reference_artifacts);
                assert_eq!(row.candidate_artifacts, row.receipt.candidate_artifacts);
            }
            LedgerStatus::Missing => {
                assert_eq!(
                    row.receipt.schema_version,
                    "harness.tui-fidelity.cleanup.v3"
                );
                assert!(row.reference_artifacts.is_empty());
                assert!(row.candidate_artifacts.is_empty());
            }
        }
        if row.status == LedgerStatus::Pass {
            assert_eq!(row.reference_artifacts.len(), row.candidate_artifacts.len());
            assert!(row
                .reference_artifacts
                .iter()
                .zip(&row.candidate_artifacts)
                .all(|(reference, candidate)| reference.sha256 == candidate.sha256));
        }
    }
}

#[test]
fn baseline_ledger_never_passes_without_matching_dual_runtime_artifacts() {
    for row in ledger() {
        if row.status != LedgerStatus::Pass {
            continue;
        }
        assert!(!row.reference_artifacts.is_empty());
        assert!(!row.candidate_artifacts.is_empty());
        assert_eq!(row.receipt.schema_version, "harness.tui-fidelity.runner.v1");
        assert_eq!(row.reference_artifacts, row.receipt.reference_artifacts);
        assert_eq!(row.candidate_artifacts, row.receipt.candidate_artifacts);
        assert!(row.receipt.reference_binary.sha256 != row.receipt.candidate_binary.sha256);
        assert!(row.receipt.path.ends_with("/receipt.json"));
    }
}
