//! Explicit civil-time evaluation and durable receipts. This module does not run commands or timers.
use crate::{cron_schedule::*, store};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CronCivilTime {
    pub minute: u8,
    pub hour: u8,
    pub day_of_month: u8,
    pub month: u8,
    pub day_of_week: u8,
}
impl CronCivilTime {
    pub fn new(
        minute: u8,
        hour: u8,
        day_of_month: u8,
        month: u8,
        day_of_week: u8,
    ) -> Result<Self, CronScheduleError> {
        for (field, value, min, max) in [
            ("minute", minute, 0, 59),
            ("hour", hour, 0, 23),
            ("day_of_month", day_of_month, 1, 31),
            ("month", month, 1, 12),
            ("day_of_week", day_of_week, 0, 6),
        ] {
            if !(min..=max).contains(&value) {
                return Err(CronScheduleError::InvalidCivilTime {
                    field,
                    value: u16::from(value),
                });
            }
        }
        let days = match month {
            2 => 29,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if day_of_month > days {
            return Err(CronScheduleError::InvalidCivilTime {
                field: "day_of_month",
                value: u16::from(day_of_month),
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
    fn validate(self) -> Result<(), CronScheduleError> {
        Self::new(
            self.minute,
            self.hour,
            self.day_of_month,
            self.month,
            self.day_of_week,
        )
        .map(|_| ())
    }
}
pub type CronFireCivil = CronCivilTime;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronFireRecord {
    pub schedule_id: String,
    pub expression: String,
    pub payload_hint: String,
    pub civil: CronFireCivil,
    pub journal_seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
pub fn field_matches(field: &str, value: u8) -> bool {
    value < 64 && field_mask(field, 0, 63).is_some_and(|mask| mask & (1_u64 << value) != 0)
}
pub fn schedule_is_due(
    schedule: &CronSchedule,
    now: CronCivilTime,
) -> Result<bool, CronScheduleError> {
    now.validate()?;
    let fields = validate_cron_expression(&schedule.expression)?.fields;
    let ranges = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
    let matches = |index: usize, value: u8| {
        let (min, max) = ranges[index];
        field_mask(&fields[index], min, max).is_some_and(|mask| mask & (1_u64 << value) != 0)
    };
    let day = matches(2, now.day_of_month);
    let weekday = matches(4, now.day_of_week) || (now.day_of_week == 0 && matches(4, 7));
    // Cron uses OR when both day fields are restricted; a wildcard/step keeps AND semantics.
    let matches_day = if fields[2].contains('*') || fields[4].contains('*') {
        day && weekday
    } else {
        day || weekday
    };
    Ok(matches(0, now.minute) && matches(1, now.hour) && matches(3, now.month) && matches_day)
}
#[derive(Debug, Default)]
pub struct CronExecutor {
    fires: Vec<CronFireRecord>,
    journal_dir: Option<PathBuf>,
}
impl CronExecutor {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_journal_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            fires: Vec::new(),
            journal_dir: Some(dir.into()),
        }
    }
    pub fn fire_count(&self) -> usize {
        self.fires.len()
    }
    pub fn fires(&self) -> &[CronFireRecord] {
        &self.fires
    }
    /// Explicit evaluation is available; registration never starts a background timer.
    pub const fn executes_schedules() -> bool {
        true
    }
    pub fn restart_from_journal(&mut self) -> Result<usize, CronScheduleError> {
        if let Some(dir) = &self.journal_dir {
            self.fires = read_journal(&dir.join("cron-fires.jsonl"))?.0;
        }
        Ok(self.fires.len())
    }
    pub fn fire_due(
        &mut self,
        registry: &CronScheduleRegistry,
        now: CronCivilTime,
    ) -> Result<CronFireBatch, CronScheduleError> {
        self.fire(&registry.list(), now)
    }
    pub fn fire_one_if_due(
        &mut self,
        registry: &CronScheduleRegistry,
        id: &ScheduleId,
        now: CronCivilTime,
    ) -> Result<CronFireRecord, CronScheduleError> {
        let schedule = registry
            .get(id)
            .ok_or_else(|| CronScheduleError::NotRegistered {
                id: id.as_str().into(),
            })?;
        if !schedule_is_due(schedule, now)? {
            return Err(CronScheduleError::NotDue {
                id: id.as_str().into(),
                expression: schedule.expression.clone(),
            });
        }
        self.fire(&[schedule], now)?
            .fired
            .into_iter()
            .next()
            .ok_or_else(|| CronScheduleError::AlreadyFired {
                id: id.as_str().into(),
            })
    }
    fn fire(
        &mut self,
        schedules: &[&CronSchedule],
        now: CronCivilTime,
    ) -> Result<CronFireBatch, CronScheduleError> {
        now.validate()?;
        let path = self
            .journal_dir
            .as_ref()
            .map(|d| d.join("cron-fires.jsonl"));
        let mut batch = CronFireBatch {
            fired: Vec::new(),
            skipped: schedules.len(),
            journal_path: path.as_ref().map(|p| p.display().to_string()),
        };
        let mut due = Vec::new();
        for &schedule in schedules {
            if schedule_is_due(schedule, now)? {
                due.push(schedule);
            }
        }
        if due.is_empty() {
            return Ok(batch);
        }
        let _lock = path
            .as_ref()
            .map(|p| store::lock_private_parent(p).map_err(|e| journal_error(p, &e.to_string())))
            .transpose()?;
        let (mut records, mut bytes) = match &path {
            Some(p) => read_journal(p)?,
            None => (self.fires.clone(), Vec::new()),
        };
        // ponytail: the legacy API has no year; receipts identify supplied civil tuples, not an autonomous annual scheduler.
        let seen: HashSet<_> = records
            .iter()
            .map(|r| (r.schedule_id.as_str(), r.civil))
            .collect();
        let mut seq = records.last().map_or(0, |r| r.journal_seq);
        for schedule in due {
            if seen.contains(&(schedule.id.as_str(), now)) {
                continue;
            }
            seq = seq.checked_add(1).ok_or(CronScheduleError::Capacity)?;
            batch.fired.push(CronFireRecord {
                schedule_id: schedule.id.as_str().into(),
                expression: schedule.expression.clone(),
                payload_hint: crate::redact::redact_artifact_text(&schedule.payload_hint),
                civil: now,
                journal_seq: seq,
            });
        }
        if records.len() + batch.fired.len() > 4096 {
            return Err(CronScheduleError::Capacity);
        }
        if let Some(path) = &path
            && !batch.fired.is_empty()
        {
            if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
                bytes.push(b'\n');
            }
            for record in &batch.fired {
                serde_json::to_writer(&mut bytes, record)
                    .map_err(|_| journal_error(path, "cannot encode receipt"))?;
                bytes.push(b'\n');
            }
            if bytes.len() > 8 * 1024 * 1024 {
                return Err(CronScheduleError::Capacity);
            }
            store::write_private_atomic(path, &bytes)
                .map_err(|e| journal_error(path, &e.to_string()))?;
        }
        batch.skipped -= batch.fired.len();
        records.extend(batch.fired.iter().cloned());
        self.fires = records;
        Ok(batch)
    }
}
fn read_journal(path: &Path) -> Result<(Vec<CronFireRecord>, Vec<u8>), CronScheduleError> {
    let bytes = store::read_private_bytes(path, 8 * 1024 * 1024)
        .map_err(|e| journal_error(path, &e.to_string()))?
        .unwrap_or_default();
    let mut records = Vec::new();
    let mut seq = 0;
    let mut seen = HashSet::new();
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let mut record: CronFireRecord = serde_json::from_slice(line)
            .map_err(|_| journal_error(path, "invalid receipt JSON"))?;
        ScheduleId::parse(&record.schedule_id)?;
        validate_cron_expression(&record.expression)?;
        record.civil.validate()?;
        if record.journal_seq != seq + 1
            || !seen.insert((record.schedule_id.clone(), record.civil))
            || record.payload_hint.len() > 4096
        {
            return Err(journal_error(
                path,
                "invalid receipt sequence, duplicate fire, or oversized payload",
            ));
        }
        record.payload_hint = crate::redact::redact_artifact_text(&record.payload_hint);
        seq = record.journal_seq;
        records.push(record);
        if records.len() > 4096 {
            return Err(CronScheduleError::Capacity);
        }
    }
    Ok((records, bytes))
}
fn journal_error(path: &Path, reason: &str) -> CronScheduleError {
    CronScheduleError::JournalIo {
        path: path.display().to_string(),
        reason: reason.into(),
    }
}
pub fn run_cron_execution_product(
    registry: &mut CronScheduleRegistry,
    executor: &mut CronExecutor,
    schedules: Vec<CronSchedule>,
    now: CronCivilTime,
) -> Result<CronFireBatch, CronScheduleError> {
    let mut candidate = registry.clone();
    for schedule in schedules {
        candidate.register(schedule)?;
    }
    let batch = executor.fire_due(&candidate, now)?;
    *registry = candidate;
    Ok(batch)
}
