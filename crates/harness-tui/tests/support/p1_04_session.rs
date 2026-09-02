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

#[derive(Clone, Copy)]
pub(crate) enum TerminalVariant {
    Unicode,
    BasicAscii,
}

#[derive(Clone, Copy)]
pub(crate) struct CaptureTarget {
    pub(crate) variant: TerminalVariant,
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
}

impl Session {
    pub(crate) fn spawn(binary: &Path, target: CaptureTarget) -> Self {
        let CaptureTarget {
            variant,
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
            ("HARNESS_DISABLE_ANIMATIONS", "1"),
            ("HARNESS_SEED", "42"),
            ("LANG", "C.UTF-8"),
            ("LC_ALL", "C.UTF-8"),
            ("TZ", "UTC"),
        ] {
            command.env(key, value);
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
        }
    }

    pub(crate) fn wait_for(&mut self, needle: &str) {
        self.wait_until(|terminal| terminal.text().contains(needle), needle);
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

    pub(crate) fn resize_burst(&mut self, final_cols: u16, final_rows: u16) -> Value {
        let sequence = [
            (final_cols.saturating_add(9), final_rows.saturating_add(3)),
            (
                final_cols.saturating_sub(7).max(40),
                final_rows.saturating_sub(2).max(12),
            ),
            (final_cols, final_rows),
        ];
        let output_before_prime = self.raw().len();
        let (prime_cols, prime_rows) = sequence[0];
        self.master
            .resize(size(prime_cols, prime_rows))
            .unwrap_or_abort();
        self.terminal.resize(prime_cols, prime_rows);
        self.wait_until(
            |terminal| terminal.raw().len() > output_before_prime,
            "priming resize frame",
        );

        let output_before_burst = self.raw().len();
        for (cols, rows) in sequence[1..].iter().copied() {
            self.master.resize(size(cols, rows)).unwrap_or_abort();
            self.terminal.resize(cols, rows);
        }
        self.wait_until(
            |terminal| terminal.raw().len() > output_before_burst,
            "debounced resize frame",
        );
        self.send(b"\x1b[6~");
        self.wait_for(scenario::READY_MARKER);
        assert_eq!(self.terminal.state_size(), (final_rows, final_cols));
        json!({
            "primingResize": { "cols": prime_cols, "rows": prime_rows },
            "events": sequence[1..].iter().map(|(cols, rows)| json!({ "cols": cols, "rows": rows })).collect::<Vec<_>>(),
            "final": { "cols": final_cols, "rows": final_rows },
            "inputAfterDebouncedResize": true,
            "parserStatePreserved": true,
            "masterPtyResize": true,
        })
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
        let replies = self.terminal.drain_replies();
        if !replies.is_empty() {
            self.send(&replies);
        }
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
