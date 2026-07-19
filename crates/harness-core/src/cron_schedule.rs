//! Recurring schedule / cron registry + execution product.
//!
//! Registration stores validated five-field expressions. Execution lives in
//! [`crate::cron_execute`]: due matching, fire records, and optional durable
//! journal side effects (`executes_schedules = true`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable schedule identifier (operator-facing).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ScheduleId(String);

impl ScheduleId {
    pub fn parse(value: &str) -> Result<Self, CronScheduleError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(CronScheduleError::EmptyId);
        }
        if trimmed.chars().any(|ch| ch.is_control()) {
            return Err(CronScheduleError::InvalidId {
                value: value.to_string(),
            });
        }
        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A registered recurring schedule definition (not an active timer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: ScheduleId,
    /// Five-field cron expression (stored as text; not executed by this MVP).
    pub expression: String,
    /// Optional human label.
    pub label: Option<String>,
    /// Payload hint for a future executor (opaque to this registry).
    pub payload_hint: String,
}

impl CronSchedule {
    /// Operator-facing one-line diagnostics.
    pub fn one_line(&self) -> String {
        let label = self
            .label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("(none)");
        format!(
            "cron schedule `{}` expr=`{}` label=`{}` (executes=true)",
            self.id.as_str(),
            self.expression,
            label
        )
    }
}

/// Fail-closed registry errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CronScheduleError {
    #[error("cron schedule id must be non-empty after trim")]
    EmptyId,
    #[error("cron schedule id is invalid: {value}")]
    InvalidId { value: String },
    #[error("cron expression must be non-empty after trim")]
    EmptyExpression,
    #[error("cron expression must have exactly 5 fields (got {field_count}): {expression}")]
    InvalidFieldCount {
        expression: String,
        field_count: usize,
    },
    #[error("cron expression field `{field}` is invalid in `{expression}`")]
    InvalidField { expression: String, field: String },
    #[error("cron schedule `{id}` is already registered")]
    AlreadyRegistered { id: String },
    #[error("cron schedule `{id}` is not registered")]
    NotRegistered { id: String },
    #[error("cron schedule `{id}` is not due for expression `{expression}`")]
    NotDue { id: String, expression: String },
    #[error("invalid civil time field `{field}` value {value}")]
    InvalidCivilTime { field: &'static str, value: u16 },
    #[error("cron fire journal I/O failed at `{path}`: {reason}")]
    JournalIo { path: String, reason: String },
}

/// Validated five-field cron expression text (structure only; not executed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedCronExpression {
    pub expression: String,
    pub fields: [String; 5],
}

/// Validate a classic five-field cron expression shape.
///
/// Checks whitespace-separated field count and a conservative character set
/// (`0-9`, `*`, `/`, `-`, `,`). Does **not** evaluate ranges, steps, or next-fire
/// times, and does **not** schedule execution.
pub fn validate_cron_expression(
    expression: &str,
) -> Result<ValidatedCronExpression, CronScheduleError> {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return Err(CronScheduleError::EmptyExpression);
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(CronScheduleError::InvalidFieldCount {
            expression: trimmed.to_string(),
            field_count: parts.len(),
        });
    }
    let mut fields = [
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ];
    for (idx, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.chars().all(is_allowed_cron_field_char) {
            return Err(CronScheduleError::InvalidField {
                expression: trimmed.to_string(),
                field: (*part).to_string(),
            });
        }
        fields[idx] = (*part).to_string();
    }
    Ok(ValidatedCronExpression {
        expression: trimmed.to_string(),
        fields,
    })
}

fn is_allowed_cron_field_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '*' | '/' | '-' | ',')
}

/// In-memory foundation registry for recurring schedules.
///
/// Registration is pure bookkeeping. Callers must not treat presence as proof
/// that a timer is firing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CronScheduleRegistry {
    schedules: BTreeMap<ScheduleId, CronSchedule>,
}

