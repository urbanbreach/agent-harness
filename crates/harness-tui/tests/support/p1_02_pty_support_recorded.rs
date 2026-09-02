use crate::scenario;
use harness_tui::UnwrapOrAbort;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const SETTINGS_FOOTER: &str = "↑/↓ navigate · Enter edit · Esc close";

pub(crate) fn run_modal_journey(cols: u16, rows: u16) {
    let mut helper = Helper::spawn(cols, rows);
    helper.wait_for(scenario::READY_MARKER);

    // When: Commands dispatches Settings through the shipped palette path.
    helper.open_settings(cols, rows);
    assert_modal(&helper.screen(), cols, rows);

    // Then: tab focus moves in both directions while shared chrome remains aligned.
    helper.send(b"\t");
    helper.wait_for("Runtime  [TUI]");
    helper.wait_for_modal(cols, rows);
    helper.send(b"\x1b[Z");
    helper.wait_for("[Runtime]  TUI");
    helper.wait_for_modal(cols, rows);
    assert_modal(&helper.screen(), cols, rows);

    // When: Escape closes the child modal.
    helper.send(b"\x1b");
    helper.wait_for("Commands");
    helper.wait_until_absent("Commands / Settings");

    // Then: a stale outside press paired with an inside release cannot dismiss Settings.
    helper.send(b"\r");
    helper.wait_for("Commands / Settings");
    helper.wait_for_modal(cols, rows);
    let popup = popup(cols, rows);
    helper.mouse_down(1, 1);
    helper.mouse_up(popup.0 + 3, popup.1 + 4);
    helper.wait_for("Commands / Settings");
    helper.wait_for_modal(cols, rows);
    assert_modal(&helper.screen(), cols, rows);

    // When: a click lands inside the retained six-cell top-right close target.
    helper.click(popup.0 + popup.2 - 3, popup.1 + 1);

    // Then: the exact Commands parent surface is restored.
    helper.wait_for("Commands");
    helper.wait_until_absent("Commands / Settings");
    helper.exit();
}

fn assert_modal(screen: &str, cols: u16, rows: u16) {
    let (x, y, width, height) = popup(cols, rows);
    let lines: Vec<&str> = screen.lines().collect();
    assert!(
        modal_is_complete(screen, cols, rows),
        "{cols}x{rows}\n{screen}"
    );
    assert_eq!(cell(&lines, y, x), Some('┌'), "{cols}x{rows}\n{screen}");
    assert_eq!(
        cell(&lines, y, x + width - 1),
        Some('┐'),
        "{cols}x{rows}\n{screen}"
    );
    assert_eq!(
        cell(&lines, y + height - 1, x),
        Some('└'),
        "{cols}x{rows}\n{screen}"
    );
    assert_eq!(
        cell(&lines, y + height - 1, x + width - 1),
        Some('┘'),
        "{cols}x{rows}\n{screen}"
    );
}

fn modal_is_complete(screen: &str, cols: u16, rows: u16) -> bool {
    let (x, y, width, height) = popup(cols, rows);
    let lines: Vec<&str> = screen.lines().collect();
    let right = x + width - 1;
    let bottom = y + height - 1;
    let title = line_slice(&lines, y, x, width);
    let breadcrumb = line_slice(&lines, y + 1, x + 2, width.saturating_sub(4));
    let tabs = line_slice(&lines, y + 2, x + 2, width.saturating_sub(4));
    let footer = line_slice(&lines, bottom - 1, x + 1, width.saturating_sub(2));
    title.starts_with("┌─ Settings")
        && breadcrumb.starts_with("Commands / Settings")
        && (tabs.starts_with("[Runtime]  TUI") || tabs.starts_with("Runtime  [TUI]"))
        && footer.contains(SETTINGS_FOOTER)
        && cell(&lines, y, right) == Some('┐')
        && (y + 1..bottom).all(|row| cell(&lines, row, right) == Some('│'))
        && cell(&lines, bottom, x) == Some('└')
        && cell(&lines, bottom, right) == Some('┘')
}

fn line_slice(lines: &[&str], row: u16, column: u16, width: u16) -> String {
    lines
        .get(usize::from(row))
        .copied()
        .unwrap_or("")
        .chars()
        .skip(usize::from(column))
        .take(usize::from(width))
        .collect()
}

