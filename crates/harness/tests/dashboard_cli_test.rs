use harness::{CliDeps, CliIo, ExitOutcome, UnwrapOrAbort};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    SCHEMA_VERSION,
};
use std::fs;
use std::io::Cursor;

fn run_cli(args: &[&str], deps: CliDeps) -> (i32, String, String) {
    let args: Vec<&str> = std::iter::once("harness")
        .chain(args.iter().copied())
        .collect();
    let mut stdin = Cursor::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let ExitOutcome { code, .. } = harness::run(args, &mut io, deps);
    (
        code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_events(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn seed_session(session_dir: &std::path::Path, run_id: &str, workspace_root: &std::path::Path) {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events(
        &run_dir,
        &[
            envelope(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: run_id.to_string().into(),
                    workspace_root: workspace_root.display().to_string(),
                }),
            ),
            envelope(
                run_id,
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
}

#[test]
fn dashboard_list_empty_session_dir_returns_zero_count_json() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "dashboard",
            "list",
            "--json",
            "--session-dir",
            session_dir.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_abort();
    assert_eq!(json["session_count"], 0);
    assert_eq!(json["schema_version"], "harness-dashboard-list-v1");
}

#[test]
fn dashboard_list_finds_seeded_session() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    seed_session(&session_dir, "run_dashboard_1", temp.path());
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "dashboard",
            "list",
            "--json",
            "--session-dir",
            session_dir.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_abort();
    assert_eq!(json["session_count"], 1);
    let sessions = json["sessions"].as_array().unwrap();
    assert_eq!(sessions[0]["run_id"], "run_dashboard_1");
}

#[test]
fn dashboard_list_text_mode_prints_session_info() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    seed_session(&session_dir, "run_dashboard_text", temp.path());
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "dashboard",
            "list",
            "--session-dir",
            session_dir.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("sessions in"), "stdout: {stdout}");
    assert!(stdout.contains("run_dashboard_text"), "stdout: {stdout}");
}

#[test]
fn dashboard_status_reports_session_count_and_config_state() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    seed_session(&session_dir, "run_dash_status", temp.path());
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "dashboard",
            "status",
            "--json",
            "--session-dir",
            session_dir.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_abort();
    assert_eq!(json["session_count"], 1);
    assert!(json["session_dir"].is_string());
    assert!(json["config_loaded"].is_boolean());
}

#[test]
fn dashboard_status_text_mode_prints_summary() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "dashboard",
            "status",
            "--session-dir",
            session_dir.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("dashboard status"), "stdout: {stdout}");
    assert!(stdout.contains("session_count"), "stdout: {stdout}");
}

#[test]
fn dashboard_recent_limits_results() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    seed_session(&session_dir, "run_recent_a", temp.path());
    seed_session(&session_dir, "run_recent_b", temp.path());
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "dashboard",
            "recent",
            "--json",
            "--limit",
            "1",
            "--session-dir",
            session_dir.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_abort();
    assert_eq!(json["session_count"], 1);
    assert_eq!(json["schema_version"], "harness-dashboard-recent-v1");
    assert_eq!(json["limit"], 1);
}

#[test]
fn dashboard_recent_defaults_to_five() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    for i in 0..7 {
        seed_session(&session_dir, &format!("run_recent_{i}"), temp.path());
    }
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "dashboard",
            "recent",
            "--json",
            "--session-dir",
            session_dir.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_abort();
    assert_eq!(json["session_count"], 5);
}

#[test]
fn dashboard_missing_session_dir_returns_error() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let missing = temp.path().join("nonexistent-sessions");
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());
    // act
    let (code, _stdout, stderr) = run_cli(
        &[
            "dashboard",
            "list",
            "--json",
            "--session-dir",
            missing.to_str().unwrap(),
        ],
        deps,
    );
    // assert
    assert_ne!(code, 0);
    assert!(stderr.contains("failed to read"), "stderr: {stderr}");
}
