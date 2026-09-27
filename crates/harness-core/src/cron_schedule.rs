use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ScheduleId(String);
impl ScheduleId {
    pub fn parse(value: &str) -> Result<Self, CronScheduleError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(CronScheduleError::EmptyId);
        }
        if value.len() > 256
            || value.chars().any(char::is_control)
            || crate::redact::redact_artifact_text(value) != value
        {
            return Err(CronScheduleError::InvalidId {
                value: "invalid identifier".into(),
            });
        }
        Ok(Self(value.into()))
    }
    pub fn from_static_literal(value: &'static str) -> Self {
        Self(value.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: ScheduleId,
    pub expression: String,
    pub label: Option<String>,
    pub payload_hint: String,
}
impl CronSchedule {
    pub fn one_line(&self) -> String {
        format!(
            "cron schedule `{}` expr=`{}` label=`{}` (executes=false)",
            self.id.as_str(),
            self.expression,
            self.label.as_deref().unwrap_or("(none)")
        )
    }
}
#[derive(Debug, thiserror::Error)]
pub enum CronScheduleError {
    #[error("cron schedule id must be non-empty")]
    EmptyId,
    #[error("cron schedule id is invalid: {value}")]
    InvalidId { value: String },
    #[error("cron expression must be non-empty")]
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
    #[error("schedule exceeds size or count limits")]
    Capacity,
    #[error("cron schedule `{id}` already fired at this civil time")]
    AlreadyFired { id: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedCronExpression {
    pub expression: String,
    pub fields: [String; 5],
}
pub fn validate_cron_expression(
    expression: &str,
) -> Result<ValidatedCronExpression, CronScheduleError> {
    if expression.trim().is_empty() {
        return Err(CronScheduleError::EmptyExpression);
    }
    if expression.len() > 1024 {
        return Err(CronScheduleError::Capacity);
    }
    let fields: Vec<_> = expression.split_whitespace().map(String::from).collect();
    let field_count = fields.len();
    let fields: [String; 5] =
        fields
            .try_into()
            .map_err(|_| CronScheduleError::InvalidFieldCount {
                expression: expression.into(),
                field_count,
            })?;
    for (field, (min, max)) in fields
        .iter()
        .zip([(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)])
    {
        if field_mask(field, min, max).is_none() {
            return Err(CronScheduleError::InvalidField {
                expression: expression.into(),
                field: field.clone(),
            });
        }
    }
    Ok(ValidatedCronExpression {
        expression: fields.join(" "),
        fields,
    })
}
pub(crate) fn field_mask(field: &str, min: u8, max: u8) -> Option<u64> {
    fn number(text: &str) -> Option<u16> {
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    }
    let mut mask = 0;
    for part in field.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => (range, number(step)?),
            None => (part, 1),
        };
        if step == 0 {
            return None;
        }
        let (start, end) = if range == "*" {
            (u16::from(min), u16::from(max))
        } else if let Some((a, b)) = range.split_once('-') {
            (number(a)?, number(b)?)
        } else {
            let n = number(range)?;
            (
                n,
                if part.contains('/') {
                    u16::from(max)
                } else {
                    n
                },
            )
        };
        if start < u16::from(min) || end > u16::from(max) || start > end {
            return None;
        }
        for value in (start..=end).step_by(usize::from(step)) {
            mask |= 1_u64 << value;
        }
    }
    Some(mask)
}
#[derive(Clone, Debug, Default)]
pub struct CronScheduleRegistry {
    schedules: BTreeMap<ScheduleId, CronSchedule>,
}
impl CronScheduleRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, mut schedule: CronSchedule) -> Result<(), CronScheduleError> {
        schedule.id = ScheduleId::parse(schedule.id.as_str())?;
        schedule.expression = validate_cron_expression(&schedule.expression)?.expression;
        if self.schedules.contains_key(&schedule.id) {
            return Err(CronScheduleError::AlreadyRegistered { id: schedule.id.0 });
        }
        if self.schedules.len() >= 1024
            || schedule.payload_hint.len() > 4096
            || schedule
                .label
                .as_ref()
                .is_some_and(|l| l.len() > 256 || l.chars().any(char::is_control))
        {
            return Err(CronScheduleError::Capacity);
        }
        schedule.payload_hint = crate::redact::redact_artifact_text(&schedule.payload_hint);
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
            .ok_or_else(|| CronScheduleError::NotRegistered { id: id.0.clone() })
    }
    pub fn len(&self) -> usize {
        self.schedules.len()
    }
    pub fn is_empty(&self) -> bool {
        self.schedules.is_empty()
    }
    pub fn executor_available(&self) -> bool {
        false
    }
    pub fn summary(&self) -> CronScheduleSummary {
        CronScheduleSummary {
            registered: self.len(),
            with_label: self
                .schedules
                .values()
                .filter(|s| s.label.as_ref().is_some_and(|l| !l.trim().is_empty()))
                .count(),
            executor_available: self.executor_available(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronScheduleSummary {
    pub registered: usize,
    pub with_label: usize,
    pub executor_available: bool,
}
impl CronScheduleSummary {
    pub fn one_line(&self) -> String {
        format!(
            "cron: {} registered ({} labeled; executor_available={})",
            self.registered, self.with_label, self.executor_available
        )
    }
    pub fn has_schedules(&self) -> bool {
        self.registered > 0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CronRegisterOutcome {
    Registered { id: String, expression: String },
    Failed { id: String, reason: String },
}
impl CronRegisterOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Registered { id, expression } => {
                format!("cron register: ok id=`{id}` expr=`{expression}` (executes=false)")
            }
            Self::Failed { id, reason } => format!("cron register: failed id=`{id}` ({reason})"),
        }
    }
}
pub fn register_cron_schedule(
    registry: &mut CronScheduleRegistry,
    schedule: CronSchedule,
) -> CronRegisterOutcome {
    let id = schedule.id.0.clone();
    let expression = schedule.expression.clone();
    match registry.register(schedule) {
        Ok(()) => CronRegisterOutcome::Registered { id, expression },
        Err(e) => CronRegisterOutcome::Failed {
            id,
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            Self::Failed { id, reason } => format!("cron remove: failed id=`{id}` ({reason})"),
        }
    }
}
pub fn remove_cron_schedule(
    registry: &mut CronScheduleRegistry,
    id: &ScheduleId,
) -> CronRemoveOutcome {
    match registry.remove(id) {
        Ok(s) => CronRemoveOutcome::Removed {
            id: s.id.0,
            expression: s.expression,
        },
        Err(e) => CronRemoveOutcome::Failed {
            id: id.0.clone(),
            reason: e.to_string(),
        },
    }
}
