#[path = "support/p0_03_pty.rs"]
mod scenario;

use harness_tui::UnwrapOrAbort;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const PRIMARY_COLS: u16 = 100;
const PRIMARY_ROWS: u16 = 30;
const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const SYNC_OUTPUT_START: &[u8] = b"\x1b[?2026h";
const SYNC_OUTPUT_END: &[u8] = b"\x1b[?2026l";

#[test]
fn p0_03_pty_helper() {
    // arrange
    // Given: direct invocation opts into the shared deterministic scenario.
    // act
    // When: the real Harness TUI runs inside the caller's PTY.
    scenario::run_helper();
    // assert
    // Then: run_helper prints the external-driver contract after a clean exit.
}

#[test]
fn p0_03_helper_fixture_contract_without_xterm() {
    // arrange
    // Given: the recorded fixture's source and event chunks.
    // act
    // When: the fixture contract is checked without launching a terminal.
    scenario::assert_fixture_contract();
    // assert
    // Then: all required content and event-driven marker boundaries are present.
}

#[test]
fn p0_03_real_pty_records_markdown_and_event_driven_fence() {
    // arrange
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // Given: the real Harness TUI has rendered the table and opened a Rust fence.
    let mut helper = Helper::spawn();
    helper.wait_for(scenario::READY_MARKER);
    let ready_screen = helper.screen();
    assert!(
        ready_screen.contains('┌') && ready_screen.contains('└'),
        "markdown table must render as a box\n{ready_screen}"
    );
    assert!(
        ready_screen.contains("nested _emphasis_"),
        "nested emphasis must remain visible\n{ready_screen}"
    );
    assert!(
        ready_screen.contains("東京"),
        "CJK text must remain visible\n{ready_screen}"
    );
    assert!(
        ready_screen.contains("👩"),
        "emoji must remain visible\n{ready_screen}"
    );
    assert!(
        ready_screen.contains("valid"),
        "valid link label must remain visible\n{ready_screen}"
    );
    assert!(
        ready_screen.contains("unsafe"),
        "unsafe link label must remain visible\n{ready_screen}"
    );
    assert!(
        !ready_screen.contains("unsafe)"),
        "markdown destination delimiter leaked into visible text\n{ready_screen}"
    );

    // act
    // When: the first exact fixture command requests the next live event chunk.
    helper.send(scenario::CHUNK_1_COMMAND.as_bytes());
    helper.send(b"\r");
    helper.wait_for(scenario::FENCE_CHUNK_1_MARKER);

    // When: the second exact fixture command requests the closing chunk and settlement.
    helper.send(scenario::CHUNK_2_COMMAND.as_bytes());
    helper.send(b"\r");
    helper.wait_for(scenario::FENCE_CHUNK_2_MARKER);
    helper.wait_for(scenario::SETTLED_MARKER);

    // assert
    // Then: the response settles only after both event-driven chunks are visible.
    let settled_screen = helper.screen();
    assert!(
        settled_screen.contains("QA-P0-03-FENCE-CHUNK-2")
            && settled_screen.contains(scenario::SETTLED_MARKER),
        "settled marker must follow the second fence chunk\n{settled_screen}"
    );

    // And: the helper exits through the real command palette and reports its contract.
    helper.exit_via_palette();
    helper.wait_for_raw(scenario::HELPER_CONTRACT);
    println!("{}", scenario::HELPER_CONTRACT);
}

struct Helper {
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
    raw: Vec<u8>,
    synchronized_output: bool,
    control_tail: Vec<u8>,
}