impl CronScheduleRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, schedule: CronSchedule) -> Result<(), CronScheduleError> {
        let validated = validate_cron_expression(&schedule.expression)?;
        if self.schedules.contains_key(&schedule.id) {
            return Err(CronScheduleError::AlreadyRegistered {
                id: schedule.id.as_str().to_string(),
            });
        }
        let mut schedule = schedule;
        schedule.expression = validated.expression;
        self.schedules.insert(schedule.id.clone(), schedule);
        Ok(())
    }

    pub fn get(&self, id: &ScheduleId) -> Option<&CronSchedule> {
        self.schedules.get(id)
    }

    pub fn list(&self) -> Vec<&CronSchedule> {
        self.schedules.values().collect()
    }

    pub fn remove(&mut self, id: &ScheduleId) -> Result<CronSchedule, CronScheduleError> {
        self.schedules
            .remove(id)
            .ok_or_else(|| CronScheduleError::NotRegistered {
                id: id.as_str().to_string(),
            })
    }

    pub fn len(&self) -> usize {
        self.schedules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.schedules.is_empty()
    }

    /// Product executes due schedules via [`crate::cron_execute::CronExecutor`].
    pub const fn executes_schedules(&self) -> bool {
        true
    }

    /// Operator-facing counts for registered schedules (diagnostics only).
    pub fn summary(&self) -> CronScheduleSummary {
        let mut with_label = 0usize;
        for schedule in self.schedules.values() {
            if schedule
                .label
                .as_ref()
                .is_some_and(|label| !label.trim().is_empty())
            {
                with_label = with_label.saturating_add(1);
            }
        }
        CronScheduleSummary {
            registered: self.schedules.len(),
            with_label,
            executes_schedules: self.executes_schedules(),
        }
    }
}

/// Operator-facing counts for a cron schedule registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CronScheduleSummary {
    pub registered: usize,
    pub with_label: usize,
    /// True when the product executor path is available.
    pub executes_schedules: bool,
}

impl CronScheduleSummary {
    pub fn one_line(&self) -> String {
        format!(
            "cron: {} registered ({} labeled; executes={})",
            self.registered, self.with_label, self.executes_schedules
        )
    }

    pub const fn has_schedules(&self) -> bool {
        self.registered > 0
    }
}

/// Result of a single schedule registration attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CronRegisterOutcome {
    Registered { id: String, expression: String },
    Failed { id: String, reason: String },
}

impl CronRegisterOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Registered { id, expression } => {
                format!("cron register: ok id=`{id}` expr=`{expression}` (executes=true)")
            }
            Self::Failed { id, reason } => {
                format!("cron register: failed id=`{id}` ({reason})")
            }
        }
    }
}

/// Register a schedule and return a structured operator-facing outcome.
pub fn register_cron_schedule(
    registry: &mut CronScheduleRegistry,
    schedule: CronSchedule,
) -> CronRegisterOutcome {
    let id = schedule.id.as_str().to_string();
    let expression = schedule.expression.clone();
    match registry.register(schedule) {
        Ok(()) => CronRegisterOutcome::Registered { id, expression },
        Err(err) => CronRegisterOutcome::Failed {
            id,
            reason: err.to_string(),
        },
    }
}

/// Structured operator-facing outcome for removing a cron schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CronRemoveOutcome {
    Removed { id: String, expression: String },
    Failed { id: String, reason: String },
}

impl CronRemoveOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Removed { id, expression } => {
                format!("cron remove: ok id=`{id}` expr=`{expression}`")
            }
            Self::Failed { id, reason } => {
                format!("cron remove: failed id=`{id}` ({reason})")
            }
        }
    }
}

