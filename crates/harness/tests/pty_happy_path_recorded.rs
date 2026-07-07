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

fn record_prompt_and_quit(session_dir: &Path) -> serde_json::Value {
    let mut helper = spawn_harness_pty(&[
        "tui".to_string(),
        "--mock".to_string(),
        "--deterministic".to_string(),
        "--session-dir".to_string(),
        session_dir.display().to_string(),
    ]);
    let mut screens = Vec::new();
    screens.push(helper.wait_for("ctrl+p commands", "start"));
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
    screens.push(helper.wait_for("Permission required", "permission modal"));
    screens.push(helper.wait_for("edit fs request is paused for review", "permission summary"));
    helper.send_key(b'\r');
    screens.push(helper.wait_for("ready for next turn", "tool edit completed"));
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
    screens.push(helper.wait_for("ctrl+p commands", "resume startup"));
    helper.write_text("/");
    screens.push(helper.wait_for("Switch session", "slash commands"));
    helper.write_text("sessions");
    helper.send_key(b'\r');
    screens.push(helper.wait_for("Continue session", "resume picker"));
    helper.send_key(0x1b);
    screens.push(helper.wait_for("ctrl+p commands", "resume picker dismissed"));
    quit_helper(&mut helper, &mut screens);
    helper.wait_success("resume PTY child");

    json!({
        "stage": "resume_picker_and_quit",
        "command": format!("harness tui --mock --session-dir {}", session_dir.display()),
        "screens": screens,
    })
}

fn quit_helper(helper: &mut SpawnedHarness, screens: &mut Vec<serde_json::Value>) {
    helper.send_ctrl(b'p');
    screens.push(helper.wait_for("Commands", "quit palette"));
    helper.write_text("exit the app");
    screens.push(helper.wait_for("Exit the app", "quit command"));
    helper.send_key(b'\r');
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
                Err(_) => panic!("abort"),
            }

            drain_output(&mut self.parser, &self.output_rx);
            if Instant::now() >= deadline {
                let _final_screen = self.parser.screen().contents();
                let _ = self.child.kill();
                panic!("abort");
            }

            thread::sleep(READ_POLL_TIMEOUT);
        }
    }
}

fn spawn_harness_pty(args: &[String]) -> SpawnedHarness {
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
            panic!("abort");
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
