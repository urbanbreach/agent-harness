use harness_testkit::simulation::{
    artifact_index_rows, behavior_delta, build_normalized_summary, build_report,
    compare_normalized_summaries, invariant_results, scan_simulation_artifact_root,
    simulation_event_rows, stable_fingerprint_value, summary_text, validate_artifact_index_file,
    validate_matrix_file, validate_report_file, validate_simulation_events_file, write_json_pretty,
    write_jsonl, RedactionSummary, ReportInput, SummaryInput, DEFAULT_SEED, REQUIRED_ARTIFACTS,
};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse(env::args().skip(1).collect())?;
    let matrix = validate_matrix_file(&args.matrix).map_err(format_failures)?;
    fs::create_dir_all(&args.artifact_root).map_err(|err| {
        format!(
            "failed to create artifact root {}: {err}",
            args.artifact_root.display()
        )
    })?;

    copy_file(
        &args.matrix,
        &args.artifact_root.join("simulation-matrix.json"),
    )?;

    let baseline_events = read_jsonl(&args.baseline_events)?;
    let repeat_events = read_jsonl(&args.repeat_events)?;
    let baseline_replay = read_json(&args.baseline_replay)?;
    let repeat_replay = read_json(&args.repeat_replay)?;

    let baseline_normalized =
        build_normalized_summary(&matrix, &baseline_events, &baseline_replay, &args.seed);
    let repeat_normalized =
        build_normalized_summary(&matrix, &repeat_events, &repeat_replay, &args.seed);
    write_json_pretty(
        &args.artifact_root.join("normalized-summary-baseline.json"),
        &baseline_normalized,
    )
    .map_err(|err| format!("failed to write normalized baseline: {err}"))?;
    write_json_pretty(
        &args.artifact_root.join("normalized-summary-repeat.json"),
        &repeat_normalized,
    )
    .map_err(|err| format!("failed to write normalized repeat: {err}"))?;

    let same_seed_status =
        match compare_normalized_summaries(&baseline_normalized, &repeat_normalized) {
            Ok(()) => {
                fs::write(
                    args.artifact_root.join("same-seed-comparison.txt"),
                    "status=pass\n",
                )
                .map_err(|err| format!("failed to write same-seed comparison: {err}"))?;
                "pass"
            }
            Err(failures) => {
                fs::write(
                    args.artifact_root.join("same-seed-comparison.txt"),
                    format!("status=fail\n{}\n", format_failures(failures)),
                )
                .map_err(|err| format!("failed to write same-seed comparison: {err}"))?;
                "fail"
            }
        };

    let run_fingerprint = stable_fingerprint_value(&baseline_normalized);
    let simulation_events = simulation_event_rows(
        &matrix,
        &baseline_events,
        &baseline_replay,
        &args.seed,
        &run_fingerprint,
    );
    write_jsonl(
        &args.artifact_root.join("simulation-events.jsonl"),
        &simulation_events,
    )
    .map_err(|err| format!("failed to write simulation events: {err}"))?;

    let invariants = invariant_results(&matrix, &baseline_normalized, same_seed_status);
    let deltas = behavior_delta(&matrix, &baseline_normalized);

    let raw_evidence_paths = copy_raw_evidence(&args)?;
    let mut relative_paths = REQUIRED_ARTIFACTS
        .iter()
        .map(|item| (*item).to_owned())
        .collect::<Vec<_>>();
    relative_paths.extend(raw_evidence_paths.iter().cloned());

    let placeholder_redaction_summary = RedactionSummary::clean();
    let mut index_rows = artifact_index_rows(&args.artifact_root, &matrix, &relative_paths);
    write_jsonl(
        &args.artifact_root.join("artifact-index.jsonl"),
        &index_rows,
    )
    .map_err(|err| format!("failed to write preliminary artifact index: {err}"))?;
    write_report(
        &args,
        ReportInput {
            matrix: &matrix,
            normalized: &baseline_normalized,
            run_fingerprint: &run_fingerprint,
            invariant_results: &invariants,
            behavior_delta: &deltas,
            artifact_index: &index_rows,
            redaction_summary: &placeholder_redaction_summary,
            same_seed_status,
            raw_evidence_paths: raw_evidence_paths.clone(),
        },
    )?;
    write_summary(
        &args,
        &matrix,
        &baseline_normalized,
        &run_fingerprint,
        &invariants,
        &placeholder_redaction_summary,
        same_seed_status,
    )?;

    let redaction_summary = scan_simulation_artifact_root(&args.artifact_root)
        .map_err(|failure| failure.to_string())?;
    if redaction_summary.secret_finding_count != 0 {
        return Err(format!(
            "secret-scan failed: rejected_artifacts={:?}",
            redaction_summary.rejected_artifacts
        ));
    }

    index_rows = artifact_index_rows(&args.artifact_root, &matrix, &relative_paths);
    write_jsonl(
        &args.artifact_root.join("artifact-index.jsonl"),
        &index_rows,
    )
    .map_err(|err| format!("failed to write artifact index: {err}"))?;
    write_report(
        &args,
        ReportInput {
            matrix: &matrix,
            normalized: &baseline_normalized,
            run_fingerprint: &run_fingerprint,
            invariant_results: &invariants,
            behavior_delta: &deltas,
            artifact_index: &index_rows,
            redaction_summary: &redaction_summary,
            same_seed_status,
            raw_evidence_paths: raw_evidence_paths.clone(),
        },
    )?;
    write_summary(
        &args,
        &matrix,
        &baseline_normalized,
        &run_fingerprint,
        &invariants,
        &redaction_summary,
        same_seed_status,
    )?;

    index_rows = artifact_index_rows(&args.artifact_root, &matrix, &relative_paths);
    write_jsonl(
        &args.artifact_root.join("artifact-index.jsonl"),
        &index_rows,
    )
    .map_err(|err| format!("failed to refresh final artifact index: {err}"))?;
    write_report(
        &args,
        ReportInput {
            matrix: &matrix,
            normalized: &baseline_normalized,
            run_fingerprint: &run_fingerprint,
            invariant_results: &invariants,
            behavior_delta: &deltas,
            artifact_index: &index_rows,
            redaction_summary: &redaction_summary,
            same_seed_status,
            raw_evidence_paths,
        },
    )?;

    let final_redaction_summary = scan_simulation_artifact_root(&args.artifact_root)
        .map_err(|failure| failure.to_string())?;
    if final_redaction_summary.secret_finding_count != 0 {
        return Err(format!(
            "secret-scan failed: rejected_artifacts={:?}",
            final_redaction_summary.rejected_artifacts
        ));
    }

    validate_simulation_events_file(&matrix, &args.artifact_root.join("simulation-events.jsonl"))
        .map_err(format_failures)?;
    validate_artifact_index_file(
        &matrix,
        &args.artifact_root,
        &args.artifact_root.join("artifact-index.jsonl"),
    )
    .map_err(format_failures)?;
    validate_report_file(&matrix, &args.artifact_root.join("simulation-report.json"))
        .map_err(format_failures)?;

    if same_seed_status != "pass" {
        return Err("same-seed normalized comparison failed".to_owned());
    }
    if invariants
        .iter()
        .any(|row| row.get("status").and_then(Value::as_str) != Some("pass"))
    {
        return Err("one or more simulation invariants failed".to_owned());
    }

    println!("simulation evidence PASS");
    println!("artifact_root={}", args.artifact_root.as_path().display());
    Ok(())
}

