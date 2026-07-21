//! Recurring schedule execution (fire due schedules + durable fire journal).
//!
//! Evaluates five-field cron expressions against a civil time, records fires
//! with real side effects (journal append), and marks the product as executing.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cron_schedule::{CronSchedule, CronScheduleError, CronScheduleRegistry, ScheduleId};

/// Civil time components used for cron field matching (local or injected clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CronCivilTime {
    pub minute: u8,
    pub hour: u8,
    pub day_of_month: u8,
    pub month: u8,
    /// 0 = Sunday … 6 = Saturday (classic cron weekday).
    pub day_of_week: u8,
}

impl CronCivilTime {
    pub const fn new(
        minute: u8,
        hour: u8,
        day_of_month: u8,
        month: u8,
        day_of_week: u8,
    ) -> Result<Self, CronScheduleError> {
        if minute > 59 {
            return Err(CronScheduleError::InvalidCivilTime {
                field: "minute",
                value: minute as u16,
            });
        }
        if hour > 23 {
            return Err(CronScheduleError::InvalidCivilTime {
                field: "hour",
                value: hour as u16,
            });
        }
        if day_of_month < 1 || day_of_month > 31 {
            return Err(CronScheduleError::InvalidCivilTime {
                field: "day_of_month",
                value: day_of_month as u16,
            });
        }
        if month < 1 || month > 12 {
            return Err(CronScheduleError::InvalidCivilTime {
                field: "month",
                value: month as u16,
            });
        }
        if day_of_week > 6 {
            return Err(CronScheduleError::InvalidCivilTime {
                field: "day_of_week",
                value: day_of_week as u16,
            });
        }
        Ok(Self {
            minute,
            hour,
            day_of_month,
            month,
            day_of_week,
        })
    }
}

/// One recorded schedule fire (product side effect).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronFireRecord {
    pub schedule_id: String,
    pub expression: String,
    pub payload_hint: String,
    pub civil: CronFireCivil,
    pub journal_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronFireCivil {
    pub minute: u8,
    pub hour: u8,
    pub day_of_month: u8,
    pub month: u8,
    pub day_of_week: u8,
}

impl From<CronCivilTime> for CronFireCivil {
    fn from(value: CronCivilTime) -> Self {
        Self {
            minute: value.minute,
            hour: value.hour,
            day_of_month: value.day_of_month,
            month: value.month,
            day_of_week: value.day_of_week,
        }
    }
}

/// Result of evaluating due schedules at one civil time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronFireBatch {
    pub fired: Vec<CronFireRecord>,
    pub skipped: usize,
    pub journal_path: Option<String>,
}

impl CronFireBatch {
    pub fn one_line(&self) -> String {
        format!(
            "cron fire: fired={} skipped={} journal={}",
            self.fired.len(),
            self.skipped,
            self.journal_path.as_deref().unwrap_or("(memory)")
        )
    }
}

/// Match a single cron field against a value (supports `*`, lists, ranges, steps).
pub fn field_matches(field: &str, value: u8) -> bool {
    if field == "*" {
        return true;
    }
    for part in field.split(',') {
        if part_matches(part, value) {
            return true;
        }
    }
    false
}

fn part_matches(part: &str, value: u8) -> bool {
    if let Some((range, step)) = part.split_once('/') {
        let step: u8 = match step.parse() {
            Ok(s) if s > 0 => s,
            _ => return false,
        };
        let (start, end) = if range == "*" {
            (0u8, 59u8)
        } else if let Some((a, b)) = range.split_once('-') {
            match (a.parse::<u8>(), b.parse::<u8>()) {
                (Ok(a), Ok(b)) => (a, b),
                _ => return false,
            }
        } else {
            match range.parse::<u8>() {
                Ok(a) => (a, 59),
                Err(_) => return false,
            }
        };
        if value < start || value > end {
            return false;
        }
        return (value - start) % step == 0;
    }
    if let Some((a, b)) = part.split_once('-') {
        return match (a.parse::<u8>(), b.parse::<u8>()) {
            (Ok(a), Ok(b)) => value >= a && value <= b,
            _ => false,
        };
    }
    part.parse::<u8>().is_ok_and(|n| n == value)
}

/// True when the schedule's five-field expression matches `now`.
pub fn schedule_is_due(
    schedule: &CronSchedule,
    now: CronCivilTime,
) -> Result<bool, CronScheduleError> {
    let validated = crate::cron_schedule::validate_cron_expression(&schedule.expression)?;
    let [minute, hour, dom, month, dow] = validated.fields;
    Ok(field_matches(&minute, now.minute)
        && field_matches(&hour, now.hour)
        && field_matches(&dom, now.day_of_month)
        && field_matches(&month, now.month)
        && field_matches(&dow, now.day_of_week))
}

