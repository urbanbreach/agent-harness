use crate::CliIo;
use harness_core::redact::{redact_in_place, Redactor};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path, time::SystemTime};

#[path = "census/fold.rs"]
mod fold;

#[derive(clap::Args)]
pub(super) struct CensusCommand {
    /// Include sessions with a recorded event at or after this UTC date or RFC3339 time.
    #[arg(long, value_parser = parse_since)]
    since: Option<SystemTime>,
    /// Include sessions using a model whose id contains this substring (case insensitive).
    #[arg(long)]
    model: Option<String>,
    /// Maximum matching sessions, newest first.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    limit: Option<u32>,
}

fn parse_since(value: &str) -> Result<SystemTime, String> {
    let date;
    let value = if value.len() == 10 {
        date = format!("{value}T00:00:00Z");
        date.as_str()
    } else {
        value
    };
    humantime::parse_rfc3339(value)
        .map_err(|_| "expected an RFC3339 timestamp or YYYY-MM-DD".into())
}

#[derive(Default, Serialize)]
struct Metrics {
    turns: u64,
    provider_requests: u64,
    tool_calls: u64,
    tool_calls_by_tool: BTreeMap<String, u64>,
    eval_calls: u64,
    direct_tool_calls: u64,
    eval_share: f64,
    longest_identical_tool_run: u64,
    identical_tool_runs_ge_3: u64,
    open_todo_turns: u64,
    unverified_edit_turns: u64,
    runtime_reminders_by_kind: BTreeMap<String, u64>,
    compactions: u64,
    provider_errors: u64,
    provider_fallbacks: u64,
    turn_failures_by_kind: BTreeMap<String, u64>,
    subagent_spawns: u64,
}

impl Metrics {
    fn merge(&mut self, other: &Self) {
        self.turns += other.turns;
        self.provider_requests += other.provider_requests;
        self.tool_calls += other.tool_calls;
        self.eval_calls += other.eval_calls;
        self.direct_tool_calls += other.direct_tool_calls;
        self.longest_identical_tool_run = self
            .longest_identical_tool_run
            .max(other.longest_identical_tool_run);
        self.identical_tool_runs_ge_3 += other.identical_tool_runs_ge_3;
        self.open_todo_turns += other.open_todo_turns;
        self.unverified_edit_turns += other.unverified_edit_turns;
        self.compactions += other.compactions;
        self.provider_errors += other.provider_errors;
        self.provider_fallbacks += other.provider_fallbacks;
        self.subagent_spawns += other.subagent_spawns;
        for (destination, source) in [
            (&mut self.tool_calls_by_tool, &other.tool_calls_by_tool),
            (
                &mut self.runtime_reminders_by_kind,
                &other.runtime_reminders_by_kind,
            ),
            (
                &mut self.turn_failures_by_kind,
                &other.turn_failures_by_kind,
            ),
        ] {
            for (key, count) in source {
                *destination.entry(key.clone()).or_default() += count;
            }
        }
        self.update_eval_share();
    }

    fn update_eval_share(&mut self) {
        self.eval_share = if self.tool_calls == 0 {
            0.0
        } else {
            count_as_float(self.eval_calls) / count_as_float(self.tool_calls)
        };
    }
}

fn count_as_float(value: u64) -> f64 {
    // Each 32-bit limb converts exactly; their sum rounds like the original integer conversion.
    let [low0, low1, low2, low3, high0, high1, high2, high3] = value.to_le_bytes();
    f64::from(u32::from_le_bytes([high0, high1, high2, high3])) * 4_294_967_296.0
        + f64::from(u32::from_le_bytes([low0, low1, low2, low3]))
}

#[derive(Serialize)]
struct Session {
    run_id: String,
    models: Vec<String>,
    metrics: Metrics,
    #[serde(skip)]
    by_model: BTreeMap<String, Metrics>,
    #[serde(skip)]
    latest: Option<SystemTime>,
}

pub(super) fn execute(
    root: &Path,
    command: CensusCommand,
    json: bool,
    redactor: &dyn Redactor,
    io: &mut CliIo<'_>,
) -> Result<(), String> {
    let filter = command.model.as_deref().map(str::to_lowercase);
    let mut sessions = Vec::new();
    let mut unavailable = Vec::new();
    for directory in crate::replay::session_directories(root)? {
        let run_id = directory
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid session id")?
            .to_owned();
        let events = match crate::replay::read_bounded_history(&directory) {
            Ok(events) if events.iter().all(|e| e.run_id.as_str() == run_id) => events,
            _ => {
                unavailable.push(run_id);
                continue;
            }
        };
        let session = fold::project(run_id, &events);
        if command
            .since
            .is_some_and(|since| session.latest.is_none_or(|time| time < since))
            || filter.as_ref().is_some_and(|filter| {
                !session
                    .models
                    .iter()
                    .any(|model| model.to_lowercase().contains(filter))
            })
        {
            continue;
        }
        sessions.push(session);
    }
    sessions.sort_by(|a, b| {
        b.latest
            .cmp(&a.latest)
            .then_with(|| a.run_id.cmp(&b.run_id))
    });
    if let Some(limit) = command.limit {
        sessions.truncate(limit as usize);
    }
    unavailable.sort();
    let mut models: BTreeMap<String, Metrics> = BTreeMap::new();
    let mut totals = Metrics::default();
    for session in &sessions {
        totals.merge(&session.metrics);
        for (model, metrics) in &session.by_model {
            models.entry(model.clone()).or_default().merge(metrics);
        }
    }
    let mut report = serde_json::json!({
        "schema_version": "harness-sessions-census-v1",
        "session_count": sessions.len(),
        "sessions": sessions,
        "models": models,
        "totals": totals,
        "unavailable_session_ids": unavailable,
    });
    redact_in_place(redactor, &mut report);
    if json {
        return crate::inspect::print_json(io, &report);
    }
    writeln!(io.stdout, "session/model\tturns\trequests\ttools\teval%\tmax-repeat\truns>=3\topen-todo\tunverified\tcompactions\terrors\tfallbacks\tspawns").map_err(|e| e.to_string())?;
    if let Some(rows) = report["sessions"].as_array() {
        for row in rows {
            print_row(io, row["run_id"].as_str().unwrap_or(""), &row["metrics"])?;
        }
    }
    writeln!(io.stdout, "\nBy model").map_err(|e| e.to_string())?;
    if let Some(rows) = report["models"].as_object() {
        for (model, row) in rows {
            print_row(io, model, row)?;
        }
    }
    print_row(io, "TOTAL", &report["totals"])?;
    writeln!(
        io.stdout,
        "Unavailable session ids: {}",
        report["unavailable_session_ids"]
    )
    .map_err(|e| e.to_string())
}

fn print_row(io: &mut CliIo<'_>, id: &str, row: &serde_json::Value) -> Result<(), String> {
    let id: String = id
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    writeln!(
        io.stdout,
        "{id}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        row["turns"],
        row["provider_requests"],
        row["tool_calls"],
        row["eval_share"].as_f64().unwrap_or(0.0) * 100.0,
        row["longest_identical_tool_run"],
        row["identical_tool_runs_ge_3"],
        row["open_todo_turns"],
        row["unverified_edit_turns"],
        row["compactions"],
        row["provider_errors"],
        row["provider_fallbacks"],
        row["subagent_spawns"]
    )
    .map_err(|e| e.to_string())?;
    for field in [
        "tool_calls_by_tool",
        "runtime_reminders_by_kind",
        "turn_failures_by_kind",
    ] {
        writeln!(io.stdout, "  {field}: {}", row[field]).map_err(|e| e.to_string())?;
    }
    Ok(())
}
