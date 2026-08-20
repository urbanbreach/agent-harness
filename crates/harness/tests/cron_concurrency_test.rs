use std::path::Path;

use harness::UnwrapOrAbort;
use harness_core::cron_execute::{CronCivilTime, CronExecutor, CronFireBatch};
use harness_core::cron_schedule::{CronSchedule, CronScheduleRegistry, ScheduleId};
use serde_json::Value;
use tempfile::TempDir;

fn schedule(id: &str, expression: &str) -> CronSchedule {
    CronSchedule {
        id: ScheduleId::parse(id).unwrap_or_abort(),
        expression: expression.to_string(),
        label: None,
        payload_hint: id.to_string(),
    }
}

fn fire_batch(journal_dir: &Path, schedules: Vec<CronSchedule>) -> CronFireBatch {
    let mut registry = CronScheduleRegistry::new();
    for schedule in schedules {
        registry.register(schedule).unwrap_or_abort();
    }
    CronExecutor::with_journal_dir(journal_dir)
        .fire_due(
            &registry,
            CronCivilTime::new(30, 14, 1, 1, 3).unwrap_or_abort(),
        )
        .unwrap_or_abort()
}

fn journal_records(journal: &Path) -> Vec<Value> {
    let body = std::fs::read_to_string(journal).unwrap_or_abort();
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_abort())
        .collect()
}

#[test]
fn cron_batch_fires_each_registered_due_schedule_exactly_once() {
    // arrange — one durable batch with one due and one idle schedule
    let dir = TempDir::new().unwrap_or_abort();
    let journal = dir.path().join("cron-fires.jsonl");

    // act
    let batch = fire_batch(
        dir.path(),
        vec![
            schedule("due", "30 14 * * *"),
            schedule("idle", "0 0 * * *"),
        ],
    );

    // assert — one fire total; journal holds a single seq-0 record for "due"
    assert_eq!(batch.fired.len(), 1);
    let records = journal_records(&journal);
    assert_eq!(records.len(), 1, "journal holds one record: {records:?}");
    assert_eq!(records[0]["schedule_id"], "due");
    assert_eq!(records[0]["journal_seq"], 0);
}

#[test]
fn cron_batch_allocates_unique_monotonic_journal_sequences() {
    // arrange — one durable batch with eight distinct due schedules
    let dir = TempDir::new().unwrap_or_abort();
    let journal = dir.path().join("cron-fires.jsonl");
    let schedules = (0..8)
        .map(|index| schedule(&format!("s{index}"), "* * * * *"))
        .collect();

    // act
    let batch = fire_batch(dir.path(), schedules);

    // assert — every schedule fired once; seqs are unique and monotonic in file order
    assert_eq!(batch.fired.len(), 8);
    let seqs: Vec<u64> = journal_records(&journal)
        .into_iter()
        .map(|record| record["journal_seq"].as_u64().unwrap_or_abort())
        .collect();
    let expected: Vec<u64> = (0..8).collect();
    assert_eq!(seqs, expected, "contiguous monotonically increasing seqs");
}
