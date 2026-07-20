use harness::UnwrapOrAbort;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde_json::json;
use tempfile::tempdir;
use vt100::Parser;

const ARTIFACT_DIR_ENV: &str = "HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR";
const MARKER_TIMEOUT: Duration = Duration::from_secs(20);
const CHILD_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const COLS: u16 = 110;
const ROWS: u16 = 32;

#[test]
#[ignore = "signoff-pty happy path; run via scripts/test-lanes.sh signoff-pty"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn scripted_tui_happy_path_records_start_prompt_permission_tool_edit_resume_and_quit() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    let artifact_dir = std::env::var_os(ARTIFACT_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_abort();
    fs::create_dir_all(&artifact_dir).unwrap_or_abort();

    let temp = tempdir().unwrap_or_abort();
    let prompt_session_dir = temp.path().join("prompt-sessions");
    let scenario_session_dir = temp.path().join("scenario-sessions");

    let prompt_recording = record_prompt_and_quit(&prompt_session_dir);
    let prompt_run_dir = newest_run_dir(&prompt_session_dir).unwrap_or_abort();
    let prompt_events_path = prompt_run_dir.join("events.jsonl");
    let prompt_events = fs::read_to_string(&prompt_events_path).unwrap_or_abort();
    assert!(prompt_events.contains("\"event_type\":\"user_message_submitted\""));
    assert!(prompt_events.contains("Hello from PTY"));

    let scenario_recording = record_permission_tool_edit_and_auto_exit(&scenario_session_dir);
    let scenario_run_dir = newest_run_dir(&scenario_session_dir).unwrap_or_abort();
    let scenario_events_path = scenario_run_dir.join("events.jsonl");
    let scenario_events = fs::read_to_string(&scenario_events_path).unwrap_or_abort();
    for marker in [
        "\"event_type\":\"permission_requested\"",
        "\"event_type\":\"permission_resolved\"",
        "\"event_type\":\"tool_call_requested\"",
        "\"event_type\":\"tool_call_finished\"",
        "\"event_type\":\"edit_applied\"",
        "\"event_type\":\"run_finished\"",
        "demo.txt",
    ] {
        assert!(
            scenario_events.contains(marker),
            "scenario events missing {marker}"
        );
    }

    let resume_recording = record_resume_picker_and_quit(&prompt_session_dir);

    let prompt_events_artifact = artifact_dir.join("prompt.events.jsonl");
    let scenario_events_artifact = artifact_dir.join("scenario.events.jsonl");
    fs::copy(&prompt_events_path, &prompt_events_artifact).unwrap_or_abort();
    fs::copy(&scenario_events_path, &scenario_events_artifact).unwrap_or_abort();

    let manifest_path = artifact_dir.join("tui-happy-path-recording.json");
    let summary_path = artifact_dir.join("tui-happy-path-recording.md");
    let manifest = json!({
        "schema_version": "harness-tui-pty-happy-path-v1",
        "timestamp_unix_ms": timestamp_unix_ms(),
        "lane": "signoff-pty",
        "artifact_dir": artifact_dir.display().to_string(),
        "command": "scripts/test-lanes.sh signoff-pty",
        "env": {
            ARTIFACT_DIR_ENV: artifact_dir.display().to_string(),
            "RUST_TEST_THREADS": "1"
        },
        "covered_flow_steps": [
            "start",
            "prompt",
            "permission approval",
            "tool call",
            "edit",
            "resume picker",
            "quit"
        ],
        "runs": {
            "prompt_run_dir": prompt_run_dir.display().to_string(),
            "scenario_run_dir": scenario_run_dir.display().to_string(),
            "prompt_events_artifact": prompt_events_artifact.display().to_string(),
            "scenario_events_artifact": scenario_events_artifact.display().to_string()
        },
        "recordings": [prompt_recording, scenario_recording, resume_recording]
    });
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    fs::write(
        &summary_path,
        format!(
            "# TUI PTY happy-path recording\n\n- lane: signoff-pty\n- manifest: {}\n- prompt events: {}\n- scenario events: {}\n- covered: start, prompt, permission approval, tool call, edit, resume picker, quit\n",
            manifest_path.display(),
            prompt_events_artifact.display(),
            scenario_events_artifact.display()
        ),
    )
    .unwrap_or_abort();

    assert!(manifest_path.is_file());
    assert!(summary_path.is_file());
}

