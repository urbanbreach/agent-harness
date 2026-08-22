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

/// Write a phase breadcrumb to stderr so a timed-out PTY test reports the
/// exact sub-journey that stalled. `eprintln!` flushes stderr immediately.
fn pty_phase(label: &str) {
    eprintln!("[pty-happy-path] phase={label}");
}

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

    let checkout_before = checkout_status(Some(&artifact_dir));
    let tracked_drift_path = repo_root().join("crates/harness/src/drift.rs");
    let tracked_drift_before = fs::read(&tracked_drift_path).unwrap_or_abort();
    let harness_json_path = repo_root().join("crates/harness/harness.json");
    assert!(!harness_json_path.exists());

    let temp = tempdir().unwrap_or_abort();
    let prompt_session_dir = temp.path().join("prompt-sessions");
    let scenario_session_dir = temp.path().join("scenario-sessions");

    pty_phase("main:record_prompt");
    let prompt_recording = record_prompt_and_quit(&prompt_session_dir);
    let prompt_run_dir = newest_run_dir(&prompt_session_dir).unwrap_or_abort();
    let prompt_events_path = prompt_run_dir.join("events.jsonl");
    let prompt_events = fs::read_to_string(&prompt_events_path).unwrap_or_abort();
    assert!(prompt_events.contains("\"event_type\":\"user_message_submitted\""));
    assert!(prompt_events.contains("Hello from PTY"));

    pty_phase("main:record_permission_tool_edit");
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
        assert!(scenario_events.contains(marker));
    }

    pty_phase("main:record_resume_picker");
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
    assert_eq!(checkout_before, checkout_status(Some(&artifact_dir)));
    assert!(!harness_json_path.exists());
    assert_eq!(
        tracked_drift_before,
        fs::read(&tracked_drift_path).unwrap_or_abort()
    );
}

fn record_prompt_and_quit(session_dir: &Path) -> serde_json::Value {
    pty_phase("prompt:spawn");
    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);
    let mut screens = Vec::new();
    pty_phase("prompt:wait_for_composer");
    screens.push(helper.wait_for("❯", "start"));
    pty_phase("prompt:type");
    helper.write_text("Hello from PTY");
    pty_phase("prompt:wait_for_draft");
    screens.push(helper.wait_for("Hello from PTY", "prompt draft"));
    pty_phase("prompt:submit");
    helper.send_key(b'\r');
    pty_phase("prompt:wait_for_response");
    screens.push(helper.wait_for("Hello world", "prompt response"));
    pty_phase("prompt:quit");
    quit_helper(&mut helper, &mut screens);
    pty_phase("prompt:wait_success");
    helper.wait_success("prompt PTY child");
    pty_phase("prompt:done");

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
    pty_phase("scenario:spawn");
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
    pty_phase("scenario:wait_permission_modal");
    screens.push(helper.wait_for("Allow Edit", "permission modal"));
    pty_phase("scenario:wait_permission_choices");
    screens.push(helper.wait_for("always-approve", "permission choices"));
    pty_phase("scenario:allow");
    helper.send_key(b'\r');
    helper.send_key(b'\r');
    pty_phase("scenario:wait_tool_completed");
    screens.push(helper.wait_until_absent("Allow Edit", "tool edit completed"));
    pty_phase("scenario:wait_success");
    helper.wait_success("scenario PTY child");
    pty_phase("scenario:done");

    json!({
        "stage": "permission_tool_edit_auto_exit",
        "command": format!("harness tui --scenario golden_path_interactive --deterministic --exit-on-finish --session-dir {}", session_dir.display()),
        "screens": screens,
    })
}

