use std::fmt::{Display, Formatter};

use super::model::{DashboardRow, DashboardStatus};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectionKey(String);

impl SelectionKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn try_new(value: impl Into<String>) -> Result<Self, SelectionKeyError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SelectionKeyError::Empty);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SelectionKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKeyError {
    Empty,
}

impl Display for SelectionKeyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("session selection key must be non-empty"),
        }
    }
}

impl std::error::Error for SelectionKeyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardEligibilityRules {
    pub include_active: bool,
    pub include_queued: bool,
    pub include_finished: bool,
    pub include_failed: bool,
    pub include_cancelled: bool,
    pub include_stale: bool,
    pub include_children: bool,
    pub include_background: bool,
    pub include_foreign: bool,
}

impl Default for DashboardEligibilityRules {
    fn default() -> Self {
        Self {
            include_active: true,
            include_queued: true,
            include_finished: true,
            include_failed: true,
            include_cancelled: true,
            include_stale: true,
            include_children: true,
            include_background: true,
            include_foreign: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardEntryEligibility {
    pub is_eligible: bool,
    pub excluded_by: Option<EligibilityExclusion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EligibilityExclusion {
    Active,
    Queued,
    Finished,
    Failed,
    Cancelled,
    Stale,
    Child,
    Background,
    Foreign,
}

pub(crate) fn evaluate(
    row: &DashboardRow,
    rules: &DashboardEligibilityRules,
) -> DashboardEntryEligibility {
    let status_exclusion = match row.status {
        DashboardStatus::Running | DashboardStatus::Streaming if !rules.include_active => {
            Some(EligibilityExclusion::Active)
        }
        DashboardStatus::Queued if !rules.include_queued => Some(EligibilityExclusion::Queued),
        DashboardStatus::Completed if !rules.include_finished => {
            Some(EligibilityExclusion::Finished)
        }
        DashboardStatus::Failed if !rules.include_failed => Some(EligibilityExclusion::Failed),
        DashboardStatus::Cancelled if !rules.include_cancelled => {
            Some(EligibilityExclusion::Cancelled)
        }
        DashboardStatus::Stale if !rules.include_stale => Some(EligibilityExclusion::Stale),
        _ => None,
    };
    let relationship_exclusion = if row.relationship.is_foreign && !rules.include_foreign {
        Some(EligibilityExclusion::Foreign)
    } else if row.relationship.is_background && !rules.include_background {
        Some(EligibilityExclusion::Background)
    } else if row.relationship.is_child && !rules.include_children {
        Some(EligibilityExclusion::Child)
    } else {
        None
    };
    let excluded_by = status_exclusion.or(relationship_exclusion);
    DashboardEntryEligibility {
        is_eligible: excluded_by.is_none(),
        excluded_by,
    }
}