#[test]
#[ignore = "signoff dual-binary CLI PTY worktree journey; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_worktree_ctrl_w_creates_git_worktree() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let repo = temp.path().join("repo");
    let session_dir = temp.path().join("worktree-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    init_git_repo_for_worktree(&repo);

    let worktree_parent = repo.join(".agent-harness").join("worktrees");
    assert!(
        !worktree_parent.exists(),
        "worktree parent must not exist before Ctrl+W"
    );

    let mut helper = spawn_harness_pty_in(
        &repo,
        &[
            "tui".to_string(),
            "--mock".to_string(),
            "--deterministic".to_string(),
            "--session-dir".to_string(),
            session_dir.display().to_string(),
        ],
    );

    let start = helper.wait_for("New worktree", "cli welcome worktree row");
    assert!(
        start["screen"]
            .as_str()
            .unwrap_or("")
            .contains("Resume session"),
        "startup welcome must still show resume row before worktree handoff"
    );

    helper.send_ctrl(b'w');

    let live = wait_for_worktree_created_and_live_shell(
        &mut helper,
        &worktree_parent,
        "worktree created + live shell",
    );

    let worktrees = list_worktree_dirs(&worktree_parent);
    assert_eq!(
        worktrees.len(),
        1,
        "exactly one worktree directory expected under {}",
        worktree_parent.display()
    );
    let worktree_path = &worktrees[0];
    assert!(
        worktree_path.join("README.md").is_file(),
        "worktree checkout must contain seeded README.md at {}",
        worktree_path.display()
    );
    let branch = git_text(worktree_path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert!(
        branch.starts_with("harness/wt-"),
        "worktree branch must use harness/wt- prefix, got {branch:?}"
    );
    let live_screen = helper.screen_text();
    assert!(
        live_shell_markers_present(&live_screen),
        "live shell after Ctrl+W must show mock live chrome (not only startup)\n{live_screen}"
    );
    let live = json!({
        "label": live["label"].clone(),
        "marker": live["marker"].clone(),
        "screen": truncate_for_artifact(&live_screen),
        "worktree_count": live["worktree_count"].clone(),
    });

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-worktree-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --mock --deterministic --session-dir {} (cwd={})",
                session_dir.display(),
                repo.display()
            ),
            "worktree_path": worktree_path.display().to_string(),
            "worktree_branch": branch,
            "markers": {
                "startup": "New worktree",
                "action": "Ctrl+w",
                "postconditions": [
                    "git worktree directory on disk",
                    "harness/wt-* branch",
                    "welcome action rows absent"
                ]
            },
            "start_screen": start,
            "live_screen": live,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-worktree.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY resume journey; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_ctrl_s_resumes_seeded_session() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("resume-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    seed_mock_prompt_session(&session_dir);
    let seeded_run_dir = newest_run_dir(&session_dir).unwrap_or_abort();
    let seeded_run_id = run_id_from_events(&seeded_run_dir);
    assert!(
        !seeded_run_id.is_empty(),
        "seeded session must expose a run_id from events.jsonl under {}",
        seeded_run_dir.display()
    );
    let seeded_events = fs::read_to_string(seeded_run_dir.join("events.jsonl")).unwrap_or_abort();
    assert!(
        seeded_events.contains("\"event_type\":\"user_message_submitted\"")
            && seeded_events.contains("Hello from PTY"),
        "seeded session must contain the real prompt turn events"
    );

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    let start = helper.wait_for("Resume session", "cli welcome resume row");
    assert!(
        start["screen"]
            .as_str()
            .unwrap_or("")
            .contains("New worktree"),
        "startup welcome must still show worktree row before resume handoff"
    );

    helper.send_ctrl(b's');
    let picker = helper.wait_for("continue ready", "cli resume picker lists seeded session");
    let picker_screen = picker["screen"].as_str().unwrap_or("");
    assert!(
        picker_screen.contains(&seeded_run_id)
            || picker_screen.contains("continue ready")
            || helper.screen_text().contains(&seeded_run_id),
        "resume picker must list the seeded session ({seeded_run_id})\n{picker_screen}"
    );
    helper.send_key(b'\r');

    let live = wait_for_resumed_live_shell(&mut helper, &seeded_run_id, "resume live shell");
    let live_screen = helper.screen_text();
    assert!(
        live_shell_markers_present(&live_screen),
        "live shell after Ctrl+S resume must show mock live chrome\n{live_screen}"
    );
    assert!(
        live_screen.contains("Continued")
            || live_screen.contains(&seeded_run_id)
            || live_screen.contains(&format!("run {seeded_run_id}")),
        "live shell after resume must show continued identity or seeded session id {seeded_run_id}\n{live_screen}"
    );
    assert!(
        live_screen.contains("Hello from PTY") || live_screen.contains("Hello world"),
        "resumed live shell should surface prior turn content\n{live_screen}"
    );
    assert!(
        live_screen.contains("Shift+Tab:mode") || live_screen.contains("Ctrl+x:shortcuts"),
        "startup launcher chrome must yield to live shell footer after resume\n{live_screen}"
    );
    assert!(
        !live_screen.contains("New worktree"),
        "startup New worktree welcome must clear after resume into live shell\n{live_screen}"
    );
    assert!(
        seeded_run_dir.join("events.jsonl").is_file(),
        "seeded session event store must remain on disk at {}",
        seeded_run_dir.display()
    );

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-resume-v1",
            "scenario": "SCN-P0-RESUME",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --mock --deterministic --session-dir {}",
                session_dir.display()
            ),
            "seeded_run_dir": seeded_run_dir.display().to_string(),
            "seeded_run_id": seeded_run_id,
            "markers": {
                "startup": "Resume session",
                "action": "Ctrl+s",
                "picker": "continue ready",
                "postconditions": [
                    "live shell markers",
                    "session id on screen",
                    "New worktree welcome absent"
                ]
            },
            "start_screen": start,
            "picker_screen": picker,
            "live_screen": live,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-resume.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_startup_structural_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    let start = helper.wait_for("❯", "cli composer focus");
    let start_screen = start["screen"].as_str().unwrap_or("");
    let welcome_chrome = start_screen.contains("New worktree")
        && start_screen.contains("Resume session")
        && start_screen.contains("Quit");
    assert!(
        welcome_chrome,
        "compiled harness CLI under PTY must show freeze-aligned welcome action rows\n{start_screen}"
    );

    helper.send_ctrl(b'p');
    let palette = helper.wait_for("Commands", "cli palette");

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --mock --deterministic --session-dir {}",
                session_dir.display()
            ),
            "markers": ["composer ❯", "New worktree", "Resume session", "Quit", "Commands"],
            "start_screen": start,
            "palette_screen": palette,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY overlay smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_overlay_keybind_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-overlay-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    helper.wait_for("New worktree", "cli welcome ready");

    helper.send_ctrl(b's');
    let session = helper.wait_for("e expand", "cli session overlay");
    helper.send_key(0x1b);
    helper.wait_until_absent("e expand", "session dismissed");

    helper.send_ctrl(b'x');
    let help = helper.wait_for("Keyboard Shortcuts", "cli help overlay");
    helper.send_key(0x1b);
    helper.wait_until_absent("Keyboard Shortcuts", "help dismissed");

    helper.send_ctrl(b'p');
    let palette = helper.wait_for("Commands", "cli palette overlay");

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-overlay-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --mock --deterministic --session-dir {}",
                session_dir.display()
            ),
            "keybinds": {
                "session": "Ctrl+s",
                "help": "Ctrl+x",
                "palette": "Ctrl+p"
            },
            "markers": ["Resume session", "Keyboard Shortcuts", "Commands"],
            "session_screen": session,
            "help_screen": help,
            "palette_screen": palette,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-overlay-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY scenario permission smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_scenario_permission_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-scenario-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--scenario".to_string(),
        "golden_path_interactive".to_string(),
        "--deterministic".to_string(),
        "--exit-on-finish".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    let permission = helper.wait_for("Allow Edit", "cli scenario permission modal");
    let choices = helper.wait_for("always-approve", "cli scenario permission choices");
    helper.send_key(b'\r');
    helper.send_key(b'\r');
    let completed = helper.wait_for("demo.txt", "cli scenario tool edit completed");
    helper.wait_success("cli scenario PTY child");

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-scenario-permission-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --scenario golden_path_interactive --deterministic --exit-on-finish --session-dir {}",
                session_dir.display()
            ),
            "markers": ["Allow Edit", "always-approve", "demo.txt"],
            "permission_screen": permission,
            "choices_screen": choices,
            "completed_screen": completed,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-scenario-permission-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }
}