impl Drop for Helper {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Drop cannot report cleanup errors without panicking and masking the original failure.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Helper {
    fn spawn() -> Self {
        let pair = native_pty_system()
            .openpty(size(PRIMARY_COLS, PRIMARY_ROWS))
            .unwrap_or_abort();
        let current_test_bin = std::env::current_exe().unwrap_or_abort();
        let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
        command.arg("--exact");
        command.arg("p0_03_pty_helper");
        command.arg("--nocapture");
        command.env(scenario::SCENARIO_ENV, "1");
        deterministic_env(&mut command);
        let child = pair.slave.spawn_command(command).unwrap_or_abort();
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().unwrap_or_abort();
        let writer = pair.master.take_writer().unwrap_or_abort();
        Self {
            master: pair.master,
            child: Some(child),
            writer,
            output_rx: reader_channel(reader),
            parser: Parser::new(PRIMARY_ROWS, PRIMARY_COLS, 0),
            raw: Vec::new(),
            synchronized_output: false,
            control_tail: Vec::new(),
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap_or_abort();
        self.writer.flush().unwrap_or_abort();
    }

    fn wait_for(&mut self, needle: &str) {
        self.wait_for_screen(needle, true);
    }

    #[allow(
        clippy::panic,
        reason = "bounded PTY test failure needs screen evidence"
    )]
    fn wait_for_screen(&mut self, needle: &str, present: bool) {
        let deadline = Instant::now() + MARKER_TIMEOUT;
        loop {
            let screen = self.screen();
            if screen.contains(needle) == present && !self.synchronized_output {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output_rx.recv_timeout(remaining) {
                Ok(chunk) => self.process(chunk),
                Err(RecvTimeoutError::Timeout) => panic!(
                    "timed out waiting for {needle:?} present={present}\n{}",
                    self.screen()
                ),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!(
                        "PTY output closed waiting for {needle:?}\n{}",
                        self.screen()
                    )
                }
            }
        }
    }

    fn screen(&self) -> String {
        self.parser.screen().contents()
    }

    #[allow(
        clippy::panic,
        reason = "bounded PTY child failure needs explicit evidence"
    )]
    fn exit_via_palette(&mut self) {
        self.send(&[0x10]);
        self.wait_for("Commands");
        self.send(b"exit the app");
        self.wait_for("Exit the app");
        self.send(b"\r");
        let mut child = self.child.take().unwrap_or_abort();
        let mut killer = child.clone_killer();
        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = tx.send(child.wait());
        });
        match rx.recv_timeout(EXIT_TIMEOUT) {
            Ok(Ok(status)) => assert!(status.success(), "helper exited with {status:?}"),
            Ok(Err(error)) => panic!("helper wait failed: {error}"),
            Err(_) => {
                let _ = killer.kill();
                panic!("helper did not exit within {EXIT_TIMEOUT:?}");
            }
        }
    }

    #[allow(
        clippy::panic,
        reason = "bounded PTY marker failure needs explicit evidence"
    )]
    fn wait_for_raw(&mut self, needle: &str) {
        let deadline = Instant::now() + MARKER_TIMEOUT;
        while !String::from_utf8_lossy(&self.raw).contains(needle) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output_rx.recv_timeout(remaining) {
                Ok(chunk) => self.process(chunk),
                Err(error) => panic!("raw helper contract missing: {error:?}"),
            }
        }
    }

    fn process(&mut self, chunk: Vec<u8>) {
        self.parser.process(&chunk);
        self.raw.extend_from_slice(&chunk);
        let mut controls = std::mem::take(&mut self.control_tail);
        controls.extend_from_slice(&chunk);
        for window in controls.windows(SYNC_OUTPUT_START.len()) {
            if window == SYNC_OUTPUT_START {
                self.synchronized_output = true;
            } else if window == SYNC_OUTPUT_END {
                self.synchronized_output = false;
            }
        }
        self.control_tail = controls[controls.len().saturating_sub(7)..].to_vec();
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn reader_channel(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::sync_channel(64);
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

fn deterministic_env(command: &mut CommandBuilder) {
    for (key, value) in [
        ("HARNESS_DETERMINISTIC", "1"),
        ("HARNESS_DISABLE_ANIMATIONS", "1"),
        ("HARNESS_SEED", "42"),
        ("TERM", "xterm-256color"),
        ("LANG", "C.UTF-8"),
        ("LC_ALL", "C.UTF-8"),
        ("TZ", "UTC"),
    ] {
        command.env(key, value);
    }
}