fn cell(lines: &[&str], row: u16, column: u16) -> Option<char> {
    lines
        .get(usize::from(row))
        .and_then(|line| line.chars().nth(usize::from(column)))
}

const fn popup(cols: u16, rows: u16) -> (u16, u16, u16, u16) {
    let width = if cols < 88 { cols } else { 88 };
    let height = if rows < 28 { rows } else { 28 };
    ((cols - width) / 2, (rows - height) / 2, width, height)
}

struct Helper {
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
    query_tail: Vec<u8>,
}

impl Drop for Helper {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Helper {
    fn spawn(cols: u16, rows: u16) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap_or_abort();
        let current_test_bin = std::env::current_exe().unwrap_or_abort();
        let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
        command.args(["--exact", "p1_02_pty_helper", "--nocapture"]);
        command.env(scenario::SCENARIO_ENV, "1");
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
        let child = pair.slave.spawn_command(command).unwrap_or_abort();
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().unwrap_or_abort();
        let writer = pair.master.take_writer().unwrap_or_abort();
        Self {
            master: pair.master,
            child: Some(child),
            writer,
            output_rx: reader_channel(reader),
            parser: Parser::new(rows, cols, 0),
            query_tail: Vec::new(),
        }
    }

    fn open_settings(&mut self, cols: u16, rows: u16) {
        self.send(b"\x10");
        self.wait_for("Commands");
        self.send(b"settings");
        self.wait_for("Settings");
        self.send(b"\r");
        self.wait_for("Commands / Settings");
        self.wait_for_modal(cols, rows);
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap_or_abort();
        self.writer.flush().unwrap_or_abort();
    }

    fn mouse_down(&mut self, column: u16, row: u16) {
        self.send(format!("\x1b[<0;{column};{row}M").as_bytes());
    }

    fn mouse_up(&mut self, column: u16, row: u16) {
        self.send(format!("\x1b[<0;{column};{row}m").as_bytes());
    }

    fn click(&mut self, column: u16, row: u16) {
        self.mouse_down(column, row);
        self.mouse_up(column, row);
    }

    fn wait_for(&mut self, needle: &str) {
        self.wait_for_screen(needle, true);
    }

    fn wait_until_absent(&mut self, needle: &str) {
        self.wait_for_screen(needle, false);
    }

    #[allow(
        clippy::panic,
        reason = "bounded PTY failure includes terminal evidence"
    )]
    fn wait_for_screen(&mut self, needle: &str, present: bool) {
        let deadline = Instant::now() + MARKER_TIMEOUT;
        loop {
            if self.screen().contains(needle) == present {
                return;
            }
            match self
                .output_rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(chunk) => self.process(&chunk),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    panic!(
                        "timed out waiting for {needle:?} present={present}\n{}",
                        self.screen()
                    )
                }
            }
        }
    }

    #[allow(
        clippy::panic,
        reason = "bounded PTY failure includes the incomplete modal frame"
    )]
    fn wait_for_modal(&mut self, cols: u16, rows: u16) {
        let deadline = Instant::now() + MARKER_TIMEOUT;
        loop {
            let screen = self.screen();
            if modal_is_complete(&screen, cols, rows) {
                return;
            }
            match self
                .output_rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(chunk) => self.process(&chunk),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    panic!("timed out waiting for complete {cols}x{rows} Settings frame\n{screen}")
                }
            }
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.parser.process(std::slice::from_ref(byte));
            self.query_tail.push(*byte);
            if self.query_tail.ends_with(b"\x1b[6n") {
                let (row, column) = self.parser.screen().cursor_position();
                self.send(format!("\x1b[{};{}R", row + 1, column + 1).as_bytes());
                self.query_tail.clear();
            } else if self.query_tail.len() > 16 {
                self.query_tail.remove(0);
            }
        }
    }

    fn screen(&self) -> String {
        self.parser.screen().contents()
    }

    #[allow(clippy::panic, reason = "bounded PTY child failure must fail closed")]
    fn exit(&mut self) {
        self.send(b"\x1b");
        self.wait_until_absent("Commands");
        self.send(b"\x10");
        self.wait_for("Commands");
        self.send(b"\x7f\x7f\x7f\x7f\x7f\x7f\x7f\x7f");
        self.wait_until_absent("search: settings");
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
