use super::p0_06_artifact_support::{
    assert_manifest_receipts, hash_file, receipt, source_tree, write_json,
};
use super::p0_06_terminal_emulator::ReplyTerminal;
use harness_tui::UnwrapOrAbort;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(12);
const SCENARIO_ENV: &str = "HARNESS_TUI_PTY_HELPER_SCENARIO";
const SCENARIO: &str = "type_first_startup";
const HELPER_TEST: &str = "pty_helper_type_first_startup";
const CAPTURE_MARKER: &str = "P0-06 canonical capture";
const SIZES: [Dimensions; 3] = [
    Dimensions::new(80, 24),
    Dimensions::new(120, 40),
    Dimensions::new(160, 50),
];

#[derive(Clone, Copy, Serialize)]
struct Dimensions {
    cols: u16,
    rows: u16,
}

impl Dimensions {
    const fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }
}

pub(crate) fn native_pty_writes_canonical_artifacts_and_provenance() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let Some(root) = std::env::var_os("HARNESS_P0_06_ARTIFACT_DIR").map(PathBuf::from) else {
        return;
    };
    assert!(
        root.is_absolute(),
        "HARNESS_P0_06_ARTIFACT_DIR must be absolute: {}",
        root.display()
    );
    fs::create_dir_all(&root).unwrap_or_abort();
    let binary = std::env::current_exe().unwrap_or_abort();
    let mut captures = Vec::new();

    for dimensions in SIZES {
        captures.push(capture_size(&root, &binary, dimensions));
    }
    write_json(
        root.join("cleanup.json"),
        &json!({ "result": "PASS", "sessions": SIZES.len(), "childrenExited": true, "ptysClosed": true }),
    );

    let manifest = json!({
        "schemaVersion": "harness-pty-emulator-artifacts-v1",
        "command": binary,
        "argv": ["--exact", HELPER_TEST, "--nocapture"],
        "executableKind": "harness-tui integration-test owner",
        "productionEntrypoint": "harness_tui::run_tui_with_options",
        "binarySha256": hash_file(&binary),
        "sourceTree": source_tree(),
        "terminal": {
            "emulator": "vt100 0.16 reply terminal",
            "term": "xterm-256color",
            "capabilities": ["cursor-position-report", "alternate-screen", "application-cursor", "bracketed-paste", "mouse-protocol", "wrap", "scrollback"],
        },
        "dimensions": SIZES,
        "captures": captures,
        "cleanupReceipt": receipt(&root, &root.join("cleanup.json")),
    });
    write_json(root.join("manifest.json"), &manifest);
    assert_eq!(captures.len(), SIZES.len());
    assert!(root.join("cleanup.json").is_file());
    assert_manifest_receipts(&root, &manifest);
}

fn capture_size(root: &Path, binary: &Path, dimensions: Dimensions) -> Value {
    let Dimensions { cols, rows } = dimensions;
    let directory = root.join(format!("{cols}x{rows}"));
    fs::create_dir_all(&directory).unwrap_or_abort();
    let mut session = Session::spawn(binary, cols, rows);
    session.wait_for("❯");
    let before = session.persist(root, &directory, "before");
    session.send(CAPTURE_MARKER.as_bytes());
    session.wait_for(CAPTURE_MARKER);
    let after = session.persist(root, &directory, "after");
    session.exit();
    let cleanup = json!({
        "dimensions": { "cols": cols, "rows": rows },
        "childExited": true,
        "ptyClosed": true,
    });
    write_json(directory.join("cleanup.json"), &cleanup);
    json!({
        "cols": cols,
        "rows": rows,
        "before": before,
        "after": after,
        "cleanup": receipt(root, &directory.join("cleanup.json")),
    })
}

struct Session {
    _master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    terminal: ReplyTerminal,
    raw: Vec<u8>,
}

impl Session {
    fn spawn(binary: &Path, cols: u16, rows: u16) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap_or_abort();
        let mut command = CommandBuilder::new(binary.to_string_lossy().as_ref());
        command.arg("--exact");
        command.arg(HELPER_TEST);
        command.arg("--nocapture");
        command.env(SCENARIO_ENV, SCENARIO);
        deterministic_env(&mut command);
        let child = pair.slave.spawn_command(command).unwrap_or_abort();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap_or_abort();
        let writer = pair.master.take_writer().unwrap_or_abort();
        let (tx, output) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || tx.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            _master: pair.master,
            child: Some(child),
            writer,
            output,
            terminal: ReplyTerminal::new(cols, rows),
            raw: Vec::new(),
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap_or_abort();
        self.writer.flush().unwrap_or_abort();
    }

    #[allow(clippy::panic, reason = "bounded PTY failures include emulator state")]
    fn wait_for(&mut self, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if self.terminal.text().contains(needle) {
                return;
            }
            match self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(bytes) => self.process(&bytes),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    panic!("timed out waiting for {needle:?}\n{}", self.terminal.text())
                }
            }
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        self.raw.extend_from_slice(bytes);
        self.terminal.process(bytes);
        let replies = self.terminal.drain_replies();
        if !replies.is_empty() {
            self.send(&replies);
        }
    }

    fn persist(&mut self, root: &Path, directory: &Path, name: &str) -> Value {
        while let Ok(bytes) = self.output.try_recv() {
            self.process(&bytes);
        }
        let ansi = directory.join(format!("{name}.ansi"));
        let text = directory.join(format!("{name}.txt"));
        let screen = directory.join(format!("{name}.screen.json"));
        fs::write(&ansi, &self.raw).unwrap_or_abort();
        fs::write(&text, format!("{}\n", self.terminal.text())).unwrap_or_abort();
        write_json(
            &screen,
            &serde_json::to_value(self.terminal.state()).unwrap_or_abort(),
        );
        json!({
            "ansi": receipt(root, &ansi),
            "text": receipt(root, &text),
            "screen": receipt(root, &screen),
        })
    }

    #[allow(
        clippy::panic,
        reason = "bounded PTY cleanup failures need explicit evidence"
    )]
    fn exit(&mut self) {
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
        match rx.recv_timeout(TIMEOUT) {
            Ok(Ok(status)) => assert!(status.success(), "helper exited with {status:?}"),
            Ok(Err(error)) => panic!("helper wait failed: {error}"),
            Err(_) => {
                let _ = killer.kill();
                panic!("helper did not exit within {TIMEOUT:?}");
            }
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