fn record_resume_picker_and_quit(session_dir: &Path) -> serde_json::Value {
    pty_phase("resume:spawn");
    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);
    let mut screens = Vec::new();
    pty_phase("resume:wait_composer");
    screens.push(helper.wait_for("❯", "resume startup"));
    // Filter first: unfiltered slash list is alpha-sorted and hides /sessions off-screen.
    pty_phase("resume:type_slash");
    helper.write_text("/");
    helper.write_text("sessions");
    pty_phase("resume:wait_switch_session");
    screens.push(helper.wait_for("Switch session", "slash commands"));
    pty_phase("resume:select_switch_session");
    helper.send_key(b'\r');
    pty_phase("resume:wait_resume_picker");
    screens.push(helper.wait_for("Resume session", "resume picker"));
    pty_phase("resume:dismiss_picker");
    helper.send_key(0x1b);
    pty_phase("resume:wait_picker_absent");
    // The composer prompt "❯" is visible behind the overlay, so checking for
    // "❯" alone is not enough to prove the picker was dismissed. The welcome
    // screen also contains "Resume session", so wait for a picker-only row to
    // disappear.
    screens.push(helper.wait_until_absent("↑↓ nav", "resume picker dismissed"));
    pty_phase("resume:wait_composer_return");
    screens.push(helper.wait_for("❯", "composer focus after picker dismiss"));
    pty_phase("resume:quit");
    quit_helper(&mut helper, &mut screens);
    pty_phase("resume:wait_success");
    helper.wait_success("resume PTY child");
    pty_phase("resume:done");

    json!({
        "stage": "resume_picker_and_quit",
        "command": format!("harness tui --mock --session-dir {}", session_dir.display()),
        "screens": screens,
    })
}

fn quit_helper(helper: &mut SpawnedHarness, screens: &mut Vec<serde_json::Value>) {
    screens.push(json!({
        "label": "settled before quit",
        "marker": "ready",
        "screen": truncate_for_artifact(&helper.screen_text()),
    }));
    helper.send_ctrl(b'q');
    helper.send_ctrl(b'q');
}

struct SpawnedHarness {
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
    _workspace: Option<tempfile::TempDir>,
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
    let workspace = tempdir().unwrap_or_abort();
    init_git_repo_for_workspace(workspace.path());
    let cwd = workspace.path().to_path_buf();
    spawn_harness_pty_in_owned(&cwd, args, Some(workspace), &[])
}

fn init_git_repo_for_workspace(path: &Path) {
    fs::create_dir_all(path).unwrap_or_abort();
    run_git(path, &["init", "-b", "main"]);
    run_git(path, &["config", "user.email", "pty-test@example.com"]);
    run_git(path, &["config", "user.name", "PTY Test"]);
    fs::write(path.join("README.md"), "# PTY test workspace\n").unwrap_or_abort();
    run_git(path, &["add", "README.md"]);
    run_git(path, &["commit", "-m", "seed workspace"]);
}

fn spawn_harness_pty_in(cwd: &Path, args: &[String]) -> SpawnedHarness {
    spawn_harness_pty_in_owned(cwd, args, None, &[])
}

fn spawn_harness_pty_in_owned(
    cwd: &Path,
    args: &[String],
    workspace: Option<tempfile::TempDir>,
    environment: &[(&str, &Path)],
) -> SpawnedHarness {
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
    for (name, value) in environment {
        command.env(*name, *value);
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
        _workspace: workspace,
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

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn checkout_status(artifact_dir: Option<&Path>) -> Vec<String> {
    let root = repo_root();
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(&root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap_or_abort();
    assert!(
        output.status.success(),
        "git status failed in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let artifact_rel = artifact_dir.and_then(|dir| dir.strip_prefix(&root).ok().map(PathBuf::from));
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter(|line| {
            if line.len() < 3 {
                return true;
            }
            let path_field = &line[3..];
            let path = path_field.split(" -> ").last().unwrap_or(path_field);
            if let Some(prefix) = artifact_rel.as_deref() {
                !Path::new(path).starts_with(prefix)
            } else {
                true
            }
        })
        .map(|line| line.to_string())
        .collect()
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
    screen.contains('❯') && screen.contains("model-1")
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
        let resumed_chrome = live_shell_markers_present(&screen);
        if resumed_chrome && continued_identity && prior_turn {
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
    const MAX_CHARS: usize = 5_000;
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
