#[path = "support/p0_04_pty.rs"]
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

#[test]
fn p0_04_pty_helper() {
    // arrange
    // Given: direct invocation opts into the shared deterministic scenario.
    // act
    // When: the real Harness TUI runs inside the caller's PTY.
    scenario::run_helper();
    // assert
    // Then: run_helper prints the external-driver contract after a clean exit.
}

#[test]
fn p0_04_real_pty_records_multiline_composer_shortcuts() {
    // arrange
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // Given: the helper has a deterministic active streaming turn.
    let mut helper = Helper::spawn();
    helper.wait_for(scenario::READY_MARKER);

    // When: the command palette enables multiline input.
    helper.send(b"\x10");
    helper.wait_for("Commands");
    helper.send(b"multiline");
    helper.wait_for("Multiline Input");
    helper.send(b"\r");
    helper.wait_for("MULTILINE");

    // Then: plain Enter inserts a newline and emits no submission.
    helper.send(b"first");
    helper.wait_for("first");
    helper.send(b"\r");
    helper.send(b"second");
    helper.wait_for("second");
    let draft = helper.screen();
    assert!(
        draft.contains("first") && draft.contains("second"),
        "multiline draft missing\n{draft}"
    );
    assert!(
        !helper.raw_contains(scenario::SUBMITTED_MARKER),
        "plain Enter submitted before Shift+Enter\n{draft}"
    );

    // When: Shift+Enter submits the multiline draft while the active turn streams.
    helper.send(modified_enter(2).as_bytes());
    helper.wait_for_raw(scenario::QUEUED_MARKER);
    let queued = helper.screen();
    assert!(
        queued.contains("first") && queued.contains("second"),
        "queued draft missing\n{queued}"
    );

    // When: Ctrl+Alt+Enter interjects a draft, then Ctrl+Shift+Enter replaces the active turn.
    helper.send(b"interject draft");
    helper.wait_for("interject draft");
    helper.send(modified_enter(7).as_bytes());
    helper.wait_for_raw(scenario::INTERJECT_MARKER);
    helper.send(b"replacement draft");
    helper.wait_for("replacement draft");
    helper.send(modified_enter(6).as_bytes());
    helper.wait_for_raw(scenario::REPLACE_INTERRUPT_MARKER);
    helper.wait_for_raw(scenario::REPLACE_MARKER);
    let replaced = helper.screen();
    assert_eq!(
        replaced.matches("interject draft").count(),
        1,
        "interject draft duplicated\n{replaced}"
    );
    assert_eq!(
        replaced.matches("replacement draft").count(),
        1,
        "replacement draft duplicated\n{replaced}"
    );

    // When: the empty replacement shortcut is pressed.
    helper.send(modified_enter(6).as_bytes());

    // Then: processing a later palette action proves the empty shortcut produced no activity.
    helper.send(b"\x10");
    helper.wait_for("Commands");
    assert!(!helper.raw_contains(scenario::EMPTY_MARKER));
    assert!(!helper.screen().contains(scenario::PHANTOM_MARKER));

    // act (invalid-input path)
    // When: malformed and unbound escape sequences arrive. Each is drained
    // separately so consecutive ESC bytes never pair into the legitimate
    // Esc-Esc draft-clear gesture, and none decode to bound keys: 99~ is
    // outside the vt220 tilde table, the private `?` forms and `z` final are
    // unsupported, and the u16 parameter saturates instead of mapping a key.
    let submissions_before = helper.raw_occurrences(scenario::SUBMITTED_MARKER);
    for garbage in [
        "\x1b[99~",
        "\x1b[?999999999z",
        "\x1b[99999999999999999999~",
        "\x1b[13;18446744073709551616u",
    ] {
        helper.send(garbage.as_bytes());
        helper.drain_ms(200);
    }
    helper.drain_ms(300);

    // Then: no new submission markers fire and the input decoder stays coherent.
    assert_eq!(
        helper.raw_occurrences(scenario::SUBMITTED_MARKER),
        submissions_before,
        "invalid Enter sequences must not submit the draft"
    );

    // And: the palette still opens afterwards, proving the input decoder stayed coherent.
    helper.send(b"\x10");
    helper.wait_for("Commands");
    helper.send(b"\x1b");
    helper.wait_for_screen_absent("Commands");

    // And: any overlay opened by the malformed sequence is dismissed before
    // the final branding assertion checks the base composer surface.
    helper.send(b"\x1b");
    helper.drain_ms(300);
    helper.send(b"\x1b");
    helper.drain_ms(300);

    // And: branding and bordered shell geometry remain visible.
    let final_screen = helper.screen();
    assert!(
        final_screen.contains("MULTILINE"),
        "multiline branding missing\n{final_screen}"
    );
    assert!(
        (final_screen.contains('┌') || final_screen.contains('╭'))
            && (final_screen.contains('└') || final_screen.contains('╰')),
        "composer borders missing\n{final_screen}"
    );

    helper.exit_via_palette();
    println!("{}", scenario::HELPER_CONTRACT);
}

fn modified_enter(modifiers: u8) -> String {
    format!("\x1b[13;{modifiers}u")
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
            .openpty(PtySize {
                rows: PRIMARY_ROWS,
                cols: PRIMARY_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap_or_abort();
        let current_test_bin = std::env::current_exe().unwrap_or_abort();
        let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
        command.arg("--exact");
        command.arg("p0_04_pty_helper");
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

    fn wait_for_screen_absent(&mut self, needle: &str) {
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

    fn drain_ms(&mut self, ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            match self.output_rx.recv_timeout(remaining) {
                Ok(chunk) => self.process(chunk),
                Err(_) => return,
            }
        }
    }

    fn raw_contains(&self, needle: &str) -> bool {
        String::from_utf8_lossy(&self.raw).contains(needle)
    }

    fn raw_occurrences(&self, needle: &str) -> usize {
        String::from_utf8_lossy(&self.raw).matches(needle).count()
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
                Err(error) => panic!("raw marker {needle:?} missing: {error:?}"),
            }
        }
    }

    fn process(&mut self, chunk: Vec<u8>) {
        self.parser.process(&chunk);
        self.raw.extend_from_slice(&chunk);
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