#[test]
#[ignore = "signoff dual-binary CLI PTY scenario auto-complete smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_scenario_auto_complete_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-auto-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--scenario".to_string(),
        "golden_path".to_string(),
        "--deterministic".to_string(),
        "--exit-on-finish".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    let completed = helper.wait_for("ready for next turn", "cli auto scenario completed");
    helper.wait_success("cli auto scenario PTY child");

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-scenario-auto-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --scenario golden_path --deterministic --exit-on-finish --session-dir {}",
                session_dir.display()
            ),
            "markers": ["ready for next turn"],
            "completed_screen": completed,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-scenario-auto-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }
}

#[test]
#[ignore = "signoff dual-binary CLI PTY mock fail chrome smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_mock_fail_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-fail-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    helper.wait_for("❯", "cli fail smoke ready");
    helper.write_text("dual-binary-unmatched-prompt-for-fail-chrome");
    helper.send_key(b'\r');
    let fail = helper.wait_for("Retry failed", "cli mock fail chrome");
    let missing = helper.wait_for("mock fixture missing", "cli mock missing fixture text");

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-mock-fail-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --mock --deterministic --session-dir {}",
                session_dir.display()
            ),
            "prompt": "dual-binary-unmatched-prompt-for-fail-chrome",
            "markers": ["Retry failed", "mock fixture missing"],
            "fail_screen": fail,
            "missing_screen": missing,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-mock-fail-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY question overlay smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_scenario_question_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-question-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--scenario".to_string(),
        "question_interactive".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    let prompt = helper.wait_for("Pick one", "cli scenario question prompt");
    let options = helper.wait_for("Type your answer here", "cli scenario question options");
    let footer = helper.wait_for("Enter:submit", "cli scenario question footer");
    let screen = helper.screen_text();
    assert!(
        !screen.contains("always-approve"),
        "question dual-binary must not show edit-permission allow chrome\n{screen}"
    );
    assert!(
        screen.contains("1 (●) A") || screen.contains("Option A"),
        "question dual-binary must show numbered choice chrome\n{screen}"
    );

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-scenario-question-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --scenario question_interactive --deterministic --session-dir {}",
                session_dir.display()
            ),
            "markers": ["Pick one", "1 (●) A", "Type your answer here", "Enter:submit"],
            "prompt_screen": prompt,
            "options_screen": options,
            "footer_screen": footer,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-scenario-question-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY secondary surface smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_secondary_surface_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-secondary-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    helper.wait_for("❯", "cli secondary ready");

    helper.send_ctrl(b'x');
    helper.send_key(b's');
    let status = helper.wait_for("Status", "cli status dialog");
    let status_screen = helper.screen_text();
    assert!(
        status_screen.contains("MCP")
            || status_screen.contains("LSP")
            || status_screen.contains("No MCP")
            || status_screen.contains("Plugins"),
        "status dual-binary must show operator content\n{status_screen}"
    );
    helper.send_key(0x1b);
    helper.wait_until_absent("No MCP", "status dismissed");

    helper.send_ctrl(b'x');
    helper.send_key(b'm');
    let model = helper.wait_for("Select model", "cli model switcher");
    helper.send_key(0x1b);
    helper.wait_until_absent("Select model", "model switcher dismissed");

    // Slash rows are alpha-sorted; "Switch session" is below the fold until filtered.
    // Use an always-visible first-page marker, then filter to /toggles.
    helper.write_text("/");
    helper.wait_for("/dashboard", "cli slash menu");
    helper.write_text("toggles");
    helper.send_key(b'\r');
    let toggles = helper.wait_for("Toggles", "cli toggles menu");

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-secondary-surface-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --mock --deterministic --session-dir {}",
                session_dir.display()
            ),
            "keybinds": {
                "status": "Ctrl+x s",
                "model": "Ctrl+x m",
                "toggles": "/toggles"
            },
            "markers": ["Status", "Select model", "Toggles"],
            "status_screen": status,
            "model_screen": model,
            "toggles_screen": toggles,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-secondary-surface-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY mock success smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_mock_success_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-mock-success-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    helper.wait_for("❯", "cli mock success ready");
    helper.write_text("Hello from PTY");
    let draft = helper.wait_for("Hello from PTY", "cli mock success draft");
    helper.send_key(b'\r');
    let response = helper.wait_for("Hello world", "cli mock success response");
    let screen = helper.screen_text();
    assert!(
        !screen.contains("Retry failed"),
        "mock success dual-binary must not show fail chrome\n{screen}"
    );

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-mock-success-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --mock --deterministic --session-dir {}",
                session_dir.display()
            ),
            "markers": ["Hello from PTY", "Hello world"],
            "draft_screen": draft,
            "response_screen": response,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-mock-success-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY permission deny smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_scenario_permission_deny_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-permission-deny-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--scenario".to_string(),
        "golden_path_interactive".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    let open = helper.wait_for("Allow Edit", "cli permission deny open");
    helper.wait_for("always-approve", "cli permission deny choices");
    helper.send_ctrl(b'n');
    let decision = helper.wait_for("decision sent", "cli permission deny decision sent");
    let choices_gone =
        helper.wait_until_absent("always-approve", "cli permission deny choices gone");
    let settled = helper.wait_until_absent("Allow Edit", "cli permission deny dock dismissed");
    let screen = helper.screen_text();
    assert!(
        !screen.contains("Allow Edit") && !screen.contains("always-approve"),
        "denied permission dual-binary must dismiss permission dock\n{screen}"
    );
    assert!(
        screen.contains('❯') || screen.contains("Worked for"),
        "denied permission dual-binary must settle to post-turn chrome\n{screen}"
    );

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-scenario-permission-deny-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --scenario golden_path_interactive --deterministic --session-dir {}",
                session_dir.display()
            ),
            "keybinds": { "deny": "Ctrl+n" },
            "markers": [
                "Allow Edit",
                "always-approve",
                "decision sent",
                "absent:always-approve",
                "absent:Allow Edit"
            ],
            "open_screen": open,
            "decision_screen": decision,
            "choices_gone_screen": choices_gone,
            "settled_screen": settled,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-scenario-permission-deny-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

