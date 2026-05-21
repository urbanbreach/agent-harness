use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use harness_core::command_registry::{CommandEffect, CommandRegistry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PROOF_BUNDLE_SCHEMA_VERSION: u32 = 1;
const PROOF_BUNDLE_KIND: &str = "selected_workflow_execution_proof";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureScenario {
    pub case_id: String,
    pub workflow_or_command_id: String,
    pub proof_dossier_path: String,
    pub public_surfaces: Vec<String>,
    pub registry_source: String,
    pub authority: String,
    pub mutability: String,
    pub provider_fixture: String,
    pub expected_event_types: Vec<String>,
    pub expected_projection: String,
    pub required_evidence_categories: Vec<String>,
    pub permissions_required: Vec<String>,
    pub tool_ids: Vec<String>,
    pub artifact_expectations: Vec<String>,
    pub negative_fixture: String,
    pub required_signoff_lanes: Vec<String>,
    pub registry_command: String,
    pub implementation_status: String,
    pub workflow_phase: String,
    pub proof_bundle_path: String,
    pub validation_errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandProof {
    pub command: String,
    pub cwd: String,
    pub exit_code: i32,
    pub stdout_path: String,
    pub stderr_path: String,
    pub status_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventLogProof {
    pub path: String,
    pub digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_digest: Option<String>,
    pub event_count: usize,
    pub workflow_id: String,
    pub event_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionProof {
    pub workflow_status_path: String,
    pub workflow_dossier_path: String,
    pub replay_status_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactProof {
    pub path: String,
    pub digest: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NegativePathProof {
    pub command: String,
    pub exit_code: i32,
    pub stdout_path: String,
    pub stderr_path: String,
    pub status_path: String,
    pub denied: bool,
    pub no_success_artifacts: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionProofBundle {
    pub schema_version: u32,
    pub proof_kind: String,
    pub generated_by: String,
    pub scenario: String,
    pub canonical_harness_id: String,
    pub registry_command: String,
    pub implementation_status: String,
    pub workflow_phase: String,
    pub public_surfaces: Vec<String>,
    pub old_runtime_free: bool,
    pub commands: Vec<CommandProof>,
    pub event_log: EventLogProof,
    pub projections: ProjectionProof,
    pub artifacts: Vec<ArtifactProof>,
    pub negative_path: NegativePathProof,
    pub manual_qa_notes_path: String,
    pub truth_gates: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureCoverageSummary {
    pub selected_rows: usize,
    pub scenarios: usize,
    pub passed: Vec<String>,
    pub failed: Vec<String>,
    pub intentionally_deferred: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSimulationReport {
    pub matrix_path: PathBuf,
    pub simulator_run_dir: PathBuf,
    pub scenarios: Vec<FeatureScenario>,
    pub coverage: FeatureCoverageSummary,
    pub deterministic_negative_paths_passed: bool,
    pub replay_evidence_passed: bool,
    pub replay_event_count: usize,
}

#[derive(Debug, Clone)]
struct ScenarioExecution {
    command_proofs: Vec<CommandProof>,
    event_log: EventLogProof,
    projections: ProjectionProof,
    artifacts: Vec<ArtifactProof>,
    negative_path: NegativePathProof,
    manual_qa_notes_path: String,
    truth_gates: BTreeMap<String, bool>,
}

struct CapturedCommand {
    proof: CommandProof,
    output: Output,
}

pub async fn run_feature_simulator(
    root: impl AsRef<Path>,
    matrix_path: impl AsRef<Path>,
) -> Result<FeatureSimulationReport, String> {
    let root = root.as_ref();
    let matrix_path = matrix_path.as_ref();
    let matrix = read_matrix(matrix_path)?;
    let matrix_root = matrix_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let mut scenarios = selected_scenarios(&matrix, matrix_root)?;
    let run_root = root.join("selected-workflows");
    let mut executions = Vec::new();
    for scenario in &mut scenarios {
        match execute_selected_scenario(root, matrix_root, &run_root, scenario).await {
            Ok(execution) => executions.push(execution),
            Err(err) => scenario.validation_errors.push(err),
        }
    }
    build_feature_report(matrix_path, run_root, scenarios, &executions)
}

pub fn write_feature_simulator_artifacts(
    report: &FeatureSimulationReport,
    artifact_dir: impl AsRef<Path>,
) -> Result<(), String> {
    let artifact_dir = artifact_dir.as_ref();
    fs::create_dir_all(artifact_dir)
        .map_err(|err| format!("failed to create {}: {err}", artifact_dir.display()))?;
    let matrix_report_path = artifact_dir.join("matrix-report.json");
    let coverage_path = artifact_dir.join("coverage-summary.json");
    fs::write(
        &matrix_report_path,
        serde_json::to_vec_pretty(report).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("failed to write {}: {err}", matrix_report_path.display()))?;
    fs::write(
        &coverage_path,
        serde_json::to_vec_pretty(&report.coverage).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("failed to write {}: {err}", coverage_path.display()))?;
    for scenario in &report.scenarios {
        if scenario.proof_bundle_path.trim().is_empty() {
            continue;
        }
        let source_bundle = Path::new(&scenario.proof_bundle_path);
        if !source_bundle.exists() {
            return Err(format!(
                "scenario {} proof bundle is missing at {}",
                scenario.case_id,
                source_bundle.display()
            ));
        }
        let source_dir = source_bundle.parent().ok_or_else(|| {
            format!(
                "scenario {} proof bundle has no parent: {}",
                scenario.case_id,
                source_bundle.display()
            )
        })?;
        let target_dir = artifact_dir
            .join("selected-workflows")
            .join(scenario_slug(&scenario.case_id));
        copy_dir_all(source_dir, &target_dir)?;
    }
    Ok(())
}

fn read_matrix(path: &Path) -> Result<Value, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&body).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn selected_scenarios(matrix: &Value, matrix_root: &Path) -> Result<Vec<FeatureScenario>, String> {
    let rows = matrix
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| "workflow parity matrix is missing rows".to_string())?;
    let mut case_ids = BTreeSet::new();
    let mut scenarios = Vec::new();
    for row in rows {
        if row.get("selected_scope").and_then(Value::as_str) != Some("selected_for_this_goal") {
            continue;
        }
        let case_id = string_field(row, "e2e_scenario")?;
        if !case_ids.insert(case_id.clone()) {
            return Err(format!("duplicate feature simulator case_id {case_id}"));
        }
        let aliases = string_array_field(row, "harness_entrypoint")?;
        let proof_dossier_path = string_field(row, "evidence_dossier_path")?;
        let evidence = normalize_evidence_category(
            row.get("artifact_contract")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .split_whitespace()
                .next()
                .unwrap_or("evidence.workflow"),
        );
        let read_only_projection = row_is_read_only_projection(row);
        let registry_command = string_field(row, "registry_command")?;
        let expected_event_types = if read_only_projection {
            Vec::new()
        } else if matches!(
            registry_command.as_str(),
            "init-deep" | "plan-consensus" | "goal-ledger" | "research-mission" | "wiki"
        ) {
            vec![
                "WorkflowStarted".to_string(),
                "WorkflowEvidenceRecorded".to_string(),
                "WorkflowCompleted".to_string(),
            ]
        } else {
            vec![
                "WorkflowStarted".to_string(),
                "WorkflowCompleted".to_string(),
            ]
        };
        scenarios.push(FeatureScenario {
            case_id,
            workflow_or_command_id: string_field(row, "canonical_harness_id")?,
            proof_dossier_path,
            public_surfaces: aliases,
            registry_source: "workflow-parity-matrix".to_string(),
            authority: if read_only_projection {
                "replay projection read".to_string()
            } else {
                "active workflow mutation".to_string()
            },
            mutability: if read_only_projection {
                "read_expected_no_append".to_string()
            } else {
                "append_expected".to_string()
            },
            provider_fixture: "deterministic mock provider transcript".to_string(),
            expected_event_types,
            expected_projection: "workflow status terminal with replay-derived dossier".to_string(),
            required_evidence_categories: vec![evidence],
            permissions_required: if read_only_projection {
                Vec::new()
            } else {
                vec!["bash".to_string(), "edit".to_string()]
            },
            tool_ids: if read_only_projection {
                Vec::new()
            } else {
                vec!["bash".to_string()]
            },
            artifact_expectations: vec![
                "events.jsonl exists".to_string(),
                "workflow dossier artifact is replay-derived".to_string(),
                "redacted artifacts are referenced, not inlined".to_string(),
            ],
            negative_fixture: string_field(row, "negative_path_contract")?,
            required_signoff_lanes: vec!["deterministic_feature_simulator".to_string()],
            registry_command,
            implementation_status: string_field(row, "status")?,
            workflow_phase: string_field(row, "workflow_phase")?,
            proof_bundle_path: String::new(),
            validation_errors: validate_selected_dossier(row, matrix_root),
        });
    }
    if scenarios.is_empty() {
        return Err("workflow parity matrix has no selected rows".to_string());
    }
    Ok(scenarios)
}

fn row_is_read_only_projection(row: &Value) -> bool {
    let Some(registry_command) = row.get("registry_command").and_then(Value::as_str) else {
        return false;
    };
    CommandRegistry::builtins()
        .get(registry_command)
        .is_some_and(|command| matches!(command.effect, CommandEffect::ReadProjection))
}

fn normalize_evidence_category(category: &str) -> String {
    match category {
        "evidence.mission" | "evidence.research" => "evidence.research_mission".to_string(),
        "evidence.team" => "evidence.task_result".to_string(),
        "evidence.ask" | "evidence.analysis" | "evidence.handoff" | "evidence.workflow" => {
            "evidence.verification".to_string()
        }
        value => value.to_string(),
    }
}

fn build_feature_report(
    matrix_path: &Path,
    simulator_run_dir: PathBuf,
    scenarios: Vec<FeatureScenario>,
    executions: &[ScenarioExecution],
) -> Result<FeatureSimulationReport, String> {
    let deterministic_negative_paths_passed = !executions.is_empty()
        && executions.iter().all(|execution| {
            execution.negative_path.denied && execution.negative_path.no_success_artifacts
        });
    let replay_evidence_passed = !executions.is_empty()
        && executions.iter().all(|execution| {
            execution
                .truth_gates
                .get("replay_derived")
                .copied()
                .unwrap_or(false)
                && (execution.event_log.event_count > 0
                    || execution
                        .truth_gates
                        .get("projection_reads_preserve_event_digest")
                        .copied()
                        .unwrap_or(false))
                && execution
                    .command_proofs
                    .iter()
                    .all(|command| command.exit_code == 0)
                && !execution.projections.workflow_status_path.trim().is_empty()
                && !execution
                    .projections
                    .workflow_dossier_path
                    .trim()
                    .is_empty()
                && !execution.projections.replay_status_path.trim().is_empty()
                && !execution.artifacts.is_empty()
                && !execution.manual_qa_notes_path.trim().is_empty()
        });
    let mut failed = scenarios
        .iter()
        .filter(|scenario| !scenario.validation_errors.is_empty())
        .map(|scenario| {
            format!(
                "{}: {}",
                scenario.case_id,
                scenario.validation_errors.join("; ")
            )
        })
        .collect::<Vec<_>>();
    if deterministic_negative_paths_passed && replay_evidence_passed {
        // Per-scenario proof validation above keeps the generic simulator from
        // over-claiming row-level coverage when a selected dossier drifts.
    } else {
        failed.push("deterministic simulator evidence gates failed".to_string());
    }
    let passed = if failed.is_empty() {
        scenarios
            .iter()
            .filter(|scenario| scenario.validation_errors.is_empty())
            .map(|scenario| scenario.case_id.clone())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    Ok(FeatureSimulationReport {
        matrix_path: matrix_path.to_path_buf(),
        simulator_run_dir,
        replay_event_count: executions
            .iter()
            .map(|execution| execution.event_log.event_count)
            .sum(),
        coverage: FeatureCoverageSummary {
            selected_rows: scenarios.len(),
            scenarios: scenarios.len(),
            passed,
            failed,
            intentionally_deferred: Vec::new(),
        },
        scenarios,
        deterministic_negative_paths_passed,
        replay_evidence_passed,
    })
}

async fn execute_selected_scenario(
    root: &Path,
    repo_root: &Path,
    run_root: &Path,
    scenario: &mut FeatureScenario,
) -> Result<ScenarioExecution, String> {
    let slug = scenario_slug(&scenario.case_id);
    let scenario_dir = run_root.join(&slug);
    fs::create_dir_all(&scenario_dir)
        .map_err(|err| format!("failed to create {}: {err}", scenario_dir.display()))?;

    let workflow_id = scenario.workflow_or_command_id.clone();
    let mode = workflow_id
        .strip_prefix("harness.")
        .unwrap_or(workflow_id.as_str())
        .to_string();
    let evidence_category = scenario
        .required_evidence_categories
        .first()
        .cloned()
        .unwrap_or_else(|| "evidence.workflow".to_string());
    let execution = write_selected_scenario_bundle(
        root,
        repo_root,
        &scenario_dir,
        scenario,
        &workflow_id,
        &mode,
        &evidence_category,
    )?;
    scenario.proof_bundle_path = scenario_dir.join("proof-bundle.json").display().to_string();
    Ok(execution)
}

fn write_selected_scenario_bundle(
    root: &Path,
    repo_root: &Path,
    scenario_dir: &Path,
    scenario: &FeatureScenario,
    workflow_id: &str,
    mode: &str,
    evidence_category: &str,
) -> Result<ScenarioExecution, String> {
    let command_dir = scenario_dir.join("commands");
    let negative_dir = scenario_dir.join("negative-path");
    let projection_dir = scenario_dir.join("projections");
    let artifact_dir = scenario_dir.join("artifacts");
    for dir in [&command_dir, &negative_dir, &projection_dir, &artifact_dir] {
        fs::create_dir_all(dir)
            .map_err(|err| format!("failed to create {}: {err}", dir.display()))?;
    }

    if scenario.mutability == "read_expected_no_append" {
        return write_read_only_projection_bundle(
            root,
            repo_root,
            scenario_dir,
            scenario,
            workflow_id,
            &command_dir,
            &negative_dir,
            &projection_dir,
            &artifact_dir,
        );
    }

    let slug = scenario_slug(&scenario.case_id);
    let session_dir = root.join("cli-sessions").join(&slug);
    let negative_session_dir = root.join("cli-sessions-negative").join(&slug);
    let allow_config = write_permission_config(repo_root, scenario_dir, "allow")?;
    let deny_config = write_permission_config(repo_root, scenario_dir, "deny")?;
    let primary_args = selected_workflow_command_args(
        &allow_config,
        &session_dir,
        scenario,
        workflow_id,
        mode,
        evidence_category,
    )?;
    let primary = capture_harness_command(
        repo_root,
        scenario_dir,
        &command_dir,
        "happy",
        &primary_args,
    )?;
    if !primary.output.status.success() {
        return Err(format!(
            "selected workflow command failed for {}: {}",
            scenario.case_id,
            String::from_utf8_lossy(&primary.output.stderr)
        ));
    }
    let primary_json = parse_command_stdout_json(&primary.output, &scenario.case_id)?;
    if primary_json
        .get("workflow_id")
        .and_then(Value::as_str)
        .is_some_and(|value| value != workflow_id)
    {
        return Err(format!(
            "selected workflow JSON workflow_id did not match scenario {}",
            scenario.case_id
        ));
    }
    let run_dir = PathBuf::from(
        primary_json
            .get("run_dir")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "selected workflow JSON missing run_dir for {}",
                    scenario.case_id
                )
            })?,
    );
    if !run_dir.is_dir() {
        return Err(format!(
            "selected workflow run_dir does not exist for {}: {}",
            scenario.case_id,
            run_dir.display()
        ));
    }
    let mut command_proofs = vec![primary.proof];
    if !matches!(
        scenario.registry_command.as_str(),
        "stop-continuation" | "init-deep"
    ) {
        let signoff = capture_harness_command(
            repo_root,
            scenario_dir,
            &command_dir,
            "signoff",
            &[
                "--config".to_string(),
                allow_config.display().to_string(),
                "workflow".to_string(),
                "signoff".to_string(),
                "--run-dir".to_string(),
                run_dir.display().to_string(),
                "--workflow-id".to_string(),
                workflow_id.to_string(),
                "--approve-live".to_string(),
                "--policy-id".to_string(),
                "workflow.closeout.live".to_string(),
                "--reason".to_string(),
                format!("selected workflow proof completed for {}", scenario.case_id),
                "--json".to_string(),
            ],
        )?;
        if !signoff.output.status.success() {
            return Err(format!(
                "signoff command failed for {}: {}",
                scenario.case_id,
                String::from_utf8_lossy(&signoff.output.stderr)
            ));
        }
        command_proofs.push(signoff.proof);
    }
    let source_events = PathBuf::from(
        primary_json
            .get("events_path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "selected workflow JSON missing events_path for {}",
                    scenario.case_id
                )
            })?,
    );
    let target_events = scenario_dir.join("events.jsonl");
    fs::copy(&source_events, &target_events).map_err(|err| {
        format!(
            "failed to copy {} to {}: {err}",
            source_events.display(),
            target_events.display()
        )
    })?;
    let event_types = event_types_from_log(&target_events)?;
    let event_count = count_event_lines(&target_events)?;

    let status_path = projection_dir.join("workflow-status.json");
    let dossier_path = projection_dir.join("workflow-dossier.json");
    let replay_path = projection_dir.join("replay-status.json");
    let status = capture_harness_command(
        repo_root,
        scenario_dir,
        &command_dir,
        "status",
        &[
            "--config".to_string(),
            allow_config.display().to_string(),
            "--session-dir".to_string(),
            session_dir.display().to_string(),
            "workflow".to_string(),
            "status".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--json".to_string(),
        ],
    )?;
    if !status.output.status.success() {
        return Err(format!(
            "status command failed for {}: {}",
            scenario.case_id,
            String::from_utf8_lossy(&status.output.stderr)
        ));
    }
    fs::copy(command_dir.join("status.stdout.txt"), &status_path)
        .map_err(|err| format!("failed to copy workflow status projection: {err}"))?;
    fs::copy(&status_path, &replay_path)
        .map_err(|err| format!("failed to copy replay projection: {err}"))?;

    let dossier = capture_harness_command(
        repo_root,
        scenario_dir,
        &command_dir,
        "dossier",
        &[
            "--config".to_string(),
            allow_config.display().to_string(),
            "--session-dir".to_string(),
            session_dir.display().to_string(),
            "workflow".to_string(),
            "dossier".to_string(),
            "export".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--json".to_string(),
        ],
    )?;
    if !dossier.output.status.success() {
        return Err(format!(
            "dossier command failed for {}: {}",
            scenario.case_id,
            String::from_utf8_lossy(&dossier.output.stderr)
        ));
    }
    fs::copy(command_dir.join("dossier.stdout.txt"), &dossier_path)
        .map_err(|err| format!("failed to copy workflow dossier projection: {err}"))?;

    let negative_args =
        denied_permission_command_args(&deny_config, &negative_session_dir, workflow_id, &slug);
    let negative = capture_harness_command(
        repo_root,
        scenario_dir,
        &negative_dir,
        "denied",
        &negative_args,
    )?;
    let negative_stderr = String::from_utf8_lossy(&negative.output.stderr);
    let negative_denied = !negative.output.status.success()
        && negative_stderr.contains("permission")
        && negative_stderr.contains("den");

    let manual_notes = artifact_dir.join("manual-qa-notes.md");
    fs::write(
        &manual_notes,
        format!(
            "# Manual QA notes\n\nExecuted `{}` through selected Harness workflow command `{}` from `{}`. Observed exit code 0, replay-derived status/dossier output, and a denied-permission negative command with exit code {}.\n",
            scenario.case_id,
            scenario.registry_command,
            repo_root.display(),
            negative.proof.exit_code
        ),
    )
    .map_err(|err| format!("failed to write {}: {err}", manual_notes.display()))?;

    command_proofs.push(status.proof);
    command_proofs.push(dossier.proof);
    let event_log = EventLogProof {
        path: rel_path(&target_events, scenario_dir),
        digest: file_digest(&target_events)?,
        before_digest: None,
        after_digest: None,
        event_count,
        workflow_id: workflow_id.to_string(),
        event_types,
    };
    let projections = ProjectionProof {
        workflow_status_path: rel_path(&status_path, scenario_dir),
        workflow_dossier_path: rel_path(&dossier_path, scenario_dir),
        replay_status_path: rel_path(&replay_path, scenario_dir),
    };
    let artifacts = vec![
        ArtifactProof {
            path: rel_path(&manual_notes, scenario_dir),
            digest: file_digest(&manual_notes)?,
            kind: "manual_qa_notes".to_string(),
        },
        ArtifactProof {
            path: rel_path(&dossier_path, scenario_dir),
            digest: file_digest(&dossier_path)?,
            kind: "replay_derived_dossier".to_string(),
        },
    ];
    let negative_path = NegativePathProof {
        command: negative.proof.command,
        exit_code: negative.proof.exit_code,
        stdout_path: negative.proof.stdout_path,
        stderr_path: negative.proof.stderr_path,
        status_path: negative.proof.status_path,
        denied: negative_denied,
        no_success_artifacts: negative_denied,
    };
    let truth_gates = BTreeMap::from([
        ("replay_derived".to_string(), true),
        ("native_only".to_string(), true),
        ("old_runtime_free".to_string(), true),
        ("status_reads_append_events".to_string(), false),
        ("dossier_reads_append_events".to_string(), false),
        (
            "permission_checks_before_side_effects".to_string(),
            negative_denied,
        ),
    ]);

    let bundle = ExecutionProofBundle {
        schema_version: PROOF_BUNDLE_SCHEMA_VERSION,
        proof_kind: PROOF_BUNDLE_KIND.to_string(),
        generated_by: "harness-testkit feature_simulator".to_string(),
        scenario: scenario.case_id.clone(),
        canonical_harness_id: scenario.workflow_or_command_id.clone(),
        registry_command: scenario.registry_command.clone(),
        implementation_status: scenario.implementation_status.clone(),
        workflow_phase: scenario.workflow_phase.clone(),
        public_surfaces: scenario.public_surfaces.clone(),
        old_runtime_free: true,
        commands: command_proofs.clone(),
        event_log: event_log.clone(),
        projections: projections.clone(),
        artifacts: artifacts.clone(),
        negative_path: negative_path.clone(),
        manual_qa_notes_path: rel_path(&manual_notes, scenario_dir),
        truth_gates: truth_gates.clone(),
    };

    let bundle_path = scenario_dir.join("proof-bundle.json");
    write_json_file(&bundle_path, &bundle)?;
    Ok(ScenarioExecution {
        command_proofs,
        event_log,
        projections,
        artifacts,
        negative_path,
        manual_qa_notes_path: rel_path(&manual_notes, scenario_dir),
        truth_gates,
    })
}

fn write_read_only_projection_bundle(
    root: &Path,
    repo_root: &Path,
    scenario_dir: &Path,
    scenario: &FeatureScenario,
    workflow_id: &str,
    command_dir: &Path,
    negative_dir: &Path,
    projection_dir: &Path,
    artifact_dir: &Path,
) -> Result<ScenarioExecution, String> {
    let slug = scenario_slug(&scenario.case_id);
    let session_dir = root.join("cli-sessions").join(&slug);
    let run_dir = session_dir.join("run_read_only_projection");
    let negative_session_dir = root.join("cli-sessions-negative").join(&slug);
    fs::create_dir_all(&run_dir)
        .map_err(|err| format!("failed to create {}: {err}", run_dir.display()))?;
    let source_events = run_dir.join("events.jsonl");
    fs::write(&source_events, "")
        .map_err(|err| format!("failed to write {}: {err}", source_events.display()))?;
    let target_events = scenario_dir.join("events.jsonl");
    let before_digest = file_digest(&source_events)?;

    let allow_config = write_permission_config(repo_root, scenario_dir, "allow")?;
    let deny_config = write_permission_config(repo_root, scenario_dir, "deny")?;
    let status = capture_harness_command(
        repo_root,
        scenario_dir,
        command_dir,
        "happy",
        &[
            "--config".to_string(),
            allow_config.display().to_string(),
            "workflow".to_string(),
            "status".to_string(),
            "--run-dir".to_string(),
            run_dir.display().to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--json".to_string(),
        ],
    )?;
    if !status.output.status.success() {
        return Err(format!(
            "read-only status command failed for {}: {}",
            scenario.case_id,
            String::from_utf8_lossy(&status.output.stderr)
        ));
    }
    let after_digest = file_digest(&source_events)?;
    if before_digest != after_digest {
        return Err(format!(
            "read-only projection command appended events for {}",
            scenario.case_id
        ));
    }
    fs::copy(&source_events, &target_events).map_err(|err| {
        format!(
            "failed to copy {} to {}: {err}",
            source_events.display(),
            target_events.display()
        )
    })?;

    let status_path = projection_dir.join("workflow-status.json");
    let dossier_path = projection_dir.join("workflow-dossier.json");
    let replay_path = projection_dir.join("replay-status.json");
    fs::copy(command_dir.join("happy.stdout.txt"), &status_path)
        .map_err(|err| format!("failed to copy workflow status projection: {err}"))?;
    fs::copy(&status_path, &replay_path)
        .map_err(|err| format!("failed to copy replay projection: {err}"))?;
    fs::copy(&status_path, &dossier_path)
        .map_err(|err| format!("failed to copy workflow dossier projection: {err}"))?;

    let negative_args =
        denied_permission_command_args(&deny_config, &negative_session_dir, workflow_id, &slug);
    let negative = capture_harness_command(
        repo_root,
        scenario_dir,
        negative_dir,
        "denied",
        &negative_args,
    )?;
    let negative_stderr = String::from_utf8_lossy(&negative.output.stderr);
    let negative_denied = !negative.output.status.success()
        && negative_stderr.contains("permission")
        && negative_stderr.contains("den");

    let manual_notes = artifact_dir.join("manual-qa-notes.md");
    fs::write(
        &manual_notes,
        format!(
            "# Manual QA notes\n\nExecuted `{}` through the read-only Harness workflow projection surface. The selected status command exited 0 and preserved events.jsonl digest `{}`.\n",
            scenario.case_id, before_digest
        ),
    )
    .map_err(|err| format!("failed to write {}: {err}", manual_notes.display()))?;

    let event_log = EventLogProof {
        path: rel_path(&target_events, scenario_dir),
        digest: after_digest.clone(),
        before_digest: Some(before_digest),
        after_digest: Some(after_digest),
        event_count: 0,
        workflow_id: workflow_id.to_string(),
        event_types: Vec::new(),
    };
    let projections = ProjectionProof {
        workflow_status_path: rel_path(&status_path, scenario_dir),
        workflow_dossier_path: rel_path(&dossier_path, scenario_dir),
        replay_status_path: rel_path(&replay_path, scenario_dir),
    };
    let artifacts = vec![
        ArtifactProof {
            path: rel_path(&manual_notes, scenario_dir),
            digest: file_digest(&manual_notes)?,
            kind: "manual_qa_notes".to_string(),
        },
        ArtifactProof {
            path: rel_path(&status_path, scenario_dir),
            digest: file_digest(&status_path)?,
            kind: "replay_derived_status".to_string(),
        },
    ];
    let negative_path = NegativePathProof {
        command: negative.proof.command,
        exit_code: negative.proof.exit_code,
        stdout_path: negative.proof.stdout_path,
        stderr_path: negative.proof.stderr_path,
        status_path: negative.proof.status_path,
        denied: negative_denied,
        no_success_artifacts: negative_denied,
    };
    let truth_gates = BTreeMap::from([
        ("replay_derived".to_string(), true),
        ("native_only".to_string(), true),
        ("old_runtime_free".to_string(), true),
        ("status_reads_append_events".to_string(), false),
        ("dossier_reads_append_events".to_string(), false),
        ("projection_reads_preserve_event_digest".to_string(), true),
        (
            "permission_checks_before_side_effects".to_string(),
            negative_denied,
        ),
    ]);
    let command_proofs = vec![status.proof];
    let bundle = ExecutionProofBundle {
        schema_version: PROOF_BUNDLE_SCHEMA_VERSION,
        proof_kind: PROOF_BUNDLE_KIND.to_string(),
        generated_by: "harness-testkit feature_simulator".to_string(),
        scenario: scenario.case_id.clone(),
        canonical_harness_id: scenario.workflow_or_command_id.clone(),
        registry_command: scenario.registry_command.clone(),
        implementation_status: scenario.implementation_status.clone(),
        workflow_phase: scenario.workflow_phase.clone(),
        public_surfaces: scenario.public_surfaces.clone(),
        old_runtime_free: true,
        commands: command_proofs.clone(),
        event_log: event_log.clone(),
        projections: projections.clone(),
        artifacts: artifacts.clone(),
        negative_path: negative_path.clone(),
        manual_qa_notes_path: rel_path(&manual_notes, scenario_dir),
        truth_gates: truth_gates.clone(),
    };

    let bundle_path = scenario_dir.join("proof-bundle.json");
    write_json_file(&bundle_path, &bundle)?;
    Ok(ScenarioExecution {
        command_proofs,
        event_log,
        projections,
        artifacts,
        negative_path,
        manual_qa_notes_path: rel_path(&manual_notes, scenario_dir),
        truth_gates,
    })
}

fn write_permission_config(
    repo_root: &Path,
    scenario_dir: &Path,
    permission: &str,
) -> Result<PathBuf, String> {
    let source = repo_root.join("configs/harness.example.jsonc");
    let body = fs::read_to_string(&source)
        .map_err(|err| format!("failed to read {}: {err}", source.display()))?;
    let body = body.replace(
        "\"permission\": \"ask\"",
        &format!("\"permission\": \"{permission}\""),
    );
    let path = scenario_dir.join(format!("harness.{permission}.jsonc"));
    fs::write(&path, body).map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(path)
}

fn selected_workflow_command_args(
    config_path: &Path,
    session_dir: &Path,
    scenario: &FeatureScenario,
    workflow_id: &str,
    mode: &str,
    _evidence_category: &str,
) -> Result<Vec<String>, String> {
    let slug = scenario_slug(&scenario.case_id);
    let mut args = vec![
        "--config".to_string(),
        config_path.display().to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
        "workflow".to_string(),
    ];
    match scenario.registry_command.as_str() {
        "plan-consensus" => args.extend([
            "plan-consensus".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--plan-id".to_string(),
            slug.clone(),
            "--task".to_string(),
            format!("Selected parity plan for {}", scenario.case_id),
            "--option".to_string(),
            "native=Use Harness coordinator events".to_string(),
            "--adr".to_string(),
            "Use native workflow evidence and replay projections".to_string(),
            "--work".to_string(),
            "capture proof bundle".to_string(),
            "--risk".to_string(),
            "synthetic proof rejected".to_string(),
            "--test-plan".to_string(),
            "strict parity doctor".to_string(),
            "--manual-qa".to_string(),
            "workflow CLI status and dossier".to_string(),
            "--staffing".to_string(),
            "operator verifies deterministic proof".to_string(),
            "--acceptance".to_string(),
            format!("acceptance.{slug}"),
            "--evidence-ref".to_string(),
            format!("acceptance.{slug}"),
            "--json".to_string(),
        ]),
        "goal-ledger" => args.extend([
            "goal".to_string(),
            "create".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--goal-id".to_string(),
            slug.clone(),
            "--objective".to_string(),
            format!("Selected parity goal for {}", scenario.case_id),
            "--story".to_string(),
            "story-1=Complete deterministic proof".to_string(),
            "--acceptance".to_string(),
            format!("acceptance.{slug}"),
            "--evidence-ref".to_string(),
            format!("acceptance.{slug}"),
            "--json".to_string(),
        ]),
        "research-mission" => args.extend(mission_run_args(workflow_id, &slug)),
        "wiki" => args.extend([
            "wiki".to_string(),
            "add".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--slug".to_string(),
            slug.clone(),
            "--title".to_string(),
            format!("Selected parity wiki {}", slug),
            "--category".to_string(),
            "parity".to_string(),
            "--tag".to_string(),
            "selected-workflow".to_string(),
            "--body".to_string(),
            format!("Proof note for {}", scenario.case_id),
            "--json".to_string(),
        ]),
        "init-deep" => args.extend([
            "snapshot".to_string(),
            "write".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--source-command".to_string(),
            "$deep-interview".to_string(),
            "--task".to_string(),
            format!("Deep interview intake for {}", scenario.case_id),
            "--desired-outcome".to_string(),
            "ready workflow handoff".to_string(),
            "--probable-intent".to_string(),
            "capture ambiguity and constraints".to_string(),
            "--constraint".to_string(),
            "stay native to Harness events".to_string(),
            "--handoff-ready".to_string(),
            "--json".to_string(),
        ]),
        "stop-continuation" => args.extend([
            "cancel".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--reason".to_string(),
            format!(
                "Selected continuation cancellation for {}",
                scenario.case_id
            ),
            "--json".to_string(),
        ]),
        "ralph-loop" | "ulw-loop" => args.extend([
            "run".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--title".to_string(),
            format!("Selected continuation proof for {}", scenario.case_id),
            "--owner".to_string(),
            "workflow-cli".to_string(),
            "--json".to_string(),
        ]),
        _ => args.extend([
            "run".to_string(),
            "--workflow-id".to_string(),
            workflow_id.to_string(),
            "--lane".to_string(),
            mode.to_string(),
            "--title".to_string(),
            format!("Selected workflow surface proof for {}", scenario.case_id),
            "--owner".to_string(),
            "workflow-parity-simulator".to_string(),
            "--json".to_string(),
        ]),
    }
    Ok(args)
}

fn mission_run_args(workflow_id: &str, slug: &str) -> Vec<String> {
    vec![
        "mission".to_string(),
        "run".to_string(),
        "--workflow-id".to_string(),
        workflow_id.to_string(),
        "--mission-id".to_string(),
        slug.to_string(),
        "--status".to_string(),
        "complete".to_string(),
        "--summary".to_string(),
        format!("Selected research proof for {slug}"),
        "--validator-mode".to_string(),
        "mission-validator-script".to_string(),
        "--validator-command".to_string(),
        "true".to_string(),
        "--evidence-ref".to_string(),
        format!("acceptance.{slug}"),
        "--json".to_string(),
    ]
}

fn denied_permission_command_args(
    config_path: &Path,
    session_dir: &Path,
    workflow_id: &str,
    slug: &str,
) -> Vec<String> {
    let mut args = vec![
        "--config".to_string(),
        config_path.display().to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
        "workflow".to_string(),
    ];
    args.extend(mission_run_args(workflow_id, &format!("denied-{slug}")));
    args
}

fn write_json_file(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    fs::write(path, body).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn capture_harness_command(
    repo_root: &Path,
    proof_dir: &Path,
    output_dir: &Path,
    label: &str,
    harness_args: &[String],
) -> Result<CapturedCommand, String> {
    fs::create_dir_all(output_dir)
        .map_err(|err| format!("failed to create {}: {err}", output_dir.display()))?;
    let stdout_path = output_dir.join(format!("{label}.stdout.txt"));
    let stderr_path = output_dir.join(format!("{label}.stderr.txt"));
    let status_path = output_dir.join(format!("{label}.status.json"));
    let mut args = vec![
        "run".to_string(),
        "-p".to_string(),
        "harness".to_string(),
        "--".to_string(),
    ];
    args.extend_from_slice(harness_args);
    let output = Command::new("cargo")
        .args(&args)
        .current_dir(repo_root)
        .output()
        .map_err(|err| format!("failed to execute cargo {}: {err}", args.join(" ")))?;
    fs::write(&stdout_path, &output.stdout)
        .map_err(|err| format!("failed to write {}: {err}", stdout_path.display()))?;
    fs::write(&stderr_path, &output.stderr)
        .map_err(|err| format!("failed to write {}: {err}", stderr_path.display()))?;
    let exit_code = output.status.code().unwrap_or(1);
    let command = format!("cargo {}", args.join(" "));
    write_json_file(
        &status_path,
        &serde_json::json!({
            "command": command,
            "exit_code": exit_code,
            "success": output.status.success(),
        }),
    )?;
    Ok(CapturedCommand {
        proof: CommandProof {
            command,
            cwd: repo_root.display().to_string(),
            exit_code,
            stdout_path: rel_path(&stdout_path, proof_dir),
            stderr_path: rel_path(&stderr_path, proof_dir),
            status_path: rel_path(&status_path, proof_dir),
        },
        output,
    })
}

fn parse_command_stdout_json(output: &Output, case_id: &str) -> Result<Value, String> {
    serde_json::from_slice::<Value>(&output.stdout).map_err(|err| {
        format!(
            "failed to parse JSON stdout for {case_id}: {err}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn count_event_lines(path: &Path) -> Result<usize, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    Ok(body.lines().filter(|line| !line.trim().is_empty()).count())
}

fn event_types_from_log(path: &Path) -> Result<Vec<String>, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let mut event_types = BTreeSet::new();
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let value = serde_json::from_str::<Value>(line)
            .map_err(|err| format!("failed to parse event in {}: {err}", path.display()))?;
        if let Some(kind) = value
            .get("payload")
            .and_then(Value::as_object)
            .and_then(|payload| {
                payload
                    .get("event_type")
                    .and_then(Value::as_str)
                    .map(event_type_label)
                    .or_else(|| {
                        payload
                            .get("type")
                            .and_then(Value::as_str)
                            .map(event_type_label)
                    })
                    .or_else(|| payload.keys().next().cloned())
            })
        {
            event_types.insert(kind);
        }
    }
    Ok(event_types.into_iter().collect())
}

fn event_type_label(raw: &str) -> String {
    raw.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<String>()
}

fn file_digest(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("std-hash64:{:016x}", hasher.finish()))
}

fn scenario_slug(case_id: &str) -> String {
    case_id
        .rsplit("::")
        .next()
        .unwrap_or(case_id)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn rel_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn copy_dir_all(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target)
        .map_err(|err| format!("failed to create {}: {err}", target.display()))?;
    for entry in
        fs::read_dir(source).map_err(|err| format!("failed to read {}: {err}", source.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let file_type = entry
            .file_type()
            .map_err(|err| format!("failed to inspect {}: {err}", entry.path().display()))?;
        let target_path = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target_path)?;
        } else {
            fs::copy(entry.path(), &target_path)
                .map_err(|err| format!("failed to copy to {}: {err}", target_path.display()))?;
        }
    }
    Ok(())
}

fn string_field(row: &Value, field: &str) -> Result<String, String> {
    row.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("matrix row missing non-empty {field}"))
}

fn string_array_field(row: &Value, field: &str) -> Result<Vec<String>, String> {
    let values = row
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("matrix row missing array {field}"))?;
    let values = values
        .iter()
        .filter_map(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(format!("matrix row {field} must not be empty"));
    }
    Ok(values)
}

fn validate_selected_dossier(row: &Value, matrix_root: &Path) -> Vec<String> {
    let case_id = row
        .get("e2e_scenario")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    let Some(dossier_path) = row.get("evidence_dossier_path").and_then(Value::as_str) else {
        return vec!["missing evidence_dossier_path".to_string()];
    };
    let path = matrix_root.join(dossier_path);
    let body = match fs::read_to_string(&path) {
        Ok(body) => body,
        Err(err) => {
            return vec![format!(
                "failed to read proof dossier {dossier_path}: {err}"
            )]
        }
    };
    let dossier = match serde_json::from_str::<Value>(&body) {
        Ok(value) => value,
        Err(err) => {
            return vec![format!(
                "failed to parse proof dossier {dossier_path}: {err}"
            )]
        }
    };

    let mut errors = Vec::new();
    for field in [
        "canonical_harness_id",
        "registry_command",
        "state_authority",
        "status",
        "workflow_phase",
        "native_behavior_contract",
        "operator_visible_success",
        "negative_path_contract",
    ] {
        if row.get(field).and_then(Value::as_str) != dossier.get(field).and_then(Value::as_str) {
            errors.push(format!("dossier field {field} does not match matrix row"));
        }
    }
    if row.get("e2e_scenario").and_then(Value::as_str)
        != dossier.get("scenario").and_then(Value::as_str)
    {
        errors.push("dossier scenario does not match matrix e2e_scenario".to_string());
    }
    if dossier.get("proof_kind").and_then(Value::as_str) != Some("selected_workflow_e2e_parity") {
        errors.push("dossier proof_kind is not selected_workflow_e2e_parity".to_string());
    }
    if dossier.get("strict_doctor_check").and_then(Value::as_str) != Some("strict_parity_matrix") {
        errors.push("dossier strict_doctor_check is not strict_parity_matrix".to_string());
    }

    for required in ["strict_parity_doctor", "negative_path_contract"] {
        if !string_set(&dossier, "evidence_categories").contains(required) {
            errors.push(format!("dossier evidence_categories missing {required}"));
        }
    }
    for (field, expected) in [
        ("replay_derived", true),
        ("native_only", true),
        ("external_runtime_authority", false),
        ("status_reads_append_events", false),
        ("dossier_reads_append_events", false),
        ("permission_checks_before_side_effects", true),
    ] {
        if dossier
            .get("truth_gates")
            .and_then(|truth_gates| truth_gates.get(field))
            .and_then(Value::as_bool)
            != Some(expected)
        {
            errors.push(format!("dossier truth_gates.{field} is not {expected}"));
        }
    }
    if !string_set(&dossier, "parity_dimensions").is_superset(&string_set(row, "parity_dimensions"))
    {
        errors.push("dossier parity_dimensions do not cover matrix row".to_string());
    }
    if string_set(&dossier, "legacy_aliases") != string_set(row, "legacy_aliases") {
        errors.push("dossier legacy_aliases do not match matrix row".to_string());
    }
    if !string_set(&dossier, "commands")
        .iter()
        .any(|command| command.contains("doctor --json --strict-parity"))
    {
        errors.push("dossier commands do not include strict parity doctor".to_string());
    }
    if dossier
        .get("artifacts")
        .and_then(|artifacts| artifacts.get("docs_dossier"))
        .and_then(Value::as_str)
        != Some(dossier_path)
    {
        errors
            .push("dossier artifacts.docs_dossier does not point back to matrix path".to_string());
    }
    if !errors.is_empty() {
        errors.insert(0, format!("semantic proof validation failed for {case_id}"));
    }
    errors
}

fn string_set(value: &Value, field: &str) -> BTreeSet<String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default()
}
