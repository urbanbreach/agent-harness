use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundKind {
    Shell,
    Task,
}
impl ForegroundKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Task => "task",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoteToBackgroundRequest {
    pub handle_id: String,
    pub kind: ForegroundKind,
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
    pub fn validate(&self) -> Result<(), DemoteError> {
        if self.handle_id.trim().is_empty() {
            Err(DemoteError::EmptyHandle)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DemoteToBackgroundResult {
    Demoted {
        handle_id: String,
        background_id: String,
        kind: ForegroundKind,
    },
    Rejected {
        handle_id: String,
        reason: String,
    },
    Unavailable {
        handle_id: String,
        reason: String,
    },
}
impl DemoteToBackgroundResult {
    pub fn is_demoted(&self) -> bool {
        matches!(self, Self::Demoted { .. })
    }
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Demoted {
                handle_id,
                background_id,
                kind,
            } => format!(
                "demote: {} {handle_id} demoted (background={background_id})",
                kind.as_str()
            ),
            Self::Rejected { handle_id, reason } => {
                format!("demote: {handle_id} rejected ({reason})")
            }
            Self::Unavailable { handle_id, reason } => {
                format!("demote: {handle_id} unavailable ({reason})")
            }
        }
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoteOutcomeSummary {
    pub demoted: usize,
    pub rejected: usize,
    pub unavailable: usize,
    pub total: usize,
}
impl DemoteOutcomeSummary {
    pub fn has_demoted(&self) -> bool {
        self.demoted > 0
    }
    pub fn one_line(&self) -> String {
        format!(
            "demote outcomes: {} demoted, {} rejected, {} unavailable ({} total)",
            self.demoted, self.rejected, self.unavailable, self.total
        )
    }
}
pub fn summarize_demote_outcomes(results: &[DemoteToBackgroundResult]) -> DemoteOutcomeSummary {
    DemoteOutcomeSummary {
        demoted: results.iter().filter(|r| r.is_demoted()).count(),
        rejected: results.iter().filter(|r| r.is_rejected()).count(),
        unavailable: results.iter().filter(|r| r.is_unavailable()).count(),
        total: results.len(),
    }
}
#[derive(Debug, thiserror::Error)]
pub enum DemoteError {
    #[error("demote handle_id must be non-empty")]
    EmptyHandle,
}
/// The owning runtime must commit the handoff; this function only evaluates its supplied policy.
pub fn apply_demote_to_background<F>(
    request: &DemoteToBackgroundRequest,
    runtime_connected: bool,
    is_demotable: F,
) -> Result<DemoteToBackgroundResult, DemoteError>
where
    F: FnOnce(&str, ForegroundKind) -> bool,
{
    request.validate()?;
    let handle_id = request.handle_id.trim().to_owned();
    if !runtime_connected {
        return Ok(DemoteToBackgroundResult::Unavailable {
            handle_id,
            reason: "foreground demotion requires a connected runtime".into(),
        });
    }
    if !is_demotable(&handle_id, request.kind) {
        return Ok(DemoteToBackgroundResult::Rejected {
            handle_id,
            reason: "handle is not a demotable foreground unit".into(),
        });
    }
    Ok(DemoteToBackgroundResult::Demoted {
        background_id: format!("bg-demoted-{handle_id}"),
        handle_id,
        kind: request.kind,
    })
}
pub fn default_demote_policy(
    request: &DemoteToBackgroundRequest,
) -> Result<DemoteToBackgroundResult, DemoteError> {
    apply_demote_to_background(request, false, |_, _| false)
}
pub fn demote_task_handle_against_registry(
    handle_id: &str,
    demotable_task_ids: &[&str],
) -> Result<DemoteToBackgroundResult, DemoteError> {
    let request = DemoteToBackgroundRequest::new(handle_id, ForegroundKind::Task);
    let mut result =
        apply_demote_to_background(&request, true, |id, _| demotable_task_ids.contains(&id))?;
    if let DemoteToBackgroundResult::Demoted {
        handle_id,
        background_id,
        ..
    } = &mut result
    {
        background_id.clone_from(handle_id);
    }
    Ok(result)
}
pub fn demote_task_handles_against_registry(
    handle_ids: &[&str],
    demotable_task_ids: &[&str],
) -> Result<Vec<DemoteToBackgroundResult>, DemoteError> {
    handle_ids
        .iter()
        .map(|id| demote_task_handle_against_registry(id, demotable_task_ids))
        .collect()
}