/// Remove a schedule by id and return a structured operator-facing outcome.
pub fn remove_cron_schedule(
    registry: &mut CronScheduleRegistry,
    id: &ScheduleId,
) -> CronRemoveOutcome {
    let id_str = id.as_str().to_string();
    match registry.remove(id) {
        Ok(schedule) => CronRemoveOutcome::Removed {
            id: id_str,
            expression: schedule.expression,
        },
        Err(err) => CronRemoveOutcome::Failed {
            id: id_str,
            reason: err.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> CronSchedule {
        CronSchedule {
            id: ScheduleId::parse(id).unwrap(),
            expression: "0 9 * * 1-5".to_string(),
            label: Some("weekday morning".to_string()),
            payload_hint: "run doctor".to_string(),
        }
    }

    #[test]
    fn register_list_remove_round_trip() {
        // arrange
        // act
        // assert
        // Given
        let mut registry = CronScheduleRegistry::new();

        // When
        registry.register(sample("weekday-doctor")).unwrap();

        // Then
        assert_eq!(registry.len(), 1);
        assert!(registry.executes_schedules());
        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id.as_str(), "weekday-doctor");
        assert_eq!(listed[0].expression, "0 9 * * 1-5");

        let id = ScheduleId::parse("weekday-doctor").unwrap();
        let removed = registry.remove(&id).unwrap();
        assert_eq!(removed.id.as_str(), "weekday-doctor");
        assert!(registry.is_empty());
    }

    #[test]
    fn duplicate_and_empty_expression_fail_closed() {
        // arrange
        // act
        // assert
        let mut registry = CronScheduleRegistry::new();
        registry.register(sample("once")).unwrap();
        assert!(matches!(
            registry.register(sample("once")),
            Err(CronScheduleError::AlreadyRegistered { .. })
        ));

        let bad = CronSchedule {
            id: ScheduleId::parse("empty-expr").unwrap(),
            expression: "   ".to_string(),
            label: None,
            payload_hint: "x".to_string(),
        };
        assert!(matches!(
            registry.register(bad),
            Err(CronScheduleError::EmptyExpression)
        ));
        assert!(matches!(
            ScheduleId::parse("  "),
            Err(CronScheduleError::EmptyId)
        ));
    }

    #[test]
    fn remove_missing_is_error() {
        // arrange
        // act
        // assert
        let mut registry = CronScheduleRegistry::new();
        let id = ScheduleId::parse("missing").unwrap();
        assert!(matches!(
            registry.remove(&id),
            Err(CronScheduleError::NotRegistered { .. })
        ));
    }

    #[test]
    fn remove_cron_schedule_missing_is_failed_outcome() {
        // arrange
        // act
        // assert
        let mut registry = CronScheduleRegistry::new();
        let id = ScheduleId::parse("missing-id").unwrap();
        let outcome = remove_cron_schedule(&mut registry, &id);
        match &outcome {
            CronRemoveOutcome::Failed { id, reason } => {
                assert_eq!(id, "missing-id");
                assert!(!reason.is_empty());
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(outcome.one_line().contains("cron remove: failed"));
    }

    #[test]
    fn remove_cron_schedule_ok_after_register() {
        // arrange
        // act
        // assert
        let mut registry = CronScheduleRegistry::new();
        let schedule = sample("to-remove");
        let reg = register_cron_schedule(&mut registry, schedule);
        assert!(matches!(reg, CronRegisterOutcome::Registered { .. }));
        let id = ScheduleId::parse("to-remove").unwrap();
        let outcome = remove_cron_schedule(&mut registry, &id);
        match &outcome {
            CronRemoveOutcome::Removed { id, expression } => {
                assert_eq!(id, "to-remove");
                assert!(!expression.is_empty());
            }
            other => panic!("expected Removed, got {other:?}"),
        }
        assert!(registry.is_empty());
        assert!(outcome.one_line().contains("cron remove: ok"));
    }

    #[test]
    fn validate_cron_expression_accepts_five_field_shape() {
        // arrange
        // act
        // assert
        // Given / When
        let validated = validate_cron_expression("  0 9 * * 1-5  ").unwrap();

        // Then
        assert_eq!(validated.expression, "0 9 * * 1-5");
        assert_eq!(
            validated.fields,
            [
                "0".to_string(),
                "9".to_string(),
                "*".to_string(),
                "*".to_string(),
                "1-5".to_string()
            ]
        );
    }

    #[test]
    fn validate_cron_expression_rejects_wrong_field_count_and_bad_chars() {
        // arrange
        // act
        // assert
        assert!(matches!(
            validate_cron_expression("0 9 * *"),
            Err(CronScheduleError::InvalidFieldCount { field_count: 4, .. })
        ));
        assert!(matches!(
            validate_cron_expression("0 9 * * 1-5 extra"),
            Err(CronScheduleError::InvalidFieldCount { field_count: 6, .. })
        ));
        assert!(matches!(
            validate_cron_expression("0 9 * * mon"),
            Err(CronScheduleError::InvalidField { field, .. }) if field == "mon"
        ));
        assert!(matches!(
            validate_cron_expression("   "),
            Err(CronScheduleError::EmptyExpression)
        ));
    }

    #[test]
    fn register_rejects_invalid_expression_structure() {
        // arrange
        // act
        // assert
        // Given
        let mut registry = CronScheduleRegistry::new();
        let bad = CronSchedule {
            id: ScheduleId::parse("bad-shape").unwrap(),
            expression: "every day".to_string(),
            label: None,
            payload_hint: "x".to_string(),
        };

        // When
        let err = registry.register(bad).unwrap_err();

        // Then
        assert!(matches!(
            err,
            CronScheduleError::InvalidFieldCount { field_count: 2, .. }
        ));
        assert!(registry.is_empty());
        assert!(registry.executes_schedules());
    }

    #[test]
    fn cron_schedule_summary_counts_registered_and_labeled() {
        // arrange
        // act
        // assert
        // Given: one labeled schedule and one unlabeled schedule
        let mut registry = CronScheduleRegistry::new();
        registry.register(sample("weekday-doctor")).unwrap();
        registry
            .register(CronSchedule {
                id: ScheduleId::parse("nightly").unwrap(),
                expression: "0 0 * * *".to_string(),
                label: None,
                payload_hint: "nightly scan".to_string(),
            })
            .unwrap();

        // When
        let summary = registry.summary();

        // Then
        assert_eq!(
            summary,
            CronScheduleSummary {
                registered: 2,
                with_label: 1,
                executes_schedules: true,
            }
        );
        assert!(summary.has_schedules());
        assert!(summary.one_line().contains("2 registered"));
        assert!(summary.one_line().contains("1 labeled"));
        assert!(summary.one_line().contains("executes=true"));
        assert_eq!(CronScheduleRegistry::new().summary().registered, 0);
    }

    #[test]
    fn multi_schedule_register_remove_list_and_label_summary() {
        // arrange
        // act
        // assert
        // Given: empty registry
        let mut registry = CronScheduleRegistry::new();
        let labeled = |id: &str, expr: &str, label: &str| CronSchedule {
            id: ScheduleId::parse(id).unwrap(),
            expression: expr.to_string(),
            label: Some(label.to_string()),
            payload_hint: format!("payload-{id}"),
        };

        // When: multi-register (probe)/(probe-2)/(probe-3) labeled + one unlabeled
        let first = register_cron_schedule(&mut registry, labeled("(probe)", "0 * * * *", "probe"));
        let second =
            register_cron_schedule(&mut registry, labeled("(probe-2)", "30 * * * *", "probe-2"));
        let third = register_cron_schedule(
            &mut registry,
            labeled("(probe-3)", "15 */2 * * *", "probe-3"),
        );
        let unlabeled = register_cron_schedule(
            &mut registry,
            CronSchedule {
                id: ScheduleId::parse("(probe-4)").unwrap(),
                expression: "5 3 * * 1".to_string(),
                label: None,
                payload_hint: "(probe-4-unlabeled)".to_string(),
            },
        );

        // Then: register outcomes succeed; multi-list preserves labels; executes=true honesty
        assert!(matches!(first, CronRegisterOutcome::Registered { .. }));
        assert!(matches!(second, CronRegisterOutcome::Registered { .. }));
        assert!(matches!(
            third,
            CronRegisterOutcome::Registered { id, .. } if id == "(probe-3)"
        ));
        assert!(matches!(
            unlabeled,
            CronRegisterOutcome::Registered { id, .. } if id == "(probe-4)"
        ));
        assert_eq!(registry.list().len(), 4);
        assert!(registry.list().iter().any(|s| {
            s.id.as_str() == "(probe-2)"
                && s.label.as_deref() == Some("probe-2")
                && s.one_line().contains("executes=true")
        }));
        assert!(registry.executes_schedules());

        // When: remove first probe
        let probe_id = ScheduleId::parse("(probe)").unwrap();
        let removed = remove_cron_schedule(&mut registry, &probe_id);

        // Then: remaining registered>=2 labeled>=2; first listed is probe-2
        assert!(matches!(
            removed,
            CronRemoveOutcome::Removed { id, .. } if id == "(probe)"
        ));
        let summary = registry.summary();
        assert!(
            summary.registered >= 2 && summary.with_label >= 2,
            "expected multi-schedule after remove: {summary:?}"
        );
        assert!(summary.executes_schedules);
        assert_eq!(
            registry.list().first().map(|s| s.id.as_str()),
            Some("(probe-2)")
        );
        assert!(registry
            .get(&ScheduleId::parse("(probe-3)").unwrap())
            .is_some());
        assert!(registry.get(&probe_id).is_none());
    }
}