/// In-memory + optional durable journal executor for due cron schedules.
#[derive(Debug, Clone, Default)]
pub struct CronExecutor {
    fires: Vec<CronFireRecord>,
    next_seq: u64,
    journal_dir: Option<PathBuf>,
}

impl CronExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_journal_dir(journal_dir: impl Into<PathBuf>) -> Self {
        Self {
            fires: Vec::new(),
            next_seq: 0,
            journal_dir: Some(journal_dir.into()),
        }
    }

    pub fn fire_count(&self) -> usize {
        self.fires.len()
    }

    pub fn fires(&self) -> &[CronFireRecord] {
        &self.fires
    }

    pub const fn executes_schedules() -> bool {
        true
    }

    /// Evaluate all registered schedules at `now` and fire each due one once.
    pub fn fire_due(
        &mut self,
        registry: &CronScheduleRegistry,
        now: CronCivilTime,
    ) -> Result<CronFireBatch, CronScheduleError> {
        let mut fired = Vec::new();
        let mut skipped = 0usize;
        for schedule in registry.list() {
            if schedule_is_due(schedule, now)? {
                let record = self.record_fire(schedule, now)?;
                fired.push(record);
            } else {
                skipped = skipped.saturating_add(1);
            }
        }
        let journal_path = self
            .journal_dir
            .as_ref()
            .map(|p| p.join("cron-fires.jsonl").display().to_string());
        Ok(CronFireBatch {
            fired,
            skipped,
            journal_path,
        })
    }

    /// Fire one schedule by id if due at `now` (fail closed if not registered / not due).
    pub fn fire_one_if_due(
        &mut self,
        registry: &CronScheduleRegistry,
        id: &ScheduleId,
        now: CronCivilTime,
    ) -> Result<CronFireRecord, CronScheduleError> {
        let schedule = registry
            .get(id)
            .ok_or_else(|| CronScheduleError::NotRegistered {
                id: id.as_str().to_string(),
            })?;
        if !schedule_is_due(schedule, now)? {
            return Err(CronScheduleError::NotDue {
                id: id.as_str().to_string(),
                expression: schedule.expression.clone(),
            });
        }
        self.record_fire(schedule, now)
    }

    fn record_fire(
        &mut self,
        schedule: &CronSchedule,
        now: CronCivilTime,
    ) -> Result<CronFireRecord, CronScheduleError> {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let record = CronFireRecord {
            schedule_id: schedule.id.as_str().to_string(),
            expression: schedule.expression.clone(),
            payload_hint: schedule.payload_hint.clone(),
            civil: now.into(),
            journal_seq: seq,
        };
        if let Some(dir) = &self.journal_dir {
            append_fire_journal(dir, &record)?;
        }
        self.fires.push(record.clone());
        Ok(record)
    }
}

fn append_fire_journal(dir: &Path, record: &CronFireRecord) -> Result<(), CronScheduleError> {
    fs::create_dir_all(dir).map_err(|err| CronScheduleError::JournalIo {
        path: dir.display().to_string(),
        reason: err.to_string(),
    })?;
    let path = dir.join("cron-fires.jsonl");
    let line = serde_json::to_string(record).map_err(|err| CronScheduleError::JournalIo {
        path: path.display().to_string(),
        reason: err.to_string(),
    })?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| CronScheduleError::JournalIo {
            path: path.display().to_string(),
            reason: err.to_string(),
        })?;
    writeln!(file, "{line}").map_err(|err| CronScheduleError::JournalIo {
        path: path.display().to_string(),
        reason: err.to_string(),
    })?;
    Ok(())
}

