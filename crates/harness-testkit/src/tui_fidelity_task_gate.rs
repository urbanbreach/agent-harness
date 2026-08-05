use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::tui_fidelity_closure::complete_boulder_atomically;

mod boulder;
mod plan;
mod receipts;
mod storage;

pub(super) const ADMISSION_SCHEMA: &str = "harness.tui-fidelity.task-admission.v1";
pub(super) const GATE_SCHEMA: &str = "harness.tui-fidelity.task-verification.v1";
pub(super) const COMPLETION_SCHEMA: &str = "harness.tui-fidelity.task-completion.v1";

#[derive(Clone, Debug)]
pub struct TaskGateInput {
    pub task: String,
    pub candidate_sha256: String,
    pub boulder: PathBuf,
    pub plan: PathBuf,
    pub evidence_root: PathBuf,
    pub admission_receipt: PathBuf,
    pub aggregate_receipt: PathBuf,
    pub verification_receipt: PathBuf,
    pub gate_receipt: PathBuf,
    pub completion_receipt: PathBuf,
    pub revocations: Vec<PathBuf>,
    pub shards: Vec<PathBuf>,
    pub dependencies: Vec<PathBuf>,
    pub finalize_closure: bool,
    pub closure_receipt: Option<PathBuf>,
}

#[derive(Debug)]
pub enum TaskGateError {
    Invalid(String),
    Io { path: PathBuf, detail: String },
    Json { path: PathBuf, detail: String },
}