#[test]
#[ignore = "signoff dual-binary CLI PTY question resolve smoke; run with HARNESS_TUI_PTY_SIGNOFF=1"]
#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn dual_binary_cli_pty_scenario_question_resolve_markers() {
    // arrange
    // act
    // assert
    #[cfg(not(target_os = "linux"))]
    panic!("abort");
    if std::env::var_os("HARNESS_TUI_PTY_SIGNOFF").is_none() {
        if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
            panic!("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1");
        }
        return;
    }

    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("dual-binary-question-resolve-sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--scenario".to_string(),
        "question_interactive".to_string(),
        "--deterministic".to_string(),
        "--exit-on-finish".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);

    let open = helper.wait_for("Pick one", "cli question resolve open");
    // Freeze uses (○) until an answer is committed; focus is style-only, not ●.
    helper.wait_for("1 (○) A", "cli question choice row");
    helper.wait_for("Type your answer here", "cli question choice chrome");
    helper.write_text("1");
    helper.wait_for("1 (●) A", "cli question choice committed");
    helper.send_key(b'\r');
    // Question text can remain in transcript history; assert modal chrome dismisses.
    let dismissed = helper.wait_until_absent(
        "Type your answer here",
        "cli question resolve chrome dismissed",
    );
    helper.wait_until_absent("Enter:submit", "cli question resolve footer dismissed");
    let screen = helper.screen_text();
    assert!(
        !screen.contains("Type your answer here") && !screen.contains("Enter:submit"),
        "resolved question dual-binary must dismiss choice chrome\n{screen}"
    );
    assert!(
        !screen.contains("1 (○) A") && !screen.contains("1 (●) A"),
        "resolved question dual-binary must dismiss numbered choice chrome\n{screen}"
    );

    if let Some(artifact_dir) = std::env::var_os(ARTIFACT_DIR_ENV).map(PathBuf::from) {
        fs::create_dir_all(&artifact_dir).unwrap_or_abort();
        let receipt = json!({
            "schema_version": "harness-tui-dual-binary-cli-pty-scenario-question-resolve-smoke-v1",
            "binary": env!("CARGO_BIN_EXE_harness"),
            "command": format!(
                "harness tui --scenario question_interactive --deterministic --exit-on-finish --session-dir {}",
                session_dir.display()
            ),
            "markers": [
                "Pick one",
                "1 (○) A",
                "Type your answer here",
                "1 (●) A",
                "absent:Type your answer here",
                "absent:Enter:submit",
                "absent:1 (○) A",
                "absent:1 (●) A"
            ],
            "open_screen": open,
            "dismissed_screen": dismissed,
        });
        fs::write(
            artifact_dir.join("dual-binary-cli-pty-scenario-question-resolve-smoke.json"),
            serde_json::to_vec_pretty(&receipt).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }

    let _ = helper.child.kill();
}

