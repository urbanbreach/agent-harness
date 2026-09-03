use crate::scenario;
use crate::support::terminal::RecordedTerminal;
use harness_tui::UnwrapOrAbort;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

const SCREEN_TIMEOUT: Duration = Duration::from_secs(12);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Screen-sample stride for reveal ordering replay. Reveal stages separate
/// by thousands of bytes; a 256-byte stride resolves their order while
/// keeping one replay pass cheap enough for the signoff lane budget.
const SCAN_STRIDE: usize = 256;

#[derive(Clone, Copy)]
pub(crate) enum TerminalVariant {
    Unicode,
    BasicAscii,
}

#[derive(Clone, Copy)]
pub(crate) struct CaptureTarget {
    pub(crate) variant: TerminalVariant,
    pub(crate) reduced_motion: bool,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

impl TerminalVariant {
    pub(crate) const fn directory(self) -> &'static str {
        match self {
            Self::Unicode => "unicode",
            Self::BasicAscii => "basic-ascii",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Unicode => "Unicode",
            Self::BasicAscii => "Basic/Ascii",
        }
    }
}

pub(crate) struct Session {
    master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    terminal: RecordedTerminal,
    cols: u16,
    rows: u16,
}

impl Session {
    pub(crate) fn spawn(binary: &Path, target: CaptureTarget) -> Self {
        let CaptureTarget {
            variant,
            reduced_motion,
            cols,
            rows,
        } = target;
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .unwrap_or_abort();
        let mut command = CommandBuilder::new(binary.to_string_lossy().as_ref());
        command.args(["--exact", scenario::HELPER_TEST, "--nocapture"]);
        command.env(scenario::SCENARIO_ENV, "1");
        for (key, value) in [
            ("HARNESS_DETERMINISTIC", "1"),
            ("HARNESS_SEED", "42"),
            ("LANG", "C.UTF-8"),
            ("LC_ALL", "C.UTF-8"),
            ("TZ", "UTC"),
        ] {
            command.env(key, value);
        }
        if reduced_motion {
            command.env("HARNESS_DISABLE_ANIMATIONS", "1");
        } else {
            command.env_remove("HARNESS_DISABLE_ANIMATIONS");
            command.env_remove("HARNESS_TUI_REDUCED_MOTION");
        }
        match variant {
            TerminalVariant::Unicode => {
                command.env("TERM", "xterm-256color");
                command.env("TERM_PROGRAM", "WezTerm");
                command.env("COLORTERM", "truecolor");
                command.env_remove("NO_COLOR");
            }
            TerminalVariant::BasicAscii => {
                command.env("TERM", "dumb");
                command.env_remove("TERM_PROGRAM");
                command.env_remove("COLORTERM");
                command.env_remove("NO_COLOR");
            }
        }
        let child = pair.slave.spawn_command(command).unwrap_or_abort();
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().unwrap_or_abort();
        let writer = pair.master.take_writer().unwrap_or_abort();
        Self {
            master: pair.master,
            child: Some(child),
            writer,
            output: reader_channel(reader),
            terminal: RecordedTerminal::new(cols, rows),
            cols,
            rows,
        }
    }

    pub(crate) fn wait_for(&mut self, needle: &str) {
        self.wait_until(|terminal| terminal.text().contains(needle), needle);
    }

    pub(crate) fn wait_for_alternate_screen(&mut self) {
        self.wait_until(RecordedTerminal::alternate_screen, "alternate screen");
    }

    pub(crate) fn wait_until_absent(&mut self, needle: &str) {
        self.wait_until(
            |terminal| !terminal.text().contains(needle),
            &format!("absence of {needle}"),
        );
    }

