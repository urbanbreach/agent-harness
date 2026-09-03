#[path = "support/p0_01_pty.rs"]
mod scenario;

use harness_tui::UnwrapOrAbort;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const PRIMARY_COLS: u16 = 120;
const PRIMARY_ROWS: u16 = 40;
const COMPACT_COLS: u16 = 80;
const COMPACT_ROWS: u16 = 24;
const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn p0_01_pty_helper() {
    // arrange
    // Given: direct invocation opts into the deterministic dashboard scenario.
    // act
    // When: the real Harness TUI runs inside the caller's PTY.
    scenario::run_helper();
    // assert
    // Then: run_helper prints the external-driver contract after a clean exit.
}

#[test]
fn p0_01_dashboard_round_trip_restores_detached_anchor_display_column_and_focus() {
    // arrange
    if !cfg!(target_os = "linux") || std::env::var("HARNESS_TUI_PTY_SIGNOFF").as_deref() != Ok("1")
    {
        return;
    }

    // Given: a long transcript rendered live with its tail visible.
    let mut helper = Helper::spawn();
    helper.wait_for(scenario::TAIL_MARKER);

    // act
    // When: the user detaches by scrolling up to the middle row.
    send_page_ups_until_visible(&mut helper, scenario::MIDDLE_MARKER);
    let detached_screen = helper.screen();
    assert!(
        detached_screen.contains(scenario::MIDDLE_MARKER)
            && !detached_screen.contains(scenario::TAIL_MARKER),
        "detached viewport must show the middle row instead of the tail\n{detached_screen}"
    );

    // When: the dashboard opens as a full surface and the terminal resizes beneath it.
    send_key(&mut helper, 0x18);
    send_key(&mut helper, b's');
    helper.wait_for("Status · Harness dashboard");
    helper.resize(COMPACT_COLS, COMPACT_ROWS);
    helper.wait_for("Status · Harness dashboard");
    let dashboard_screen = helper.screen();
    assert!(
        !dashboard_screen.contains(scenario::MIDDLE_MARKER),
        "dashboard must own the full surface while open\n{dashboard_screen}"
    );

    // When: the dashboard closes after the resize.
    send_bytes(&mut helper, b"\x1b");
    helper.wait_until_absent("Status · Harness dashboard");

    // Resize back before asserting the anchor: the round trip must return the
    // same content anchor and display column even though the terminal was
    // resized while the dashboard owned the surface.
    helper.resize(PRIMARY_COLS, PRIMARY_ROWS);
    helper.drain_ms(400);
    let restored_screen = helper.screen();

    // assert
    // Then: the detached anchor returns to the same middle transcript row.
    assert!(
        restored_screen.contains(scenario::MIDDLE_MARKER),
        "closing the dashboard must restore the detached transcript anchor\n{restored_screen}"
    );
    // And: the viewport stays detached rather than snapping to the live tail.
    assert!(
        !restored_screen.contains(scenario::TAIL_MARKER),
        "restored viewport must remain detached after the dashboard round trip\n{restored_screen}"
    );
    // And: composer focus returns so typing still targets the prompt.
    helper.send(b"x");
    helper.wait_for("x");
    let focused_screen = helper.screen();
    assert!(
        focused_screen.contains('x'),
        "typing after restoration must reach the composer\n{focused_screen}"
    );

    helper.exit_via_palette();
    helper.wait_for_raw(scenario::HELPER_CONTRACT);
}

#[allow(
    clippy::panic,
    reason = "bounded PTY scrolling needs an iteration cap with screen evidence"
)]
fn send_page_ups_until_visible(helper: &mut Helper, marker: &str) {
    for _ in 0..30 {
        let screen = helper.screen();
        if screen.contains(marker) {
            return;
        }
        send_bytes(helper, b"\x1b[5~");
        // The sidebar also paints ▼ glyphs, so detachment is proven by the
        // live tail leaving the viewport instead of a glyph match; each
        // subsequent PageUp needs a render drain before the screen check.
        helper.wait_until_absent(scenario::TAIL_MARKER);
        helper.drain_ms(150);
    }
    panic!(
        "middle marker never reached the viewport:\n{}",
        helper.screen()
    );
}

fn send_key(helper: &mut Helper, key: u8) {
    send_bytes(helper, &[key]);
}

fn send_bytes(helper: &mut Helper, bytes: &[u8]) {
    helper.send(bytes);
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
        command.arg("p0_01_pty_helper");
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

    fn resize(&mut self, cols: u16, rows: u16) {
        self.master.resize(size(cols, rows)).unwrap_or_abort();
        self.parser = Parser::new(rows, cols, 0);
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
