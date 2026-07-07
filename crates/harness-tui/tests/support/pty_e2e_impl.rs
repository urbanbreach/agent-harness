use harness_tui::UnwrapOrAbort;
use harness_tui::{run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::cmp;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const PTY_SIGNOFF_ENV: &str = "HARNESS_TUI_PTY_SIGNOFF";
const HELPER_SCENARIO_ENV: &str = "HARNESS_TUI_PTY_HELPER_SCENARIO";
const TYPE_FIRST_STARTUP_SCENARIO: &str = "type_first_startup";
const CONNECT_AUTH_SCENARIO: &str = "connect_auth";
const TYPE_FIRST_STARTUP_TEST: &str = "pty_helper_type_first_startup";
const CONNECT_AUTH_TEST: &str = "pty_helper_connect_auth";
const READY_MARKER: &str = "Ctrl+p commands";
const DRAFT_TEXT: &str = "Hello from PTY";

const PRIMARY_COLS: u16 = 100;
const PRIMARY_ROWS: u16 = 30;
const MINIMUM_COLS: u16 = 80;
const MINIMUM_ROWS: u16 = 24;

pub(crate) fn pty_smoke_starts_accepts_input_resizes_and_exits() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for(READY_MARKER);

    helper
        .writer
        .write_all(DRAFT_TEXT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(DRAFT_TEXT);

    helper
        .master
        .resize(pty_size(MINIMUM_COLS, MINIMUM_ROWS))
        .unwrap_or_abort();
    helper.parser = Parser::new(MINIMUM_ROWS, MINIMUM_COLS, 0);
    helper.wait_for(READY_MARKER);

    send_key(helper.writer.as_mut(), 0x10).unwrap_or_abort();
    helper.wait_for("Commands");
    helper.writer.write_all(b"exit the app").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("Exit the app");
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();
    let status = helper.child.wait().unwrap_or_abort();
    assert!(status.success(), "helper tui child exited with {status:?}");
}

pub(crate) fn pty_connect_auth_drives_provider_connection() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_helper(CONNECT_AUTH_TEST, CONNECT_AUTH_SCENARIO);
    helper.wait_for("ctrl+p commands");

    send_key(helper.writer.as_mut(), b'/').unwrap_or_abort();
    helper.writer.write_all(b"connect").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("connect");
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();

    std::thread::sleep(std::time::Duration::from_millis(500));
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();

    std::thread::sleep(std::time::Duration::from_millis(200));
    helper.writer.write_all(b"/exit").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();

    let status = helper.child.wait().unwrap_or_abort();
    assert!(
        status.success(),
        "connect helper tui child exited with {status:?}"
    );
}

pub(crate) fn pty_helper_type_first_startup() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(TYPE_FIRST_STARTUP_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (_keepalive, update_rx) = mpsc::channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_connect_auth() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(CONNECT_AUTH_SCENARIO) {
        return;
    }

    let (update_tx, update_rx) = mpsc::channel();
    let auth_tx = update_tx.clone();
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        if matches!(intent, UiIntent::OpenAuthManager { .. }) {
            auth_tx
                .send(LiveUpdate::AuthBackendResult { success: true })
                .unwrap_or_abort();
        }
    });

    run_tui_with_options(TuiOptions {
        mode: TuiMode::Startup {
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
        },
        exit_on_finish: false,
        on_ui_intent: Some(on_ui_intent),
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    })
    .unwrap_or_abort();
}

struct SpawnedHelper {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
}

impl SpawnedHelper {
    fn wait_for(&mut self, needle: &str) {
        wait_for_screen_contains(&mut self.parser, &self.output_rx, needle);
    }
}

fn spawn_type_first_startup_helper() -> SpawnedHelper {
    spawn_helper(TYPE_FIRST_STARTUP_TEST, TYPE_FIRST_STARTUP_SCENARIO)
}

fn spawn_helper(test_name: &str, scenario: &str) -> SpawnedHelper {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(PRIMARY_COLS, PRIMARY_ROWS))
        .unwrap_or_abort();

    let current_test_bin = std::env::current_exe().unwrap_or_abort();
    let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
    command.arg("--exact");
    command.arg(test_name);
    command.arg("--nocapture");
    command.env(HELPER_SCENARIO_ENV, scenario);
    configure_deterministic_env(&mut command);

    let child = pair.slave.spawn_command(command).unwrap_or_abort();
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().unwrap_or_abort();
    let writer = pair.master.take_writer().unwrap_or_abort();
    let output_rx = spawn_reader_thread(reader);

    SpawnedHelper {
        master: pair.master,
        child,
        writer,
        output_rx,
        parser: Parser::new(PRIMARY_ROWS, PRIMARY_COLS, 0),
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn wait_for_screen_contains(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>, needle: &str) {
    let deadline = Instant::now() + MARKER_TIMEOUT;

    loop {
        drain_output(parser, output_rx);
        let current = parser.screen().contents();
        if current.contains(needle) {
            return;
        }

        let now = Instant::now();
        if now >= deadline {
            let _ = needle;
            let _ = &current;
            let _ = MARKER_TIMEOUT;
            panic!("abort");
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
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

fn send_key(writer: &mut dyn Write, key: u8) -> std::io::Result<()> {
    writer.write_all(&[key])?;
    writer.flush()
}

fn send_bytes(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
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