fn record_prompt_and_quit(session_dir: &Path) -> serde_json::Value {
    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);
    let mut screens = Vec::new();
    screens.push(helper.wait_for("❯", "start"));
    helper.write_text("Hello from PTY");
    screens.push(helper.wait_for("Hello from PTY", "prompt draft"));
    helper.send_key(b'\r');
    screens.push(helper.wait_for("Hello world", "prompt response"));
    quit_helper(&mut helper, &mut screens);
    helper.wait_success("prompt PTY child");

    json!({
        "stage": "prompt_and_quit",
        "command": format!("harness tui --mock --deterministic --session-dir {}", session_dir.display()),
        "screens": screens,
    })
}

fn seed_mock_prompt_session(session_dir: &Path) {
    let _ = record_prompt_and_quit(session_dir);
}

fn run_id_from_events(run_dir: &Path) -> String {
    let events_path = run_dir.join("events.jsonl");
    let events = fs::read_to_string(&events_path).unwrap_or_abort();
    for line in events.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(run_id) = value.get("run_id").and_then(|v| v.as_str()) {
            if !run_id.is_empty() {
                return run_id.to_string();
            }
        }
    }
    run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn record_permission_tool_edit_and_auto_exit(session_dir: &Path) -> serde_json::Value {
    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--scenario".to_string(),
        "golden_path_interactive".to_string(),
        "--deterministic".to_string(),
        "--exit-on-finish".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);
    let mut screens = Vec::new();
    screens.push(helper.wait_for("Allow Edit", "permission modal"));
    screens.push(helper.wait_for("always-approve", "permission choices"));
    helper.send_key(b'\r');
    helper.send_key(b'\r');
    screens.push(helper.wait_for("demo.txt", "tool edit completed"));
    helper.wait_success("scenario PTY child");

    json!({
        "stage": "permission_tool_edit_auto_exit",
        "command": format!("harness tui --scenario golden_path_interactive --deterministic --exit-on-finish --session-dir {}", session_dir.display()),
        "screens": screens,
    })
}

