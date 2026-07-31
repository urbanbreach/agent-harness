//! Cross-process atomicity for `harness cron fire-due`.
//!
//! Spawns real concurrent `harness` CLI processes against one shared journal
//! directory and proves the reload→dedup→append transaction serializes on the
//! cross-process journal lock: each due schedule fires exactly once per civil
//! time, and journal sequences are unique and monotonically increasing.

use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Barrier};
use std::thread;

use harness::UnwrapOrAbort;
use serde_json::Value;
use tempfile::TempDir;

const CIVIL_ARGS: [&str; 10] = [
    "--minute",
    "30",
    "--hour",
    "14",
    "--day-month",
    "1",
    "--month",
    "1",
    "--weekday",
    "3",
];

fn run_concurrent(journal_dir: &Path, specs_per_worker: Vec<Vec<String>>) -> Vec<Output> {
    let barrier = Arc::new(Barrier::new(specs_per_worker.len()));
    let journal_dir = journal_dir.to_path_buf();
    let workers: Vec<thread::JoinHandle<Output>> = specs_per_worker
        .into_iter()
        .map(|specs| {
            let barrier = Arc::clone(&barrier);
            let journal_dir = journal_dir.clone();
            thread::spawn(move || {
                barrier.wait();
                let mut args: Vec<String> = vec![
                    "cron".to_string(),
                    "fire-due".to_string(),
                    "--journal-dir".to_string(),
                ];
                args.push(journal_dir.display().to_string());
                args.extend(CIVIL_ARGS.iter().map(|arg| (*arg).to_string()));
                args.extend(specs);
                Command::new(env!("CARGO_BIN_EXE_harness"))
                    .current_dir(&journal_dir)
                    .args(&args)
                    .output()
                    .unwrap_or_abort()
            })
        })
        .collect();
    workers
        .into_iter()
        .map(|worker| worker.join().unwrap_or_abort())
        .collect()
}

fn total_fired_across(outputs: &[Output]) -> usize {
    outputs
        .iter()
        .map(|output| {
            assert!(
                output.status.success(),
                "exit={:?} stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let json: Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
            json["fired"].as_array().unwrap_or_abort().len()
        })
        .sum()
}

fn journal_records(journal: &Path) -> Vec<Value> {
    let body = std::fs::read_to_string(journal).unwrap_or_abort();
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_abort())
        .collect()
}

#[test]
fn concurrent_cron_processes_fire_each_due_schedule_exactly_once() {
    // arrange — eight concurrent processes, one shared journal, one due schedule
    let dir = TempDir::new().unwrap_or_abort();
    let journal = dir.path().join("cron-fires.jsonl");
    let specs = (0..8)
        .map(|_| vec!["due:30 14 * * *".to_string(), "idle:0 0 * * *".to_string()])
        .collect();

    // act
    let outputs = run_concurrent(dir.path(), specs);

    // assert — one fire total; journal holds a single seq-0 record for "due"
    assert_eq!(
        total_fired_across(&outputs),
        1,
        "concurrent processes must fire the due schedule exactly once"
    );
    let records = journal_records(&journal);
    assert_eq!(records.len(), 1, "journal holds one record: {records:?}");
    assert_eq!(records[0]["schedule_id"], "due");
    assert_eq!(records[0]["journal_seq"], 0);
}

#[test]
fn concurrent_cron_processes_allocate_unique_monotonic_journal_sequences() {
    // arrange — eight concurrent processes, each with its own distinct schedule
    let dir = TempDir::new().unwrap_or_abort();
    let journal = dir.path().join("cron-fires.jsonl");
    let specs = (0..8)
        .map(|index| vec![format!("s{index}:* * * * *")])
        .collect();

    // act
    let outputs = run_concurrent(dir.path(), specs);

    // assert — every schedule fired once; seqs are unique and monotonic in file order
    assert_eq!(total_fired_across(&outputs), 8);
    let seqs: Vec<u64> = journal_records(&journal)
        .into_iter()
        .map(|record| record["journal_seq"].as_u64().unwrap_or_abort())
        .collect();
    let expected: Vec<u64> = (0..8).collect();
    assert_eq!(seqs, expected, "contiguous monotonically increasing seqs");
}
