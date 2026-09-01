use harness_tui::UnwrapOrAbort;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::io::{Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const TIMEOUT: Duration = Duration::from_secs(8);
const SCROLLBACK_ROWS: usize = 1_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalState {
    rows: u16,
    cols: u16,
    cells: Vec<TerminalCell>,
    cursor: CursorState,
    alternate_screen: bool,
    modes: TerminalModes,
    wrapped_rows: Vec<u16>,
    scrollback_lines: usize,
    scrollback_text: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct TerminalCell {
    row: u16,
    column: u16,
    text: String,
    width: u8,
}

#[derive(Debug, Serialize)]
struct CursorState {
    row: u16,
    column: u16,
    visible: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalModes {
    application_cursor: bool,
    application_keypad: bool,
    bracketed_paste: bool,
    mouse_protocol: String,
}

pub(crate) struct ReplyTerminal {
    parser: Parser,
    query_tail: Vec<u8>,
    replies: Vec<u8>,
    rows: u16,
    cols: u16,
}

impl ReplyTerminal {
    pub(crate) fn new(cols: u16, rows: u16) -> Self {
        Self {
            parser: Parser::new(rows, cols, SCROLLBACK_ROWS),
            query_tail: Vec::new(),
            replies: Vec::new(),
            rows,
            cols,
        }
    }

    pub(crate) fn process(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.parser.process(std::slice::from_ref(byte));
            self.query_tail.push(*byte);
            self.collect_replies();
        }
    }

    pub(crate) fn drain_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub(crate) fn text(&self) -> String {
        self.parser.screen().contents()
    }

    pub(crate) fn state(&self) -> TerminalState {
        let screen = self.parser.screen();
        let cells = (0..self.rows)
            .flat_map(|row| {
                (0..self.cols).filter_map(move |column| {
                    let cell = screen.cell(row, column)?;
                    let text = cell.contents();
                    let width = if cell.is_wide() {
                        2
                    } else if cell.is_wide_continuation() {
                        0
                    } else {
                        1
                    };
                    (!text.is_empty() || width != 1).then_some(TerminalCell {
                        row,
                        column,
                        text: text.to_string(),
                        width,
                    })
                })
            })
            .collect();
        let (row, column) = screen.cursor_position();
        let mut history = screen.clone();
        history.set_scrollback(usize::MAX);
        let scrollback_lines = history.scrollback();
        let scrollback_text = history.contents();
        TerminalState {
            rows: self.rows,
            cols: self.cols,
            cells,
            cursor: CursorState {
                row,
                column,
                visible: !screen.hide_cursor(),
            },
            alternate_screen: screen.alternate_screen(),
            modes: TerminalModes {
                application_cursor: screen.application_cursor(),
                application_keypad: screen.application_keypad(),
                bracketed_paste: screen.bracketed_paste(),
                mouse_protocol: format!("{:?}", screen.mouse_protocol_mode()),
            },
            wrapped_rows: (0..self.rows)
                .filter(|row| screen.row_wrapped(*row))
                .collect(),
            scrollback_lines,
            scrollback_text,
            text: screen.contents(),
        }
    }

    fn collect_replies(&mut self) {
        let mut consumed = 0;
        while let Some(relative) = self.query_tail[consumed..]
            .windows(4)
            .position(|window| window == b"\x1b[6n")
        {
            let end = consumed + relative + 4;
            let (row, column) = self.parser.screen().cursor_position();
            self.replies
                .extend_from_slice(format!("\x1b[{};{}R", row + 1, column + 1).as_bytes());
            consumed = end;
        }
        if consumed > 0 {
            self.query_tail.drain(..consumed);
        }
        if self.query_tail.len() > 16 {
            let keep_from = self.query_tail.len() - 16;
            self.query_tail.drain(..keep_from);
        }
    }
}

#[allow(clippy::panic, reason = "bounded PTY failures need explicit evidence")]
pub(crate) fn native_pty_forwards_terminal_query_replies() {
    if !cfg!(target_os = "linux") {
        return;
    }

    // Given: a child in a native PTY that blocks until its cursor report is answered.
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap_or_abort();
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg(
        r#"stty raw -echo; printf '\033[2;4H\033[6n'; reply=$(dd bs=1 count=6 2>/dev/null | od -An -tx1 | tr -d ' \n'); printf '\r\nREPLY:%s\r\n' "$reply""#,
    );
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).unwrap_or_abort();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap_or_abort();
    let mut writer = pair.master.take_writer().unwrap_or_abort();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = [0_u8; 4096];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || tx.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut terminal = ReplyTerminal::new(80, 24);
    let deadline = Instant::now() + TIMEOUT;

    // When: every PTY output chunk is emulated and generated replies are forwarded.
    let observed = loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(bytes) => {
                terminal.process(&bytes);
                let replies = terminal.drain_replies();
                if !replies.is_empty() {
                    writer.write_all(&replies).unwrap_or_abort();
                    writer.flush().unwrap_or_abort();
                }
                let text = terminal.state().text;
                if text.contains("REPLY:") {
                    break text;
                }
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                panic!("native PTY child did not receive an emulator reply")
            }
        }
    };

    // Then: the child reports the exact cursor-position reply generated from emulator state.
    assert!(observed.contains("REPLY:1b5b323b3452"), "{observed:?}");
    let status = child.wait().unwrap_or_abort();
    assert!(status.success(), "query fixture exited with {status:?}");
}

pub(crate) fn emulator_replies_with_cursor_state_at_query_in_same_chunk() {
    // Given: one PTY read chunk queries the cursor before moving it elsewhere.
    let mut terminal = ReplyTerminal::new(80, 24);

    // When: the entire chunk is processed in one call.
    terminal.process(b"\x1b[2;4H\x1b[6n\x1b[3;7H");

    // Then: the reply captures the cursor position at the query byte sequence.
    assert_eq!(terminal.drain_replies(), b"\x1b[2;4R");
}

pub(crate) fn emulator_structures_terminal_state_and_scrollback() {
    // Given: control bytes exercising alternate screen, modes, wrapping, and scrollback.
    let mut terminal = ReplyTerminal::new(8, 3);
    terminal.process(b"one\r\ntwo\r\nthree\r\nfour\r\n");
    let scrolled = terminal.state();
    terminal.process(b"\x1b[?1049h\x1b[?25l\x1b[?1h\x1b[?2004h\x1b[?1000hABCDEFGH!");

    // When: structured state is collected from the emulator.
    let active = terminal.state();

    // Then: machine-consumed terminal state exposes every P0-06 assertion class.
    assert!(scrolled.scrollback_lines > 0, "{scrolled:?}");
    assert!(scrolled.scrollback_text.contains("one"), "{scrolled:?}");
    assert!(active.alternate_screen, "{active:?}");
    assert!(!active.cursor.visible, "{active:?}");
    assert!(active.modes.application_cursor, "{active:?}");
    assert!(active.modes.bracketed_paste, "{active:?}");
    assert_ne!(active.modes.mouse_protocol, "None", "{active:?}");
    assert!(!active.wrapped_rows.is_empty(), "{active:?}");
    assert!(
        active.cells.iter().any(|cell| cell.text == "A"),
        "{active:?}"
    );
    terminal.process(b"\x1b[?1049l");
    assert!(!terminal.state().alternate_screen);
}
