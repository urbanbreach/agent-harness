use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::cmp;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PTY_COLS: u16 = 80;
const PTY_ROWS: u16 = 24;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MARKER_TIMEOUT: Duration = Duration::from_secs(3);
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_tui_golden_path() {
    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let session_dir = create_temp_session_dir();

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
    command.arg("--scenario");
    command.arg("golden_path_interactive");
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.cwd(repo_root);
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn harness tui command");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let mut writer = pair.master.take_writer().expect("take pty writer");
    let output_rx = spawn_reader_thread(reader);
    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);

    wait_for_screen_contains(&mut parser, &output_rx, "Tabs", STARTUP_TIMEOUT)
        .expect("wait for initial TUI render");

    let permission_checkpoint = wait_for_screen_contains(
        &mut parser,
        &output_rx,
        "PermissionRequested",
        MARKER_TIMEOUT,
    )
    .unwrap_or_else(|_| screen_contents(&parser));
    insta::assert_snapshot!("pty_permission_requested", permission_checkpoint);

    send_key(writer.as_mut(), b'a').expect("send approve key");

    let run_finished_checkpoint =
        wait_for_screen_contains(&mut parser, &output_rx, "RunFinished", MARKER_TIMEOUT)
            .unwrap_or_else(|_| screen_contents(&parser));
    insta::assert_snapshot!("pty_run_finished", run_finished_checkpoint);

    send_key(writer.as_mut(), b'3').expect("switch to diff tab");
    wait_for_screen_contains(&mut parser, &output_rx, "Diff tab placeholder", MARKER_TIMEOUT)
        .expect("wait for diff tab marker");
    insta::assert_snapshot!("pty_diff_tab", screen_contents(&parser));

    send_key(writer.as_mut(), b'q').expect("send quit key");
    drop(writer);

    let status = wait_for_child_exit(child, EXIT_TIMEOUT);
    assert!(
        status.success(),
        "expected harness tui to exit with status 0, got {status:?}"
    );
}

fn wait_for_screen_contains(
    parser: &mut vt100::Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        let current = screen_contents(parser);
        if current.contains(needle) {
            return Ok(current);
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for screen marker '{needle}' after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(
            READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "pty output stream closed while waiting for '{needle}'; last screen:\n{current}"
                ));
            }
        }
    }
}

fn drain_output(parser: &mut vt100::Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn screen_contents(parser: &vt100::Parser) -> String {
    parser.screen().contents()
}

fn send_key(writer: &mut dyn Write, key: u8) -> std::io::Result<()> {
    writer.write_all(&[key])?;
    writer.flush()
}

fn wait_for_child_exit(
    mut child: Box<dyn portable_pty::Child + Send>,
    timeout: Duration,
) -> portable_pty::ExitStatus {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = child.wait();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => panic!("wait for harness process failed: {err}"),
        Err(RecvTimeoutError::Timeout) => {
            panic!("timed out waiting {timeout:?} for harness process to exit")
        }
        Err(RecvTimeoutError::Disconnected) => {
            panic!("harness process wait channel disconnected before receiving status")
        }
    }
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

    let harness_bin = repo.join("target").join("debug").join(binary_name("harness"));
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
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn create_temp_session_dir() -> PathBuf {
    let base = std::env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp session dir");

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let dir = base.join(format!("pty-e2e-{}-{suffix}", std::process::id()));
    fs::create_dir_all(&dir).expect("create unique temp session dir");
    dir
}

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}
