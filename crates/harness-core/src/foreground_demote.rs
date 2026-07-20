//! Foreground → background demotion request types (honest MVP).
//!
//! Models a demote request for a running foreground shell/task handle. Full
//! interactive shell demotion product UX is out of scope; this is the typed
//! request/result surface so orchestration layers can accept demotion without
//! claiming reference-complete behavior.
//!
//! Existing `task(run_in_background: true)` spawn and `background_output` wait
//! paths remain the primary background APIs; demotion is a separate mid-flight
//! transition.

use serde::{Deserialize, Serialize};

/// Kind of foreground work being demoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundKind {
    /// Interactive / foreground bash-like process.
    Shell,
    /// Foreground child task (task tool wait path).
    Task,
}

impl ForegroundKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Task => "task",
        }
    }
}

/// Request to demote a foreground unit of work to background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoteToBackgroundRequest {
    /// Stable handle for the foreground work (task id, request id, or shell id).
    pub handle_id: String,
    pub kind: ForegroundKind,
    /// Optional operator reason (diagnostics only; not a secret).
    pub reason: Option<String>,
}

impl DemoteToBackgroundRequest {
    pub fn new(handle_id: impl Into<String>, kind: ForegroundKind) -> Self {
        Self {
            handle_id: handle_id.into(),
            kind,
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Validate non-empty handle. Parse-don't-validate at the API boundary.
    pub fn validate(&self) -> Result<(), DemoteError> {
        if self.handle_id.trim().is_empty() {
            return Err(DemoteError::EmptyHandle);
        }
        Ok(())
    }
}

/// Result of applying a demote request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DemoteToBackgroundResult {
    /// Demotion accepted; work continues as background under `background_id`.
    Demoted {
        handle_id: String,
        background_id: String,
        kind: ForegroundKind,
    },
    /// Handle not found / not demotable.
    Rejected { handle_id: String, reason: String },
    /// Demotion API present but runtime wiring not connected for this kind.
    Unavailable { handle_id: String, reason: String },
}

impl DemoteToBackgroundResult {
    pub const fn is_demoted(&self) -> bool {
        matches!(self, Self::Demoted { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub const fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim shell demote product).
    pub fn one_line(&self) -> String {
        match self {
            Self::Demoted {
                handle_id,
                background_id,
                kind,
            } => format!(
                "demote: {} `{}` → background `{}`",
                kind.as_str(),
                handle_id,
                background_id
            ),
            Self::Rejected { handle_id, reason } => {
                format!("demote rejected: `{handle_id}` ({reason})")
            }
            Self::Unavailable { handle_id, reason } => {
                format!("demote unavailable: `{handle_id}` ({reason})")
            }
        }
    }
}

/// Count demote outcomes for operator/CLI surfaces (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DemoteOutcomeSummary {
    pub demoted: usize,
    pub rejected: usize,
    pub unavailable: usize,
    pub total: usize,
}

impl DemoteOutcomeSummary {
    pub fn one_line(&self) -> String {
        format!(
            "demote outcomes: {} demoted, {} rejected, {} unavailable ({} total)",
            self.demoted, self.rejected, self.unavailable, self.total
        )
    }

