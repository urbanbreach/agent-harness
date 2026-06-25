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
const ONBOARDING_AUTH_SCENARIO: &str = "onboarding_auth";
const TYPE_FIRST_STARTUP_TEST: &str = "pty_helper_type_first_startup";
const ONBOARDING_AUTH_TEST: &str = "pty_helper_onboarding_auth";
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
        .expect("type smoke draft");
    helper.writer.flush().expect("flush smoke draft");
    helper.wait_for(DRAFT_TEXT);

    helper
        .master
        .resize(pty_size(MINIMUM_COLS, MINIMUM_ROWS))
        .expect("resize smoke pty");
    helper.parser = Parser::new(MINIMUM_ROWS, MINIMUM_COLS, 0);
    helper.wait_for(READY_MARKER);

    send_key(helper.writer.as_mut(), 0x10).expect("open smoke command palette");
    helper.wait_for("Commands");
    helper
        .writer
        .write_all(b"quit")
        .expect("filter smoke command palette to quit");
    helper
        .writer
        .flush()
        .expect("flush smoke command palette filter");
    send_key(helper.writer.as_mut(), b'\r').expect("submit smoke quit command");
    let status = helper.child.wait().expect("wait for helper tui child exit");
    assert!(status.success(), "helper tui child exited with {status:?}");
}

pub(crate) fn pty_onboarding_auth_drives_required_screens() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_helper(ONBOARDING_AUTH_TEST, ONBOARDING_AUTH_SCENARIO);
    helper.wait_for("Harness setup");
    helper.wait_for("Connect a provider");

    send_key(helper.writer.as_mut(), b'\r').expect("start onboarding setup");
    helper.wait_for("Select provider");
    send_key(helper.writer.as_mut(), b'\r').expect("choose Codex provider");
    helper.wait_for("Choose login method");
    send_bytes(helper.writer.as_mut(), b"\x1b[B").expect("move to browser auth method");
    send_bytes(helper.writer.as_mut(), b"\x1b[B").expect("move to api-key auth method");
    send_key(helper.writer.as_mut(), b'\r').expect("choose API key auth method");
    helper.wait_for("secret redacted");

    helper
        .writer
        .write_all(b"sk-pty-redacted-test")
        .expect("type hidden API key");
    helper.writer.flush().expect("flush hidden API key");
    send_key(helper.writer.as_mut(), b'\r').expect("submit hidden API key");
    helper.wait_for("Credential stored");
    helper.wait_for("secret redacted");

    send_key(helper.writer.as_mut(), b'\r').expect("continue to skill selection");
    helper.wait_for("Select skills");
    send_key(helper.writer.as_mut(), b'\r').expect("finish onboarding and return to start shell");
    helper.wait_for("ctrl+p commands");
    send_key(helper.writer.as_mut(), b'\r').expect("start new session from launcher");

    let status = helper
        .child
        .wait()
        .expect("wait for onboarding helper exit");
    assert!(
        status.success(),
        "onboarding helper tui child exited with {status:?}"
    );
}

pub(crate) fn pty_helper_type_first_startup() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(TYPE_FIRST_STARTUP_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().expect("create temp helper run dir");
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
    .expect("run type-first helper tui");
}

pub(crate) fn pty_helper_onboarding_auth() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(ONBOARDING_AUTH_SCENARIO) {
        return;
    }

    let (update_tx, update_rx) = mpsc::channel();
    let auth_tx = update_tx.clone();
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        if matches!(intent, UiIntent::OpenAuthManager { .. }) {
            auth_tx
                .send(LiveUpdate::AuthBackendResult { success: true })
                .expect("send mocked auth backend result");
        }
    });

    run_tui_with_options(TuiOptions {
        mode: TuiMode::Startup {
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            onboarding_required: true,
            update_rx,
        },
        exit_on_finish: false,
        on_ui_intent: Some(on_ui_intent),
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    })
    .expect("run onboarding helper tui");
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
        .expect("open helper pty pair");

    let current_test_bin = std::env::current_exe().expect("resolve current test binary");
    let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
    command.arg("--exact");
    command.arg(test_name);
    command.arg("--nocapture");
    command.env(HELPER_SCENARIO_ENV, scenario);
    configure_deterministic_env(&mut command);

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn helper test binary");
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .expect("clone helper pty reader");
    let writer = pair.master.take_writer().expect("take helper pty writer");
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
            panic!(
                "timed out waiting for marker '{needle}' after {MARKER_TIMEOUT:?}; final screen:\n{current}"
            );
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
