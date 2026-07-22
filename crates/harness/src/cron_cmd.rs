//! Product CLI for cron recurring schedule execution (`harness cron`).
//!
//! Surfaces fire-due evaluation over [`harness_core::cron_execute::CronExecutor`]
//! with a durable fire journal. Schedules are provided inline as `id:<five-field-expr>`
//! pairs; the command registers each schedule, evaluates due ones at the operator-supplied
//! civil time, and appends fire records to `<journal-dir>/cron-fires.jsonl`.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::cron_execute::{CronCivilTime, CronExecutor, CronFireBatch};
use harness_core::cron_schedule::{
    CronSchedule, CronScheduleError, CronScheduleRegistry, ScheduleId,
};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct CronCommand {
    #[command(subcommand)]
    command: CronSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum CronSubcommand {
    /// Register schedules inline, fire due ones at a civil time, and journal fires.
    FireDue(CronFireDueCommand),
}

#[derive(Debug, Args, Clone)]
struct CronFireDueCommand {
    /// Current minute 0-59.
    #[arg(long, default_value_t = 0)]
    minute: u8,
    /// Current hour 0-23.
    #[arg(long, default_value_t = 0)]
    hour: u8,
    /// Day of month 1-31.
    #[arg(long = "day-month", default_value_t = 1)]
    day_month: u8,
    /// Month 1-12.
    #[arg(long, default_value_t = 1)]
    month: u8,
    /// Day of week 0-6 (0=Sunday).
    #[arg(long, default_value_t = 0)]
    weekday: u8,
    /// Journal directory for cron-fires.jsonl (default: <workspace>/.agent-harness/cron-journal).
    #[arg(long)]
    journal_dir: Option<PathBuf>,
    /// Schedules to fire; each must be `id:<five-field-expression>`.
    specs: Vec<String>,
}

pub(crate) fn execute_with_io(command: CronCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    match command.command {
        CronSubcommand::FireDue(cmd) => run_fire_due(cmd, io, deps),
    }
}

fn workspace_root(deps: &CliDeps) -> PathBuf {
    deps.current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn parse_schedule_spec(spec: &str) -> Result<CronSchedule, String> {
    let (id_str, expression) = spec
        .split_once(':')
        .ok_or_else(|| format!("schedule spec `{spec}` must be `id:<five-field-expression>`"))?;
    let id_str = id_str.trim();
    let expression = expression.trim();
    if id_str.is_empty() || expression.is_empty() {
        return Err(format!("schedule spec `{spec}` has empty id or expression"));
    }
    let id = ScheduleId::parse(id_str)
        .map_err(|err| format!("invalid schedule id `{id_str}`: {err}"))?;
    Ok(CronSchedule {
        id,
        expression: expression.to_string(),
        label: None,
        payload_hint: id_str.to_string(),
    })
}

fn run_fire_due(cmd: CronFireDueCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let now = match CronCivilTime::new(cmd.minute, cmd.hour, cmd.day_month, cmd.month, cmd.weekday)
    {
        Ok(now) => now,
        Err(err) => return map_cron_error(io, err),
    };

    let journal_dir = cmd
        .journal_dir
        .unwrap_or_else(|| workspace_root(deps).join(".agent-harness/cron-journal"));
    let mut executor = CronExecutor::with_journal_dir(&journal_dir);
    let mut registry = CronScheduleRegistry::new();

    for schedule in cmd.specs {
        match parse_schedule_spec(&schedule) {
            Ok(schedule) => {
                if let Err(err) = registry.register(schedule) {
                    return map_cron_error(io, err);
                }
            }
            Err(msg) => {
                let _ = writeln!(io.stderr, "cron: {msg}");
                return 1;
            }
        }
    }

    match executor.fire_due(&registry, now) {
        Ok(batch) => write_json(
            io,
            &CronFireJson {
                journal_dir: journal_dir.display().to_string(),
                batch,
            },
        ),
        Err(err) => map_cron_error(io, err),
    }
}

fn map_cron_error(io: &mut CliIo<'_>, err: CronScheduleError) -> i32 {
    let _ = writeln!(io.stderr, "cron: {err}");
    1
}

#[derive(Debug, Serialize)]
struct CronFireJson {
    journal_dir: String,
    #[serde(flatten)]
    batch: CronFireBatch,
}

fn write_json(io: &mut CliIo<'_>, value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "cron: failed to serialize JSON: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliIo;
    use std::fs;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn run_cli(workspace: &std::path::Path, args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real().with_current_dir(workspace.to_path_buf());
        let mut argv: Vec<String> = vec!["harness".to_string(), "cron".to_string()];
        for arg in args {
            argv.push((*arg).to_string());
        }
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn fire_due_fires_matching_schedule_and_journals_durable_records() {
        // arrange — a workspace that owns the durable cron journal
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let journal = ws.join(".agent-harness/cron-journal/cron-fires.jsonl");

        // act — fire due schedules at a matching civil time
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "fire-due",
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
                "due:30 14 * * *",
                "not-due:0 0 * * *",
            ],
        );

        // assert — the due schedule fired, the non-due was skipped, journal persisted
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"fired\": ["), "stdout: {stdout}");
        assert!(
            stdout.contains("\"schedule_id\": \"due\""),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("\"skipped\": 1"), "stdout: {stdout}");
        assert!(journal.is_file(), "durable cron fire journal must exist");
        let body = fs::read_to_string(&journal).expect("journal readable");
        assert!(body.contains("\"schedule_id\":\"due\""), "journal: {body}");
        assert!(
            !body.contains("\"schedule_id\":\"not-due\""),
            "journal excludes non-due: {body}"
        );
    }

    #[test]
    fn fire_due_with_no_schedules_reports_empty_batch() {
        // arrange
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let journal = ws.join(".agent-harness/cron-journal/cron-fires.jsonl");

        // act
        let (code, stdout, _stderr) = run_cli(
            ws,
            &[
                "fire-due",
                "--minute",
                "0",
                "--hour",
                "0",
                "--day-month",
                "1",
                "--month",
                "1",
                "--weekday",
                "0",
            ],
        );

        // assert — empty batch, no journal file written, no crash
        assert_eq!(code, 0);
        assert!(stdout.contains("\"fired\": []"), "stdout: {stdout}");
        assert!(stdout.contains("\"skipped\": 0"), "stdout: {stdout}");
        assert!(!journal.exists(), "no journal when nothing fires");
    }

    #[test]
    fn fire_due_explicit_journal_dir_writes_to_specified_path() {
        // arrange — an explicit journal directory outside the workspace
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let journal_dir = ws.join("custom-journal");
        let journal = journal_dir.join("cron-fires.jsonl");

        // act
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "fire-due",
                "--journal-dir",
                journal_dir.to_str().unwrap(),
                "--minute",
                "0",
                "--hour",
                "9",
                "--day-month",
                "1",
                "--month",
                "6",
                "--weekday",
                "1",
                "monday:0 9 * * 1",
            ],
        );

        // assert — schedule fires into the explicit journal directory
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"schedule_id\": \"monday\""),
            "stdout: {stdout}"
        );
        assert!(journal.is_file(), "journal written to explicit path");
        let body = fs::read_to_string(&journal).expect("journal readable");
        assert!(
            body.contains("\"schedule_id\":\"monday\""),
            "journal: {body}"
        );
    }

    #[test]
    fn fire_due_invalid_civil_time_fails_closed() {
        // arrange
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act — minute=99 is out of range 0-59
        let (code, _, stderr) = run_cli(
            ws,
            &[
                "fire-due",
                "--minute",
                "99",
                "--hour",
                "0",
                "--day-month",
                "1",
                "--month",
                "1",
                "--weekday",
                "0",
            ],
        );

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("cron:"), "stderr: {stderr}");
    }

    #[test]
    fn fire_due_malformed_spec_fails_closed() {
        // arrange
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act — spec without colon separator
        let (code, _, stderr) = run_cli(
            ws,
            &[
                "fire-due",
                "--minute",
                "0",
                "--hour",
                "0",
                "--day-month",
                "1",
                "--month",
                "1",
                "--weekday",
                "0",
                "no-colon-here",
            ],
        );

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("cron:"), "stderr: {stderr}");
    }

    #[test]
    fn fire_due_duplicate_schedule_id_fails_closed() {
        // arrange
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act — two schedules with the same id
        let (code, _, stderr) = run_cli(
            ws,
            &[
                "fire-due",
                "--minute",
                "0",
                "--hour",
                "0",
                "--day-month",
                "1",
                "--month",
                "1",
                "--weekday",
                "0",
                "dup:0 0 * * *",
                "dup:30 12 * * *",
            ],
        );

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("cron:"), "stderr: {stderr}");
    }
}