    pub(crate) fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap_or_abort();
        self.writer.flush().unwrap_or_abort();
    }

    /// Waits until every required marker has been painted. Polls one
    /// composite predicate so each poll materializes the screen text once.
    pub(crate) fn wait_for_all_markers(&mut self, required: &[&str]) {
        self.wait_until(
            |terminal| {
                let text = terminal.text();
                required.iter().all(|marker| text.contains(*marker))
            },
            &format!("all of {required:?}"),
        );
    }

    /// Finds the raw-stream offset at which each marker first becomes
    /// visible, by replaying the fully recorded stream once and sampling the
    /// assembled screen every `SCAN_STRIDE` bytes. The pre-input reveal only
    /// paints, never erases, so first-seen sample offsets preserve true
    /// marker order as long as the stride is far below the reveal's stage
    /// separation. The result is a pure function of the recorded stream:
    /// unlike live screen sampling, PTY read coalescing cannot skip a
    /// transient stage, so ordering evidence derived from these offsets is
    /// race-free.
    pub(crate) fn marker_byte_offsets<'a>(
        &self,
        required: &[&'a str],
        optional: &[&'a str],
    ) -> (Vec<(&'a str, usize)>, Vec<(&'a str, usize)>) {
        let raw = self.terminal.raw();
        let mut replay = RecordedTerminal::new(self.cols, self.rows);
        let mut required_seen: Vec<(&'a str, usize)> = Vec::new();
        let mut optional_seen: Vec<(&'a str, usize)> = Vec::new();
        let mut pending = required.len() + optional.len();
        let mut scanned = 0usize;
        while scanned < raw.len() && pending > 0 {
            let chunk_end = (scanned + SCAN_STRIDE).min(raw.len());
            replay.process_replay_chunk(&raw[scanned..chunk_end]);
            scanned = chunk_end;
            let text = replay.text();
            for marker in required {
                if text.contains(marker) && !required_seen.iter().any(|(seen, _)| *seen == *marker)
                {
                    required_seen.push((marker, scanned));
                    pending -= 1;
                }
            }
            for marker in optional {
                if text.contains(marker) && !optional_seen.iter().any(|(seen, _)| *seen == *marker)
                {
                    optional_seen.push((marker, scanned));
                    pending -= 1;
                }
            }
        }
        (required_seen, optional_seen)
    }

    pub(crate) fn persist(&mut self, root: &Path, directory: &Path, name: &str) -> Value {
        while let Ok(bytes) = self.output.try_recv() {
            self.process(&bytes);
        }
        let ansi = directory.join(format!("{name}.ansi"));
        let text = directory.join(format!("{name}.txt"));
        let screen = directory.join(format!("{name}.screen.json"));
        fs::write(&ansi, self.terminal.raw()).unwrap_or_abort();
        fs::write(&text, format!("{}\n", self.terminal.text())).unwrap_or_abort();
        super::artifacts::write_json(
            &screen,
            &serde_json::to_value(self.terminal.state()).unwrap_or_abort(),
        );
        json!({
            "ansi": super::artifacts::receipt(root, &ansi),
            "text": super::artifacts::receipt(root, &text),
            "screen": super::artifacts::receipt(root, &screen),
        })
    }

    pub(crate) fn text(&self) -> String {
        self.terminal.text()
    }

    pub(crate) fn raw(&self) -> &[u8] {
        self.terminal.raw()
    }

    #[expect(
        clippy::panic,
        reason = "bounded PTY shutdown failures must retain exact wait evidence"
    )]
    pub(crate) fn exit(&mut self) {
        self.send(&[0x11]);
        self.send(&[0x11]);
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
                let _ = rx.recv_timeout(EXIT_TIMEOUT);
                panic!("helper did not exit within {EXIT_TIMEOUT:?}");
            }
        }
    }

    #[expect(clippy::panic, reason = "bounded PTY failure includes screen evidence")]
    fn wait_until(&mut self, condition: impl Fn(&RecordedTerminal) -> bool, description: &str) {
        let deadline = Instant::now() + SCREEN_TIMEOUT;
        loop {
            if condition(&self.terminal) {
                return;
            }
            match self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(bytes) => self.process(&bytes),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    panic!(
                        "timed out waiting for {description}\n{}",
                        self.terminal.text()
                    )
                }
            }
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        self.terminal.process(bytes);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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

fn reader_channel(mut reader: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 8_192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if tx.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}