fn record_resume_picker_and_quit(session_dir: &Path) -> serde_json::Value {
    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);
    let mut screens = Vec::new();
    screens.push(helper.wait_for("❯", "resume startup"));
    // Filter first: unfiltered slash list is alpha-sorted and hides /sessions off-screen.
    helper.write_text("/");
    helper.write_text("sessions");
    screens.push(helper.wait_for("Switch session", "slash commands"));
    helper.send_key(b'\r');
    screens.push(helper.wait_for("Resume session", "resume picker"));
    helper.send_key(0x1b);
    screens.push(helper.wait_for("❯", "resume picker dismissed"));
    quit_helper(&mut helper, &mut screens);
    helper.wait_success("resume PTY child");

    json!({
        "stage": "resume_picker_and_quit",
        "command": format!("harness tui --mock --session-dir {}", session_dir.display()),
        "screens": screens,
    })
}

fn quit_helper(helper: &mut SpawnedHarness, screens: &mut Vec<serde_json::Value>) {
    helper.send_ctrl(b'q');
    screens.push(json!({
        "label": "quit via ctrl+q",
        "marker": "Ctrl+q",
        "screen": truncate_for_artifact(&helper.screen_text()),
    }));
}

struct SpawnedHarness {
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
}

impl SpawnedHarness {
    fn wait_for(&mut self, needle: &str, label: &str) -> serde_json::Value {
        let screen = wait_for_screen_contains(&mut self.parser, &self.output_rx, needle);
        json!({
            "label": label,
            "marker": needle,
            "screen": truncate_for_artifact(&screen),
        })
    }

