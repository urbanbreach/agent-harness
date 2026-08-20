use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::tui_fidelity_obligation::CaptureKey;

use super::{
    CoverageManifest, CoverageReport, MatrixError, MatrixExecution, MatrixExecutionReceipt,
    MatrixReceipt, MatrixRowReceipt,
};

type TrialResult = (bool, bool, String);

pub fn execute_matrix<F>(
    manifest: CoverageManifest,
    report: CoverageReport,
    suite: &str,
    evidence_root: &Path,
    mut execute_trial: F,
) -> Result<MatrixReceipt, MatrixError>
where
    F: FnMut(MatrixExecution) -> Result<(bool, bool, String), MatrixError>,
{
    fs::create_dir_all(evidence_root).map_err(|error| MatrixError::Io {
        path: evidence_root.to_path_buf(),
        detail: error.to_string(),
    })?;
    let mut execution_by_key = BTreeMap::<(String, u8), TrialResult>::new();
    for task in planned_tasks(&manifest, evidence_root)? {
        let value = match execute_trial(task.execution) {
            Ok(value) => value,
            Err(error) => (false, false, error.to_string()),
        };
        execution_by_key.insert(task.key, value);
    }
    finish_matrix(manifest, report, suite, evidence_root, execution_by_key)
}

pub fn execute_matrix_bounded<F>(
    manifest: CoverageManifest,
    report: CoverageReport,
    suite: &str,
    evidence_root: &Path,
    workers: usize,
    execute_trial: F,
) -> Result<MatrixReceipt, MatrixError>
where
    F: Fn(MatrixExecution) -> Result<(bool, bool, String), MatrixError> + Sync,
{
    if workers == 0 {
        return Err(MatrixError::Invalid(
            "matrix worker count must be at least one".to_owned(),
        ));
    }
    fs::create_dir_all(evidence_root).map_err(|error| MatrixError::Io {
        path: evidence_root.to_path_buf(),
        detail: error.to_string(),
    })?;
    let tasks = planned_tasks(&manifest, evidence_root)?;
    let queue = Arc::new(Mutex::new(VecDeque::from(tasks)));
    let worker_count = workers.min(
        queue
            .lock()
            .map_err(|error| MatrixError::Execution(format!("matrix queue lock: {error}")))?
            .len(),
    );
    let results = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let execute_trial = &execute_trial;
            handles.push(scope.spawn(move || {
                let mut results = Vec::new();
                loop {
                    let task = queue
                        .lock()
                        .map_err(|error| {
                            MatrixError::Execution(format!("matrix queue lock: {error}"))
                        })?
                        .pop_front();
                    let Some(task) = task else { break };
                    let value = match execute_trial(task.execution) {
                        Ok(value) => value,
                        Err(error) => (false, false, error.to_string()),
                    };
                    results.push((task.key, value));
                }
                Ok::<Vec<((String, u8), TrialResult)>, MatrixError>(results)
            }));
        }
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| MatrixError::Execution("matrix worker panicked".to_owned()))?
            })
            .collect::<Result<Vec<_>, MatrixError>>()
    })?;
    let execution_by_key = results.into_iter().flatten().collect();
    finish_matrix(manifest, report, suite, evidence_root, execution_by_key)
}

struct MatrixTask {
    key: (String, u8),
    execution: MatrixExecution,
}

fn planned_tasks(
    manifest: &CoverageManifest,
    evidence_root: &Path,
) -> Result<Vec<MatrixTask>, MatrixError> {
    let mut tasks = Vec::with_capacity(manifest.rows.len() * 5);
    for row in &manifest.rows {
        CaptureKey::from_row(row)
            .canonical_json()
            .map_err(|error| MatrixError::Invalid(error.to_string()))?;
        for trial in 1..=row.trials {
            tasks.push(MatrixTask {
                key: (row.row_id.clone(), trial),
                execution: MatrixExecution {
                    row: row.clone(),
                    trial,
                    evidence_dir: evidence_root
                        .join(&row.row_id)
                        .join(format!("trial-{trial}")),
                },
            });
        }
    }
    Ok(tasks)
}

fn finish_matrix(
    manifest: CoverageManifest,
    report: CoverageReport,
    suite: &str,
    evidence_root: &Path,
    execution_by_key: BTreeMap<(String, u8), TrialResult>,
) -> Result<MatrixReceipt, MatrixError> {
    let mut rows = Vec::with_capacity(manifest.rows.len());
    let mut capture_succeeded = true;
    let mut comparison_passed = true;
    if execution_by_key.len() != report.execution_count {
        return Err(MatrixError::Execution(format!(
            "matrix executed {} row/trials but coverage requires {}",
            execution_by_key.len(),
            report.execution_count
        )));
    }
    for (captured, compared, _) in execution_by_key.values() {
        capture_succeeded &= *captured;
        comparison_passed &= *compared;
    }
    for row in manifest.rows {
        let key = CaptureKey::from_row(&row)
            .canonical_json()
            .map_err(|error| MatrixError::Invalid(error.to_string()))?;
        let mut executions = Vec::with_capacity(usize::from(row.trials));
        for trial in 1..=row.trials {
            let result = execution_by_key
                .get(&(row.row_id.clone(), trial))
                .ok_or_else(|| {
                    MatrixError::Execution(format!(
                        "missing matrix result for row {} trial {trial}",
                        row.row_id
                    ))
                })?;
            executions.push(MatrixExecutionReceipt {
                trial,
                capture_key: key.clone(),
                capture_succeeded: result.0,
                comparison_passed: result.1,
                detail: result.2.clone(),
            });
        }
        rows.push(MatrixRowReceipt {
            row_id: row.row_id,
            requirement_id: row.requirement_id,
            executions,
        });
    }
    let passed = capture_succeeded && comparison_passed;
    let receipt = MatrixReceipt {
        schema_version: "harness.tui-fidelity.matrix.v3".to_owned(),
        suite: suite.to_owned(),
        status: if passed { "complete" } else { "failed" }.to_owned(),
        capture_succeeded,
        comparison_passed: passed,
        report,
        rows,
    };
    let receipt_path = evidence_root.join("matrix-receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| MatrixError::Json(error.to_string()))?;
    fs::write(&receipt_path, &bytes).map_err(|error| MatrixError::Io {
        path: receipt_path,
        detail: error.to_string(),
    })?;
    if receipt.comparison_passed {
        let completion_path = evidence_root.join("matrix-completion.json");
        fs::write(&completion_path, &bytes).map_err(|error| MatrixError::Io {
            path: completion_path,
            detail: error.to_string(),
        })?;
        Ok(receipt)
    } else {
        Err(MatrixError::Execution(
            "capture and comparison must both pass for every execution".to_owned(),
        ))
    }
}
