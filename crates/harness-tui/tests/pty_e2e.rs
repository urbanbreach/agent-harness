use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::cmp;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use vt100::Parser;

const PTY_COLS: u16 = 100;
const PTY_ROWS: u16 = 30;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const STABLE_WINDOW: Duration = Duration::from_millis(180);
const STABLE_TIMEOUT: Duration = Duration::from_secs(2);

#[test]
fn pty_e2e_snapshots_are_stable() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let session_root = tempfile::tempdir().expect("create temp session root");
    let config_path = write_test_config(&session_root);

    let startup_and_prompt = capture_interactive_snapshots(&harness_bin, &repo_root, &config_path);
    assert_or_update_snapshot("startup", &startup_and_prompt.startup);
    assert_or_update_snapshot("after_prompt", &startup_and_prompt.after_prompt);

    let after_tool_call = capture_tool_call_snapshot(&harness_bin, &repo_root, &config_path);
    assert_or_update_snapshot("after_tool_call", &after_tool_call);

    assert_snapshot_secrets_clean();
}

#[test]
fn snapshot_files_exist_and_are_secret_clean() {
    let snapshot_dir = snapshot_dir();
    let expected = [
        snapshot_dir.join("startup.snap"),
        snapshot_dir.join("after_prompt.snap"),
        snapshot_dir.join("after_tool_call.snap"),
    ];

    for path in expected {
        assert!(path.exists(), "missing snapshot file: {}", path.display());
    }

    assert_snapshot_secrets_clean();
}

struct InteractiveSnapshots {
    startup: String,
    after_prompt: String,
}

fn capture_interactive_snapshots(
    harness_bin: &Path,
    repo_root: &Path,
    config_path: &Path,
) -> InteractiveSnapshots {
    let session_dir = tempfile::tempdir().expect("create temp interactive session dir");
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty pair");

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--config");
    command.arg(config_path.to_string_lossy().to_string());
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.path().to_string_lossy().to_string());
    command.cwd(repo_root);
    configure_deterministic_env(&mut command);

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn interactive harness tui");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let mut writer = pair.master.take_writer().expect("take pty writer");
    let output_rx = spawn_reader_thread(reader);
    let mut parser = Parser::new(PTY_ROWS, PTY_COLS, 0);

    let startup = wait_for_screen_contains(&mut parser, &output_rx, "Prompt", STARTUP_TIMEOUT)
        .expect("wait for startup screen");

    send_key(writer.as_mut(), b'\t').expect("focus prompt pane");
    send_key(writer.as_mut(), b'\t').expect("focus prompt input");
    writer
        .write_all(b"Hello from PTY")
        .expect("type prompt text");
    writer.flush().expect("flush prompt text");
    send_key(writer.as_mut(), b'\r').expect("submit prompt");

    let after_prompt =
        wait_for_screen_contains(&mut parser, &output_rx, "Hello world", MARKER_TIMEOUT)
            .expect("wait for prompt response");

    drop(writer);
    terminate_child(child);

    InteractiveSnapshots {
        startup: normalize_snapshot(&startup),
        after_prompt: normalize_snapshot(&after_prompt),
    }
}

fn capture_tool_call_snapshot(harness_bin: &Path, repo_root: &Path, config_path: &Path) -> String {
    let session_dir = tempfile::tempdir().expect("create temp scenario session dir");
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open pty pair");

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--config");
    command.arg(config_path.to_string_lossy().to_string());
    command.arg("--scenario");
    command.arg("golden_path_interactive");
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.path().to_string_lossy().to_string());
    command.cwd(repo_root);
    configure_deterministic_env(&mut command);

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn scenario harness tui");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let mut writer = pair.master.take_writer().expect("take pty writer");
    let output_rx = spawn_reader_thread(reader);
    let mut parser = Parser::new(PTY_ROWS, PTY_COLS, 0);

    wait_for_screen_contains(
        &mut parser,
        &output_rx,
        "Permission Requested",
        MARKER_TIMEOUT,
    )
    .expect("wait for permission prompt");
    send_key(writer.as_mut(), b'a').expect("approve permission");
    send_key(writer.as_mut(), b' ').expect("disable follow mode for stable capture");
    for _ in 0..64 {
        send_key(writer.as_mut(), b'k').expect("move to first activity");
    }

    let after_tool_call = wait_for_screen_contains_once(
        &mut parser,
        &output_rx,
        "edit.hashline_apply",
        MARKER_TIMEOUT,
    )
    .expect("wait for tool-call output marker");

    drop(writer);
    terminate_child(child);

    normalize_snapshot(&after_tool_call)
}

fn assert_or_update_snapshot(name: &str, actual: &str) {
    let path = snapshot_dir().join(format!("{name}.snap"));
    if std::env::var("HARNESS_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        fs::create_dir_all(snapshot_dir()).expect("create snapshot directory");
        fs::write(&path, actual).expect("write updated snapshot file");
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "failed to read snapshot {} ({err}); run with HARNESS_UPDATE_SNAPSHOTS=1 to generate baselines",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected,
        "snapshot mismatch for {}; run with HARNESS_UPDATE_SNAPSHOTS=1 to accept changes",
        path.display()
    );
}