struct Args {
    artifact_root: PathBuf,
    matrix: PathBuf,
    baseline_events: PathBuf,
    baseline_replay: PathBuf,
    repeat_events: PathBuf,
    repeat_replay: PathBuf,
    seed: String,
}

impl Args {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut artifact_root = None;
        let mut matrix = None;
        let mut baseline_events = None;
        let mut baseline_replay = None;
        let mut repeat_events = None;
        let mut repeat_replay = None;
        let mut seed = DEFAULT_SEED.to_owned();
        let mut iter = args.into_iter();
        while let Some(flag) = iter.next() {
            let value = iter
                .next()
                .ok_or_else(|| format!("missing value for {flag}"))?;
            match flag.as_str() {
                "--artifact-root" => artifact_root = Some(PathBuf::from(value)),
                "--matrix" => matrix = Some(PathBuf::from(value)),
                "--baseline-events" => baseline_events = Some(PathBuf::from(value)),
                "--baseline-replay" => baseline_replay = Some(PathBuf::from(value)),
                "--repeat-events" => repeat_events = Some(PathBuf::from(value)),
                "--repeat-replay" => repeat_replay = Some(PathBuf::from(value)),
                "--seed" => seed = value,
                _ => return Err(format!("unknown argument: {flag}")),
            }
        }
        Ok(Self {
            artifact_root: artifact_root.ok_or("missing --artifact-root")?,
            matrix: matrix.ok_or("missing --matrix")?,
            baseline_events: baseline_events.ok_or("missing --baseline-events")?,
            baseline_replay: baseline_replay.ok_or("missing --baseline-replay")?,
            repeat_events: repeat_events.ok_or("missing --repeat-events")?,
            repeat_replay: repeat_replay.ok_or("missing --repeat-replay")?,
            seed,
        })
    }
}

fn write_report(args: &Args, input: ReportInput<'_>) -> Result<(), String> {
    let report = build_report(input);
    write_json_pretty(&args.artifact_root.join("simulation-report.json"), &report)
        .map_err(|err| format!("failed to write simulation report: {err}"))
}

fn write_summary(
    args: &Args,
    matrix: &harness_testkit::simulation::SimulationMatrix,
    normalized: &Value,
    run_fingerprint: &str,
    invariant_results: &[Value],
    redaction_summary: &RedactionSummary,
    same_seed_status: &str,
) -> Result<(), String> {
    let summary = summary_text(SummaryInput {
        matrix,
        normalized,
        run_fingerprint,
        invariant_results,
        redaction_summary,
        same_seed_status,
        artifact_index_path: "artifact-index.jsonl",
    });
    fs::write(args.artifact_root.join("simulation-summary.txt"), summary)
        .map_err(|err| format!("failed to write simulation summary: {err}"))
}

fn copy_raw_evidence(args: &Args) -> Result<Vec<String>, String> {
    let copies = [
        (&args.baseline_events, "raw-evidence/baseline/events.jsonl"),
        (&args.baseline_replay, "raw-evidence/baseline/replay.json"),
        (&args.repeat_events, "raw-evidence/repeat/events.jsonl"),
        (&args.repeat_replay, "raw-evidence/repeat/replay.json"),
    ];
    let mut paths = Vec::new();
    for (source, relative) in copies {
        copy_file(source, &args.artifact_root.join(relative))?;
        paths.push(relative.to_owned());
    }
    Ok(paths)
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create parent directory {}: {err}",
                parent.display()
            )
        })?;
    }
    fs::copy(source, destination).map_err(|err| {
        format!(
            "failed to copy {} to {}: {err}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn read_jsonl(path: &Path) -> Result<Vec<Value>, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line)
                .map_err(|err| format!("failed to parse {}:{}: {err}", path.display(), index + 1))
        })
        .collect()
}

fn format_failures(failures: Vec<harness_testkit::simulation::SimulationFailure>) -> String {
    failures
        .into_iter()
        .map(|failure| failure.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
