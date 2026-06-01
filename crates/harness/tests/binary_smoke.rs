use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Output;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::Value;
use tempfile::tempdir;
use vt100::Parser;

const EXAMPLE_CONFIG: &str = include_str!("../../../configs/harness.example.jsonc");
const TUI_STARTUP_COLS: u16 = 100;
const TUI_STARTUP_ROWS: u16 = 30;
const TUI_STARTUP_EXIT_TIMEOUT: Duration = Duration::from_secs(15);
const TUI_READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);

#[test]
#[ignore = "T5 binary smoke; set HARNESS_BINARY_SMOKE=1 and run explicitly"]
fn harness_binary_supports_operator_first_run_smoke() {
    // arrange
    assert_eq!(
        std::env::var("HARNESS_BINARY_SMOKE").as_deref(),
        Ok("1"),
        "set HARNESS_BINARY_SMOKE=1 to run the T5 binary smoke"
    );
    let temp = tempdir().expect("tempdir");
    let smoke_artifacts_dir = std::env::var_os("HARNESS_BINARY_SMOKE_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| temp.path().join("binary-smoke-artifacts"));
    fs::create_dir_all(&smoke_artifacts_dir).expect("create binary smoke artifacts dir");
    let config_path = temp.path().join("harness.jsonc");
    let prompt_events_path = temp.path().join("prompt.events.jsonl");
    let tool_prompt_events_path = smoke_artifacts_dir.join("tool-prompt.events.jsonl");
    fs::write(&config_path, EXAMPLE_CONFIG).expect("write copied harness config");
    fs::create_dir_all(temp.path().join(".agent-harness"))
        .expect("create session directory parent");

    // act
    let help_output = harness_binary()
        .arg("--help")
        .output()
        .expect("run harness --help through real binary");
    let version_output = harness_binary()
        .arg("--version")
        .output()
        .expect("run harness --version through real binary");
    let validate_output = outside_repo_harness(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate through real binary outside repo");
    let doctor_output = outside_repo_harness(temp.path())
        .arg("doctor")
        .output()
        .expect("run harness doctor through real binary outside repo");
    let doctor_json_output = outside_repo_harness(temp.path())
        .args(["doctor", "--json"])
        .output()
        .expect("run harness doctor --json through real binary outside repo");
    let prompt_events_arg = prompt_events_path
        .to_str()
        .expect("prompt events path utf-8");
    let prompt_output = outside_repo_harness(temp.path())
        .args([
            "prompt",
            "--mock",
            "--text",
            "Hello from PTY",
            "--out",
            prompt_events_arg,
            "--print-run-dir",
        ])
        .output()
        .expect("run harness prompt --mock through real binary outside repo");
    let run_output = outside_repo_harness(temp.path())
        .args(["run", "--mock", "Hello"])
        .output()
        .expect("run harness run --mock through real binary outside repo");
    let tui_startup_output = run_tui_startup_through_pty(temp.path());
    let tool_prompt_events_arg = tool_prompt_events_path
        .to_str()
        .expect("tool prompt events path utf-8");
    let tool_prompt_output = outside_repo_harness(temp.path())
        .args([
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--out",
            tool_prompt_events_arg,
            "--print-run-dir",
        ])
        .output()
        .expect("run tool-enabled mock scenario through real binary outside repo");
    write_pty_output_artifact(&smoke_artifacts_dir, "tui-startup", &tui_startup_output);
    write_output_artifact(&smoke_artifacts_dir, "tool-prompt", &tool_prompt_output);

    // assert
    assert_success(&help_output);

    let stdout = String::from_utf8_lossy(&help_output.stdout);
    assert!(stdout.contains("Usage:"), "stdout:\n{stdout}");
    assert!(stdout.contains("config"), "stdout:\n{stdout}");

    assert_success(&version_output);

    let stdout = String::from_utf8_lossy(&version_output.stdout);
    assert!(
        stdout.trim() == format!("harness {}", env!("CARGO_PKG_VERSION")),
        "stdout:\n{stdout}"
    );

    assert_success(&validate_output);

    let stdout = String::from_utf8_lossy(&validate_output.stdout);
    assert!(stdout.contains("harness.jsonc"), "stdout:\n{stdout}");

    assert_success(&doctor_output);

    let stdout = String::from_utf8_lossy(&doctor_output.stdout);
    assert!(stdout.contains("doctor ok:"), "stdout:\n{stdout}");
    assert!(stdout.contains("resolved_routes"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("will launch only at runtime"),
        "stdout:\n{stdout}"
    );

    assert_success(&doctor_json_output);

    let report: Value =
        serde_json::from_slice(&doctor_json_output.stdout).expect("doctor json report");
    assert!(report["config"]
        .as_str()
        .expect("config display")
        .contains("harness.jsonc"));
    let route_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .expect("resolved_routes check");
    assert_eq!(route_check["status"], "pass");
    assert_eq!(route_check["details"]["no_network_probes"], true);
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["model"],
        "gpt-5.4-mini"
    );

    assert_success(&prompt_output);

    let stdout = String::from_utf8_lossy(&prompt_output.stdout);
    assert!(stdout.contains("Hello world"), "stdout:\n{stdout}");
    assert!(
        stdout.contains(".agent-harness/sessions/prompt_"),
        "stdout:\n{stdout}"
    );
    let prompt_events = fs::read_to_string(&prompt_events_path).expect("read prompt event log");
    assert!(prompt_events.contains("\"event_type\":\"task_completed\""));
    assert!(prompt_events.contains("Hello world"));

    assert_success(&run_output);
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("Hello world"), "stdout:\n{stdout}");

    let stdout = String::from_utf8_lossy(&tui_startup_output.stdout);
    let stderr = String::from_utf8_lossy(&tui_startup_output.stderr);
    assert!(
        tui_startup_output.success,
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("tui setup failed:"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("ctrl+p"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("commands"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        smoke_artifacts_dir.join("tui-startup.stdout.txt").is_file()
            && smoke_artifacts_dir.join("tui-startup.stderr.txt").is_file()
            && smoke_artifacts_dir.join("tui-startup.status.txt").is_file()
    );

    assert_success(&tool_prompt_output);

    let tool_prompt_events =
        fs::read_to_string(&tool_prompt_events_path).expect("read tool prompt event log");
    assert!(tool_prompt_events.contains("\"event_type\":\"tool_call_requested\""));
    assert!(tool_prompt_events.contains("\"event_type\":\"tool_call_finished\""));
    assert!(tool_prompt_events.contains("\"tool_id\":\"edit\""));
    assert!(tool_prompt_events.contains("demo.txt"));
    assert!(
        smoke_artifacts_dir.join("tool-prompt.stdout.txt").is_file()
            && smoke_artifacts_dir.join("tool-prompt.stderr.txt").is_file()
            && smoke_artifacts_dir.join("tool-prompt.status.txt").is_file()
    );
}

fn harness_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_harness"))
}

fn outside_repo_harness(workdir: &Path) -> Command {
    let mut command = harness_binary();
    command.current_dir(workdir);
    command.env_remove("HARNESS_CONFIG");
    command.env_remove("HARNESS_CONFIG_CONTENT");
    command.env("XDG_CONFIG_HOME", workdir.join("xdg"));
    command
}

struct PtySmokeOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    success: bool,
}

fn run_tui_startup_through_pty(workdir: &Path) -> PtySmokeOutput {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: TUI_STARTUP_ROWS,
            cols: TUI_STARTUP_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open TUI startup PTY pair");

    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_harness"));
    command.args(["tui", "--mock", "--exit-on-finish"]);
    command.cwd(workdir);
    command.env_remove("HARNESS_CONFIG");
    command.env_remove("HARNESS_CONFIG_CONTENT");
    command.env("XDG_CONFIG_HOME", workdir.join("xdg"));
    configure_tui_startup_env(&mut command);

    let mut child = pair
        .slave
        .spawn_command(command)
        .expect("spawn harness TUI startup PTY child");
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .expect("clone TUI startup PTY reader");
    let output_rx = spawn_pty_reader_thread(reader);
    let mut parser = Parser::new(TUI_STARTUP_ROWS, TUI_STARTUP_COLS, 0);
    let mut stdout = Vec::new();
    let mut startup_screen = None;
    let deadline = Instant::now() + TUI_STARTUP_EXIT_TIMEOUT;

    loop {
        drain_pty_output(&mut parser, &output_rx, &mut stdout);
        capture_startup_screen(&parser, &mut startup_screen);
        match child.try_wait() {
            Ok(status) => {
                let Some(status) = status else {
                    let now = Instant::now();
                    if now >= deadline {
                        drain_pty_output(&mut parser, &output_rx, &mut stdout);
                        append_rendered_screen(&mut stdout, startup_screen.as_deref(), &parser);
                        let _ = child.kill();
                        return PtySmokeOutput {
                            stdout,
                            stderr: format!(
                                "TUI startup PTY child did not exit after {TUI_STARTUP_EXIT_TIMEOUT:?}\n"
                            )
                            .into_bytes(),
                            success: false,
                        };
                    }

                    let wait_timeout =
                        TUI_READ_POLL_TIMEOUT.min(deadline.saturating_duration_since(now));
                    if let Ok(chunk) = output_rx.recv_timeout(wait_timeout) {
                        parser.process(&chunk);
                        stdout.extend_from_slice(&chunk);
                        capture_startup_screen(&parser, &mut startup_screen);
                    }
                    continue;
                };

                drain_pty_output(&mut parser, &output_rx, &mut stdout);
                append_rendered_screen(&mut stdout, startup_screen.as_deref(), &parser);
                return PtySmokeOutput {
                    stdout,
                    stderr: Vec::new(),
                    success: status.success(),
                };
            }
            Err(err) => {
                append_rendered_screen(&mut stdout, startup_screen.as_deref(), &parser);
                return PtySmokeOutput {
                    stdout,
                    stderr: format!("failed to wait for TUI startup PTY child: {err}\n")
                        .into_bytes(),
                    success: false,
                };
            }
        }
    }
}

fn configure_tui_startup_env(command: &mut CommandBuilder) {
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("HARNESS_SEED", "42");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");
}

fn spawn_pty_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
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

fn drain_pty_output(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>, output: &mut Vec<u8>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
        output.extend_from_slice(&chunk);
    }
}

fn capture_startup_screen(parser: &Parser, startup_screen: &mut Option<String>) {
    let screen = parser.screen().contents();
    if screen.contains("ctrl+p") && screen.contains("commands") {
        *startup_screen = Some(screen);
    }
}

fn append_rendered_screen(output: &mut Vec<u8>, startup_screen: Option<&str>, parser: &Parser) {
    if let Some(screen) = startup_screen {
        output.extend_from_slice(b"\n\n--- startup screen ---\n");
        output.extend_from_slice(screen.as_bytes());
    }
    output.extend_from_slice(b"\n\n--- final screen ---\n");
    output.extend_from_slice(parser.screen().contents().as_bytes());
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_output_artifact(dir: &Path, name: &str, output: &Output) {
    fs::write(dir.join(format!("{name}.stdout.txt")), &output.stdout)
        .expect("write stdout artifact");
    fs::write(dir.join(format!("{name}.stderr.txt")), &output.stderr)
        .expect("write stderr artifact");
    fs::write(
        dir.join(format!("{name}.status.txt")),
        format!("success={}\n", output.status.success()),
    )
    .expect("write status artifact");
}

fn write_pty_output_artifact(dir: &Path, name: &str, output: &PtySmokeOutput) {
    fs::write(dir.join(format!("{name}.stdout.txt")), &output.stdout)
        .expect("write stdout artifact");
    fs::write(dir.join(format!("{name}.stderr.txt")), &output.stderr)
        .expect("write stderr artifact");
    fs::write(
        dir.join(format!("{name}.status.txt")),
        format!("success={}\n", output.success),
    )
    .expect("write status artifact");
}