impl fmt::Display for TaskGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "task gate: {detail}"),
            Self::Io { path, detail } => {
                write!(formatter, "task gate I/O {}: {detail}", path.display())
            }
            Self::Json { path, detail } => {
                write!(formatter, "task gate JSON {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for TaskGateError {}

pub fn admit(input: &TaskGateInput) -> Result<PathBuf, TaskGateError> {
    receipts::validate_input(input)?;
    let plan_text = storage::read_text(&input.plan)?;
    let plan_sha256 = storage::digest(plan_text.as_bytes())?;
    plan::ensure_open_task(&plan_text, &input.task)?;
    let boulder_json = storage::read_json(&input.boulder)?;
    boulder::validate_active(&boulder_json)?;
    boulder::validate_plan_binding(&boulder_json, &plan_sha256)?;
    boulder::validate_bootstrap(&boulder_json, &input.task)?;
    let revocations = boulder::validate_revocations(input, &boulder_json)?;
    boulder::validate_dependencies(input)?;
    let receipt = serde_json::json!({
        "schema_version": ADMISSION_SCHEMA,
        "status": "admitted",
        "task": input.task,
        "candidate_sha256": input.candidate_sha256,
        "plan_sha256": plan_sha256,
        "reviewed_plan_sha256": receipts::value_string(&boulder_json, "reviewed_plan_sha256")?,
        "worker_spawn_allowed": false,
        "task_sessions_empty": true,
        "aggregate_deadline_seconds": 120,
        "revocations": revocations,
        "dependencies": input.dependencies,
        "admitted_at_millis": storage::now_millis()?,
    });
    storage::write_unique_json(&input.admission_receipt, &receipt)?;
    Ok(input.admission_receipt.clone())
}

pub fn verify(input: &TaskGateInput) -> Result<PathBuf, TaskGateError> {
    receipts::validate_input(input)?;
    let admission = storage::read_json(&input.admission_receipt)?;
    receipts::validate_admission(&admission, input)?;
    let plan_text = storage::read_text(&input.plan)?;
    let plan_sha256 = storage::digest(plan_text.as_bytes())?;
    if receipts::value_string(&admission, "plan_sha256")? != plan_sha256 {
        return Err(TaskGateError::Invalid(
            "plan changed after task admission".to_owned(),
        ));
    }
    plan::ensure_open_task(&plan_text, &input.task)?;
    let boulder_json = storage::read_json(&input.boulder)?;
    boulder::validate_active(&boulder_json)?;
    boulder::validate_plan_binding(&boulder_json, &plan_sha256)?;
    boulder::validate_revocations(input, &boulder_json)?;
    boulder::validate_dependencies(input)?;
    let aggregate = storage::read_json(&input.aggregate_receipt)?;
    receipts::validate_watchdog(&aggregate)?;
    let verification = storage::read_json(&input.verification_receipt)?;
    receipts::validate_sealed_verification(&verification, input)?;
    if input.shards.is_empty() {
        return Err(TaskGateError::Invalid(
            "at least one selected shard receipt is required".to_owned(),
        ));
    }
    for shard in &input.shards {
        receipts::validate_shard(shard, input)?;
    }
    let receipt = serde_json::json!({
        "schema_version": GATE_SCHEMA,
        "status": "verified",
        "task": input.task,
        "candidate_sha256": input.candidate_sha256,
        "plan_sha256": plan_sha256,
        "admission_receipt": input.admission_receipt,
        "aggregate_receipt": input.aggregate_receipt,
        "verification_receipt": input.verification_receipt,
        "shards": input.shards,
        "dependencies": input.dependencies,
        "revocations": input.revocations,
        "verified_at_millis": storage::now_millis()?,
    });
    storage::write_unique_json(&input.gate_receipt, &receipt)?;
    Ok(input.gate_receipt.clone())
}

pub fn complete(input: &TaskGateInput) -> Result<PathBuf, TaskGateError> {
    receipts::validate_input(input)?;
    if input.completion_receipt.exists() {
        return Err(TaskGateError::Invalid(format!(
            "receipt replay is rejected: {}",
            input.completion_receipt.display()
        )));
    }
    let gate = storage::read_json(&input.gate_receipt)?;
    receipts::validate_gate_receipt(&gate, input)?;
    let original_plan = storage::read_text(&input.plan)?;
    let original_boulder = storage::read_text(&input.boulder)?;
    let before_sha256 = storage::digest(original_plan.as_bytes())?;
    receipts::validate_current_execution(input, &before_sha256, &gate)?;
    let updated_plan = plan::complete_task_checkbox(&original_plan, &input.task)?;
    let after_sha256 = storage::digest(updated_plan.as_bytes())?;
    let completion = serde_json::json!({
        "schema_version": COMPLETION_SCHEMA,
        "status": "completed",
        "task": input.task,
        "candidate_sha256": input.candidate_sha256,
        "plan_sha256_before": before_sha256,
        "plan_sha256_after": after_sha256,
        "verification_receipt": input.gate_receipt,
        "completed_at_millis": storage::now_millis()?,
    });
    storage::write_unique_json(&input.completion_receipt, &completion)?;
    if let Err(error) = storage::atomic_write(&input.plan, updated_plan.as_bytes()) {
        let _ = fs::remove_file(&input.completion_receipt);
        return Err(error);
    }
    if let Err(error) = boulder::record_plan_transition(
        &input.boulder,
        &input.task,
        &input.completion_receipt,
        &after_sha256,
    ) {
        return rollback(input, &original_plan, &original_boulder, error);
    }
    if input.finalize_closure {
        let Some(closure_receipt) = &input.closure_receipt else {
            return rollback(
                input,
                &original_plan,
                &original_boulder,
                TaskGateError::Invalid("finalize-closure requires a closure receipt".to_owned()),
            );
        };
        let closure_json = match storage::read_text(closure_receipt) {
            Ok(json) => json,
            Err(error) => return rollback(input, &original_plan, &original_boulder, error),
        };
        if let Err(error) = complete_boulder_atomically(&input.boulder, &closure_json) {
            return rollback(
                input,
                &original_plan,
                &original_boulder,
                TaskGateError::Invalid(error.to_string()),
            );
        }
    }
    Ok(input.completion_receipt.clone())
}

fn rollback(
    input: &TaskGateInput,
    original_plan: &str,
    original_boulder: &str,
    primary: TaskGateError,
) -> Result<PathBuf, TaskGateError> {
    let _ = fs::remove_file(&input.completion_receipt);
    let plan_error = storage::atomic_write(&input.plan, original_plan.as_bytes()).err();
    let boulder_error = storage::atomic_write(&input.boulder, original_boulder.as_bytes()).err();
    match (plan_error, boulder_error) {
        (None, None) => Err(primary),
        (Some(plan), None) => Err(TaskGateError::Invalid(format!(
            "{primary}; plan rollback failed: {plan}"
        ))),
        (None, Some(boulder)) => Err(TaskGateError::Invalid(format!(
            "{primary}; Boulder rollback failed: {boulder}"
        ))),
        (Some(plan), Some(boulder)) => Err(TaskGateError::Invalid(format!(
            "{primary}; plan rollback failed: {plan}; Boulder rollback failed: {boulder}"
        ))),
    }
}