    fn wait_until_absent(&mut self, needle: &str, label: &str) -> serde_json::Value {
        let screen = wait_for_screen_absent(&mut self.parser, &self.output_rx, needle);
        json!({
            "label": label,
            "marker_absent": needle,
            "screen": truncate_for_artifact(&screen),
        })
    }

    fn write_text(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).unwrap_or_abort();
        self.writer.flush().unwrap_or_abort();
    }

    fn send_key(&mut self, key: u8) {
        self.writer.write_all(&[key]).unwrap_or_abort();
        self.writer.flush().unwrap_or_abort();
    }

    fn send_ctrl(&mut self, key: u8) {
        self.send_key(key & 0x1f);
    }

    fn screen_text(&mut self) -> String {
        drain_output(&mut self.parser, &self.output_rx);
        self.parser.screen().contents()
    }

    #[allow(
        clippy::panic,
        clippy::match_wild_err_arm,
        reason = "test code must panic gracefully"
    )]
    fn wait_success(mut self, label: &str) {
        let deadline = Instant::now() + CHILD_EXIT_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    assert!(status.success(), "{label} exited with {status:?}");
                    return;
                }
                Ok(None) => {}
                Err(err) => panic!("{label}: wait error: {err}"),
            }

            drain_output(&mut self.parser, &self.output_rx);
            if Instant::now() >= deadline {
                let final_screen = self.parser.screen().contents();
                let _ = self.child.kill();
                panic!("{label}: child exit timeout\n--- screen ---\n{final_screen}\n--- end ---");
            }

            thread::sleep(READ_POLL_TIMEOUT);
        }
    }
}

fn spawn_harness_pty(args: &[String]) -> SpawnedHarness {
    spawn_harness_pty_in(Path::new("."), args)
}

fn spawn_harness_pty_in(cwd: &Path, args: &[String]) -> SpawnedHarness {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap_or_abort();

    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_harness"));
    command.cwd(cwd);
    for arg in args {
        command.arg(arg);
    }
    configure_deterministic_env(&mut command);

    let child = pair.slave.spawn_command(command).unwrap_or_abort();
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().unwrap_or_abort();
    let writer = pair.master.take_writer().unwrap_or_abort();
    let output_rx = spawn_reader_thread(reader);

    SpawnedHarness {
        _master: pair.master,
        child,
        writer,
        output_rx,
        parser: Parser::new(ROWS, COLS, 0),
    }
}