    pub const fn has_demoted(&self) -> bool {
        self.demoted > 0
    }
}

/// Summarize a batch of demote results for operator surfaces.
pub fn summarize_demote_outcomes(results: &[DemoteToBackgroundResult]) -> DemoteOutcomeSummary {
    let mut summary = DemoteOutcomeSummary {
        total: results.len(),
        ..DemoteOutcomeSummary::default()
    };
    for result in results {
        match result {
            DemoteToBackgroundResult::Demoted { .. } => {
                summary.demoted = summary.demoted.saturating_add(1);
            }
            DemoteToBackgroundResult::Rejected { .. } => {
                summary.rejected = summary.rejected.saturating_add(1);
            }
            DemoteToBackgroundResult::Unavailable { .. } => {
                summary.unavailable = summary.unavailable.saturating_add(1);
            }
        }
    }
    summary
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DemoteError {
    #[error("demote handle_id must be non-empty")]
    EmptyHandle,
}

/// Apply demotion against an injectable registry of demotable handles.
///
/// MVP default: when `is_demotable` returns true, accept and mint a background
/// id; otherwise reject. When `runtime_connected` is false, return structured
/// unavailable (honest for shells until a demote runtime lands).
pub fn apply_demote_to_background<F>(
    request: &DemoteToBackgroundRequest,
    runtime_connected: bool,
    is_demotable: F,
) -> Result<DemoteToBackgroundResult, DemoteError>
where
    F: FnOnce(&str, ForegroundKind) -> bool,
{
    request.validate()?;
    let handle_id = request.handle_id.trim().to_string();

    if !runtime_connected {
        return Ok(DemoteToBackgroundResult::Unavailable {
            handle_id,
            reason: format!(
                "foreground demote runtime not connected for kind `{}`; \
                 use task(run_in_background: true) at spawn time instead",
                request.kind.as_str()
            ),
        });
    }

    if !is_demotable(&handle_id, request.kind) {
        return Ok(DemoteToBackgroundResult::Rejected {
            handle_id: handle_id.clone(),
            reason: format!("handle `{handle_id}` is not a demotable foreground unit"),
        });
    }

    let background_id = format!("bg-demoted-{handle_id}");
    Ok(DemoteToBackgroundResult::Demoted {
        handle_id,
        background_id,
        kind: request.kind,
    })
}

/// Default product policy: task demotion can be wired; shell demotion unavailable.
pub fn default_demote_policy(
    request: &DemoteToBackgroundRequest,
) -> Result<DemoteToBackgroundResult, DemoteError> {
    match request.kind {
        ForegroundKind::Task => apply_demote_to_background(request, true, |_, _| true),
        ForegroundKind::Shell => apply_demote_to_background(request, false, |_, _| false),
    }
}

/// Resolve demotion for a single foreground task handle against live demotable ids.
///
/// `demotable_task_ids` are child request ids currently foreground-blocking a parent.
/// On accept, returns `background_id` equal to the child request id so callers can
/// continue with `background_output(request_id=...)`.
pub fn demote_task_handle_against_registry(
    handle_id: &str,
    demotable_task_ids: &[&str],
) -> Result<DemoteToBackgroundResult, DemoteError> {
    let request = DemoteToBackgroundRequest::new(handle_id.trim(), ForegroundKind::Task);
    apply_demote_to_background(&request, true, |id, kind| {
        kind == ForegroundKind::Task && demotable_task_ids.contains(&id)
    })
    .map(|result| match result {
        DemoteToBackgroundResult::Demoted {
            handle_id, kind, ..
        } => DemoteToBackgroundResult::Demoted {
            background_id: handle_id.clone(),
            handle_id,
            kind,
        },
        other => other,
    })
}

/// Demote each handle against the same demotable registry (bulk operator path).
pub fn demote_task_handles_against_registry(
    handle_ids: &[&str],
    demotable_task_ids: &[&str],
) -> Result<Vec<DemoteToBackgroundResult>, DemoteError> {
    let mut results = Vec::with_capacity(handle_ids.len());
    for handle_id in handle_ids {
        results.push(demote_task_handle_against_registry(
            handle_id,
            demotable_task_ids,
        )?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_handle_is_rejected_at_boundary() {
        // arrange
        // act
        // assert
        let req = DemoteToBackgroundRequest::new("  ", ForegroundKind::Task);
        assert!(matches!(req.validate(), Err(DemoteError::EmptyHandle)));
    }

    #[test]
    fn task_demote_accepts_when_runtime_connected() {
        // arrange
        // act
        // assert
        // Given
        let req = DemoteToBackgroundRequest::new("task-1", ForegroundKind::Task)
            .with_reason("operator demote");

        // When
        let result = apply_demote_to_background(&req, true, |id, kind| {
            id == "task-1" && kind == ForegroundKind::Task
        })
        .unwrap();

        // Then
        match result {
            DemoteToBackgroundResult::Demoted {
                handle_id,
                background_id,
                kind,
            } => {
                assert_eq!(handle_id, "task-1");
                assert!(background_id.contains("task-1"));
                assert_eq!(kind, ForegroundKind::Task);
            }
            other => panic!("expected Demoted, got {other:?}"),
        }
    }

    #[test]
    fn shell_default_policy_is_structured_unavailable() {
        // arrange
        // act
        // assert
        let req = DemoteToBackgroundRequest::new("shell-9", ForegroundKind::Shell);
        let result = default_demote_policy(&req).unwrap();
        assert!(result.is_unavailable());
        match result {
            DemoteToBackgroundResult::Unavailable { reason, .. } => {
                assert!(reason.contains("not connected") || reason.contains("run_in_background"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn unknown_handle_is_rejected() {
        // arrange
        // act
        // assert
        let req = DemoteToBackgroundRequest::new("missing", ForegroundKind::Task);
        let result = apply_demote_to_background(&req, true, |_, _| false).unwrap();
        match result {
            DemoteToBackgroundResult::Rejected { handle_id, reason } => {
                assert_eq!(handle_id, "missing");
                assert!(reason.contains("not a demotable"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn demote_task_handle_against_registry_accepts_known_foreground_id() {
        // arrange
        // act
        // assert
        // Given
        let demotable = ["req-child-1", "req-child-2"];

        // When
        let result = demote_task_handle_against_registry("req-child-1", &demotable).unwrap();

        // Then
        match result {
            DemoteToBackgroundResult::Demoted {
                handle_id,
                background_id,
                kind,
            } => {
                assert_eq!(handle_id, "req-child-1");
                assert_eq!(background_id, "req-child-1");
                assert_eq!(kind, ForegroundKind::Task);
            }
            other => panic!("expected Demoted, got {other:?}"),
        }
    }

    #[test]
    fn demote_task_handle_against_registry_rejects_unknown() {
        // arrange
        // act
        // assert
        let result = demote_task_handle_against_registry("missing", &["req-a"]).unwrap();
        assert!(matches!(result, DemoteToBackgroundResult::Rejected { .. }));
    }

    #[test]
    fn demote_outcome_one_line_and_summary_cover_all_statuses() {
        // arrange
        // act
        // assert
        // Given: demoted, rejected, and unavailable outcomes
        let demoted = demote_task_handle_against_registry("req-a", &["req-a"]).unwrap();
        let rejected = demote_task_handle_against_registry("missing", &["req-a"]).unwrap();
        let unavailable = default_demote_policy(&DemoteToBackgroundRequest::new(
            "shell-1",
            ForegroundKind::Shell,
        ))
        .unwrap();

        // When
        let summary =
            summarize_demote_outcomes(&[demoted.clone(), rejected.clone(), unavailable.clone()]);

        // Then
        assert!(demoted.is_demoted());
        assert!(demoted.one_line().contains("demote: task"));
        assert!(demoted.one_line().contains("req-a"));
        assert!(rejected.is_rejected());
        assert!(rejected.one_line().contains("demote rejected"));
        assert!(unavailable.is_unavailable());
        assert!(unavailable.one_line().contains("demote unavailable"));
        assert_eq!(
            summary,
            DemoteOutcomeSummary {
                demoted: 1,
                rejected: 1,
                unavailable: 1,
                total: 3,
            }
        );
        assert!(summary.has_demoted());
        assert!(summary.one_line().contains("1 demoted"));
        assert!(summary.one_line().contains("1 rejected"));
        assert!(summary.one_line().contains("1 unavailable"));
    }

    #[test]
    fn demote_task_handles_against_registry_batches_mixed_outcomes() {
        // arrange
        // act
        // assert
        // Given
        let demotable = ["req-a", "req-b"];

        // When
        let results =
            demote_task_handles_against_registry(&["req-a", "missing", "req-b"], &demotable)
                .unwrap();
        let summary = summarize_demote_outcomes(&results);

        // Then
        assert_eq!(results.len(), 3);
        assert!(results[0].is_demoted());
        assert!(results[1].is_rejected());
        assert!(results[2].is_demoted());
        assert_eq!(summary.demoted, 2);
        assert_eq!(summary.rejected, 1);
        assert_eq!(summary.total, 3);
    }
}