fn assert_snapshot_secrets_clean() {
    let dir = snapshot_dir();
    if !dir.exists() {
        return;
    }

    for entry in fs::read_dir(&dir).expect("read snapshot directory") {
        let path = entry.expect("snapshot dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("snap") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read snapshot file");
        assert!(
            !text.contains("sk-"),
            "secret-like token found in snapshot {}",
            path.display()
        );
    }
}

fn normalize_snapshot(input: &str) -> String {
    let normalized = input
        .lines()
        .map(|line| normalize_volatile_line(line.trim_end()))
        .collect::<Vec<_>>()
        .join("\n");
    normalized.trim_end().to_string()
}

fn normalize_volatile_line(line: &str) -> String {
    let Some(marker_idx) = line.find("Sequences:") else {
        return line.to_string();
    };

    let trailing_border = line.ends_with('│').then_some(" │").unwrap_or_default();
    format!("{}Sequences: <RANGE>{trailing_border}", &line[..marker_idx])
}

fn wait_for_screen_contains(
    parser: &mut Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        let current = parser.screen().contents();
        if current.contains(needle) {
            return Ok(stabilize_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for marker '{needle}' after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "pty output disconnected while waiting for '{needle}'; final screen:\n{current}"
                ));
            }
        }
    }
}

fn wait_for_screen_contains_once(
    parser: &mut Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        let current = parser.screen().contents();
        if current.contains(needle) {
            return Ok(current);
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for marker '{needle}' after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "pty output disconnected while waiting for '{needle}'; final screen:\n{current}"
                ));
            }
        }
    }
}

fn stabilize_screen(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>, initial: String) -> String {
    let mut latest = initial;
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest;
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return latest,
        }

        let current = parser.screen().contents();
        if current != latest {
            latest = current;
            stable_since = Instant::now();
            continue;
        }

        if Instant::now().saturating_duration_since(stable_since) >= STABLE_WINDOW {
            return latest;
        }
    }
}

fn drain_output(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn send_key(writer: &mut dyn Write, key: u8) -> std::io::Result<()> {
    writer.write_all(&[key])?;
    writer.flush()
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}

fn terminate_child(mut child: Box<dyn portable_pty::Child + Send>) {
    child.kill().expect("terminate harness tui child");
    std::mem::forget(child);
}

fn write_test_config(session_root: &TempDir) -> PathBuf {
    let config_path = session_root.path().join("pty-test-config.jsonc");
    let body = format!(
        r#"{{
  backgroundTask: {{
    defaultConcurrency: 2,
    providerConcurrency: 2,
    modelConcurrency: 2,
    staleTimeoutMs: 15000,
    messageStalenessTimeoutMs: 5000,
  }},
  providers: {{
    default: {{
      type: "openai_compatible",
      base_url: "http://127.0.0.1:1/v1",
      api_key: "test-key",
      api_mode: "responses",
      models: {{
        "model-1": {{
          display_name: "Model 1",
        }},
      }},
    }},
  }},
  categories: {{
    deep: {{
      description: "deep",
      model_ref: "default:model-1",
      tools: ["read"],
    }},
  }},
  permissions: {{
    edit: "ask",
    shell: "deny",
    network: "deny",
  }},
  paths: {{
    session_dir: "{}",
  }},
  deterministic: {{
    enabled: true,
    seed: 42,
  }},
  ui: {{
    default_profile: "worker",
  }},
}}"#,
        session_root.path().display()
    );

    fs::write(&config_path, body).expect("write pty test config");
    config_path
}

fn configure_deterministic_env(command: &mut CommandBuilder) {
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}

fn resolve_harness_bin() -> PathBuf {
    if let Ok(path) = std::env::var("HARNESS_BIN") {
        let harness_bin = PathBuf::from(path);
        assert!(
            harness_bin.exists(),
            "HARNESS_BIN points to missing path: {}",
            harness_bin.display()
        );
        return harness_bin;
    }

    let repo = repo_root();
    let existing = repo
        .join("target")
        .join("debug")
        .join(binary_name("harness"));
    if existing.exists() {
        return existing;
    }

    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("harness")
        .current_dir(&repo)
        .status()
        .expect("spawn cargo build -p harness");
    assert!(
        status.success(),
        "cargo build -p harness failed with status {status}"
    );

    let harness_bin = repo
        .join("target")
        .join("debug")
        .join(binary_name("harness"));
    assert!(
        harness_bin.exists(),
        "expected harness binary at {}",
        harness_bin.display()
    );
    harness_bin
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("harness-tui should live under <repo>/crates/harness-tui")
        .to_path_buf()
}

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}