fn init_git_repo_for_worktree(path: &Path) {
    fs::create_dir_all(path).unwrap_or_abort();
    run_git(path, &["init", "-b", "main"]);
    run_git(path, &["config", "user.email", "worktree-pty@example.com"]);
    run_git(path, &["config", "user.name", "Worktree PTY"]);
    fs::write(path.join("README.md"), "seed\n").unwrap_or_abort();
    run_git(path, &["add", "README.md"]);
    run_git(path, &["commit", "-m", "seed"]);
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_abort();
    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_text(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_abort();
    assert!(
        output.status.success(),
        "git {:?} failed in {}: {}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn list_worktree_dirs(parent: &Path) -> Vec<PathBuf> {
    if !parent.is_dir() {
        return Vec::new();
    }
    let mut dirs = fs::read_dir(parent)
        .unwrap_or_abort()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn live_shell_markers_present(screen: &str) -> bool {
    (screen.contains("Shift+Tab:mode") || screen.contains("Ctrl+x:shortcuts"))
        && (screen.contains("Demo") || screen.contains("model-1") || screen.contains('❯'))
}

#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn wait_for_worktree_created_and_live_shell(
    helper: &mut SpawnedHarness,
    worktree_parent: &Path,
    label: &str,
) -> serde_json::Value {
    let deadline = Instant::now() + MARKER_TIMEOUT;
    loop {
        drain_output(&mut helper.parser, &helper.output_rx);
        let screen = helper.parser.screen().contents();
        let worktrees = list_worktree_dirs(worktree_parent);
        if !worktrees.is_empty() && live_shell_markers_present(&screen) {
            return json!({
                "label": label,
                "marker": "worktree_on_disk_and_live_shell",
                "screen": truncate_for_artifact(&screen),
                "worktree_count": worktrees.len(),
            });
        }

        if Instant::now() >= deadline {
            panic!(
                "PTY worktree journey timeout ({label})\nworktree_parent={}\nworktrees={worktrees:?}\n--- screen ---\n{screen}\n--- end ---",
                worktree_parent.display()
            );
        }

        if let Ok(chunk) = helper.output_rx.recv_timeout(READ_POLL_TIMEOUT) {
            helper.parser.process(&chunk);
        }
    }
}

#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn wait_for_resumed_live_shell(
    helper: &mut SpawnedHarness,
    seeded_run_id: &str,
    label: &str,
) -> serde_json::Value {
    let deadline = Instant::now() + MARKER_TIMEOUT.saturating_mul(2);
    loop {
        drain_output(&mut helper.parser, &helper.output_rx);
        let screen = helper.parser.screen().contents();
        let continued_identity = screen.contains("Continued")
            || screen.contains(seeded_run_id)
            || screen.contains(&format!("run {seeded_run_id}"));
        let prior_turn = screen.contains("Hello from PTY") || screen.contains("Hello world");
        let live_footer = screen.contains("Shift+Tab:mode") || screen.contains("Ctrl+x:shortcuts");
        if live_shell_markers_present(&screen) && continued_identity && prior_turn && live_footer {
            return json!({
                "label": label,
                "marker": "resumed_live_shell",
                "seeded_run_id": seeded_run_id,
                "screen": truncate_for_artifact(&screen),
            });
        }

        if Instant::now() >= deadline {
            panic!(
                "PTY resume journey timeout ({label})\nseeded_run_id={seeded_run_id}\n--- screen ---\n{screen}\n--- end ---"
            );
        }

        if let Ok(chunk) = helper.output_rx.recv_timeout(READ_POLL_TIMEOUT) {
            helper.parser.process(&chunk);
        }
    }
}

#[allow(
    clippy::panic,
    clippy::match_wild_err_arm,
    reason = "test code must panic gracefully"
)]
fn wait_for_screen_contains(
    parser: &mut Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
) -> String {
    let deadline = Instant::now() + MARKER_TIMEOUT;

    loop {
        drain_output(parser, output_rx);
        let current = parser.screen().contents();
        if current.contains(needle) {
            return current;
        }

        let now = Instant::now();
        if now >= deadline {
            panic!(
                "PTY marker timeout waiting for {needle:?}\n--- screen ---\n{current}\n--- end ---"
            );
        }

        let wait_timeout = READ_POLL_TIMEOUT.min(deadline.saturating_duration_since(now));
        if let Ok(chunk) = output_rx.recv_timeout(wait_timeout) {
            parser.process(&chunk);
        }
    }
}

#[allow(
    clippy::panic,
    reason = "fail-closed PTY test timeout waiting for absence"
)]
fn wait_for_screen_absent(
    parser: &mut Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
) -> String {
    let deadline = Instant::now() + MARKER_TIMEOUT;

    loop {
        drain_output(parser, output_rx);
        let current = parser.screen().contents();
        if !current.contains(needle) {
            return current;
        }

        let now = Instant::now();
        if now >= deadline {
            panic!(
                "PTY marker timeout waiting for absence of {needle:?}\n--- screen ---\n{current}\n--- end ---"
            );
        }

        let wait_timeout = READ_POLL_TIMEOUT.min(deadline.saturating_duration_since(now));
        if let Ok(chunk) = output_rx.recv_timeout(wait_timeout) {
            parser.process(&chunk);
        }
    }
}

fn drain_output(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 || tx.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

fn configure_deterministic_env(command: &mut CommandBuilder) {
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("HARNESS_SEED", "42");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");
}

fn newest_run_dir(session_dir: &Path) -> Option<PathBuf> {
    fs::read_dir(session_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("run_"))
        })
        .max_by_key(|path| path.metadata().and_then(|meta| meta.modified()).ok())
}

fn truncate_for_artifact(screen: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let mut truncated = screen.chars().take(MAX_CHARS).collect::<String>();
    if screen.chars().count() > MAX_CHARS {
        truncated.push('…');
    }
    truncated
}

fn timestamp_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