/// Product path: register schedules, fire due ones, return batch + journal.
pub fn run_cron_execution_product(
    registry: &mut CronScheduleRegistry,
    executor: &mut CronExecutor,
    schedules: Vec<CronSchedule>,
    now: CronCivilTime,
) -> Result<CronFireBatch, CronScheduleError> {
    for schedule in schedules {
        registry.register(schedule)?;
    }
    executor.fire_due(registry, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron_schedule::{register_cron_schedule, CronSchedule, ScheduleId};

    fn sample(id: &str, expr: &str) -> CronSchedule {
        CronSchedule {
            id: ScheduleId::parse(id).unwrap(),
            expression: expr.to_string(),
            label: Some(id.to_string()),
            payload_hint: format!("payload-{id}"),
        }
    }

    #[test]
    fn field_matches_star_list_range_and_step() {
        // arrange
        // act
        // assert
        assert!(field_matches("*", 7));
        assert!(field_matches("1,5,9", 5));
        assert!(!field_matches("1,5,9", 3));
        assert!(field_matches("1-5", 3));
        assert!(!field_matches("1-5", 6));
        assert!(field_matches("*/15", 0));
        assert!(field_matches("*/15", 30));
        assert!(!field_matches("*/15", 7));
        assert!(field_matches("10-20/5", 15));
        assert!(!field_matches("10-20/5", 12));
    }

    #[test]
    fn schedule_is_due_matches_weekday_morning() {
        // arrange
        let schedule = sample("weekday", "0 9 * * 1-5");
        let weekday_morning = CronCivilTime::new(0, 9, 15, 7, 3).unwrap();
        let weekend = CronCivilTime::new(0, 9, 15, 7, 0).unwrap();
        let wrong_hour = CronCivilTime::new(0, 10, 15, 7, 3).unwrap();

        // act
        // assert
        assert!(schedule_is_due(&schedule, weekday_morning).unwrap());
        assert!(!schedule_is_due(&schedule, weekend).unwrap());
        assert!(!schedule_is_due(&schedule, wrong_hour).unwrap());
    }

    #[test]
    fn fire_due_records_and_journals_side_effects() {
        // arrange
        let dir = std::env::temp_dir().join(format!("harness-cron-journal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut registry = CronScheduleRegistry::new();
        let mut executor = CronExecutor::with_journal_dir(&dir);
        register_cron_schedule(&mut registry, sample("due", "30 14 * * *"));
        register_cron_schedule(&mut registry, sample("not-due", "0 0 * * *"));
        let now = CronCivilTime::new(30, 14, 1, 1, 3).unwrap();

        // act
        let batch = executor.fire_due(&registry, now).unwrap();

        // assert
        assert_eq!(batch.fired.len(), 1);
        assert_eq!(batch.skipped, 1);
        assert_eq!(batch.fired[0].schedule_id, "due");
        assert_eq!(batch.fired[0].payload_hint, "payload-due");
        assert_eq!(executor.fire_count(), 1);
        assert!(CronExecutor::executes_schedules());
        let journal = dir.join("cron-fires.jsonl");
        let body = fs::read_to_string(&journal).expect("journal written");
        assert!(body.contains("\"schedule_id\":\"due\""));
        assert!(body.contains("payload-due"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fire_journal_accumulates_across_executor_instances() {
        // arrange
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join("cron-fires.jsonl");
        let mut registry = CronScheduleRegistry::new();
        register_cron_schedule(&mut registry, sample("morning", "30 14 * * *"));
        register_cron_schedule(&mut registry, sample("evening", "0 20 * * *"));

        // act — two independent executor passes, like separate daemon ticks
        let first = CronExecutor::with_journal_dir(dir.path())
            .fire_due(&registry, CronCivilTime::new(30, 14, 1, 1, 3).unwrap())
            .unwrap();
        let second = CronExecutor::with_journal_dir(dir.path())
            .fire_due(&registry, CronCivilTime::new(0, 20, 1, 1, 3).unwrap())
            .unwrap();

        // assert — both fires land in one durable append-only journal
        assert_eq!(first.fired.len(), 1);
        assert_eq!(second.fired.len(), 1);
        let body = fs::read_to_string(&journal).expect("journal written");
        assert!(body.contains("\"schedule_id\":\"morning\""));
        assert!(body.contains("\"schedule_id\":\"evening\""));
        assert_eq!(body.lines().count(), 2);
    }

    #[test]
    fn fire_one_if_due_fails_closed_when_not_due() {
        // arrange
        let mut registry = CronScheduleRegistry::new();
        let mut executor = CronExecutor::new();
        registry.register(sample("nightly", "0 0 * * *")).unwrap();
        let id = ScheduleId::parse("nightly").unwrap();
        let now = CronCivilTime::new(30, 12, 1, 1, 1).unwrap();

        // act
        // assert
        let err = executor.fire_one_if_due(&registry, &id, now).unwrap_err();
        assert!(matches!(err, CronScheduleError::NotDue { .. }));
        assert_eq!(executor.fire_count(), 0);
    }

    #[test]
    fn run_cron_execution_product_registers_and_fires() {
        // arrange
        let mut registry = CronScheduleRegistry::new();
        let mut executor = CronExecutor::new();
        let now = CronCivilTime::new(0, 0, 1, 1, 4).unwrap();

        // act
        let batch = run_cron_execution_product(
            &mut registry,
            &mut executor,
            vec![sample("a", "0 0 * * *"), sample("b", "15 3 * * *")],
            now,
        )
        .unwrap();

        // assert
        assert_eq!(registry.len(), 2);
        assert_eq!(batch.fired.len(), 1);
        assert_eq!(batch.fired[0].schedule_id, "a");
        assert!(batch.one_line().contains("fired=1"));
    }
}
