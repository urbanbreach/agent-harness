#[path = "support/p0_02_pty.rs"]
mod scenario;

use harness_tui::UnwrapOrAbort;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::cmp;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const PRIMARY_COLS: u16 = 100;
const PRIMARY_ROWS: u16 = 30;
const REFLOW_COLS: u16 = 80;
const REFLOW_ROWS: u16 = 24;
const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn p0_02_pty_helper() {
    // arrange
    // Given: direct invocation opts into the shared deterministic scenario.
    // act
    // When: the real Harness TUI runs inside the caller's PTY.
    scenario::run_helper();
    // assert
    // Then: run_helper prints the external-driver contract after a clean exit.
}

#[test]
fn p0_02_real_pty_proves_dense_navigation_reflow_and_detached_append() {
    // arrange
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // Given: the helper has fourteen completed commands, three final responses,
    // and a fourth open response containing a failed command.
    let mut helper = Helper::spawn();
    helper.wait_for("P0-02 active streaming block remains open.");
    helper.wait_for("deterministic active failure");

    // act
    // When: the prompt surface detaches, then transcript focus navigates responses.
    helper.send(b"\x1b[5~");
    helper.wait_for("▼");
    helper.send(b"\t");
    helper.send(shifted('K').as_bytes());
    helper.wait_for("Harness 1/3");
    helper.wait_for("4 more");

    // Then: first and last response navigation clamp rather than wrap.
    helper.send(shifted('K').as_bytes());
    helper.send(shifted('J').as_bytes());
    helper.wait_for("Harness 2/3");
    helper.send(shifted('J').as_bytes());
    helper.wait_for("Harness 3/3");
    helper.send(shifted('J').as_bytes());
    helper.send(shifted('K').as_bytes());
    helper.wait_for("Harness 2/3");
    helper.send(shifted('K').as_bytes());
    helper.wait_for("Harness 1/3");

    // assert
    // And: keyboard and SGR-mouse activation disclose the same dense fold.
    helper.send(b"\r");
    helper.wait_until_absent("4 more");
    helper.send(b"\r");
    helper.wait_for("4 more");
    helper.click("Ran 14 commands");
    helper.wait_until_absent("4 more");
    helper.click("Ran 14 commands");
    helper.wait_for("4 more");

    // And: a real PTY resize reflows while preserving response and fold identity.
    helper.resize(REFLOW_COLS, REFLOW_ROWS);
    helper.wait_for("Harness 1/3");
    helper.wait_for("4 more");
    let reflowed = helper.screen();
    assert_eq!(helper.parser.screen().size(), (REFLOW_ROWS, REFLOW_COLS));
    assert!(reflowed
        .lines()
        .all(|line| line.chars().count() <= usize::from(REFLOW_COLS)));
    assert!(
        reflowed.contains('▼'),
        "resize must preserve detachment\n{reflowed}"
    );

    // And: the exact cancel intent triggers a live append after subscription;
    // its status is visible, while appended transcript content stays below view.
    helper.send(b"\x03");
    helper.wait_for_raw(scenario::APPEND_STATUS);
    let detached = helper.screen();
    assert!(
        detached.contains('▼'),
        "live append must stay detached\n{detached}"
    );
    assert!(
        !detached.contains(scenario::APPENDED_STREAM_TEXT),
        "streaming append must remain outside the detached viewport\n{detached}"
    );
    helper.send(b"\x1b[F");
    helper.wait_for(scenario::APPENDED_STREAM_TEXT);

    // And: the helper exits cleanly and documents its direct xterm.js command.
    helper.exit_via_palette();
    helper.wait_for_raw(scenario::HELPER_CONTRACT);
    println!("{}", scenario::HELPER_CONTRACT);
}

#[test]
fn p0_02_dropping_live_helper_reaps_child_process_group() {
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // arrange: a live helper child with its own PTY session/process group.
    let helper = Helper::spawn();
    let process_id = helper
        .child
        .as_ref()
        .and_then(|child| child.process_id())
        .unwrap_or_abort();
    let process_id_arg = process_id.to_string();
    assert!(
        std::process::Command::new("kill")
            .args(["-0", &process_id_arg])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap_or_abort()
            .success(),
        "helper child must be live before drop"
    );

    // act: drop the live helper without using its normal exit path.
    drop(helper);

    // assert: Drop killed and reaped both the child and its PTY process group.
    let process_group_id_arg = format!("-{process_id}");
    assert!(
        !std::process::Command::new("kill")
            .args(["-0", "--", &process_group_id_arg])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap_or_abort()
            .success(),
        "helper process group must be gone after drop"
    );
}

fn shifted(character: char) -> String {
    format!("\x1b[{};2u", u32::from(character))
}

struct Helper {
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
    raw: Vec<u8>,
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
        command.arg("p0_02_pty_helper");
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
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap_or_abort();
        self.writer.flush().unwrap_or_abort();
    }

    fn wait_for(&mut self, needle: &str) {
        self.wait_for_screen(needle, true);
    }

    fn wait_until_absent(&mut self, needle: &str) {
        self.wait_for_screen(needle, false);
    }

    #[allow(
        clippy::panic,
        reason = "bounded PTY test failure needs screen evidence"
    )]
    fn wait_for_screen(&mut self, needle: &str, present: bool) {
        let deadline = Instant::now() + MARKER_TIMEOUT;
        loop {
            let screen = self.screen();
            if screen.contains(needle) == present {
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

    fn resize(&mut self, cols: u16, rows: u16) {
        self.master.resize(size(cols, rows)).unwrap_or_abort();
        self.parser = Parser::new(rows, cols, 0);
    }

    fn click(&mut self, needle: &str) {
        let screen = self.screen();
        let (row, column) = screen
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                line.find(needle).map(|byte| {
                    (
                        row.saturating_add(1),
                        line[..byte].chars().count().saturating_add(1),
                    )
                })
            })
            .unwrap_or_abort();
        self.send(format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").as_bytes());
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
