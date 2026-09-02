use serde::Serialize;
use vt100::Parser;

const SCROLLBACK_ROWS: usize = 1_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScreenState {
    rows: u16,
    cols: u16,
    cells: Vec<ScreenCell>,
    cursor: CursorState,
    alternate_screen: bool,
    modes: TerminalModes,
    wrapped_rows: Vec<u16>,
    scrollback_lines: usize,
    scrollback_text: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct ScreenCell {
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

pub(crate) struct RecordedTerminal {
    parser: Parser,
    query_tail: Vec<u8>,
    replies: Vec<u8>,
    raw: Vec<u8>,
}

impl RecordedTerminal {
    pub(crate) fn new(cols: u16, rows: u16) -> Self {
        Self {
            parser: Parser::new(rows, cols, SCROLLBACK_ROWS),
            query_tail: Vec::new(),
            replies: Vec::new(),
            raw: Vec::new(),
        }
    }

    pub(crate) fn process(&mut self, bytes: &[u8]) {
        self.raw.extend_from_slice(bytes);
        for byte in bytes {
            self.parser.process(std::slice::from_ref(byte));
            self.query_tail.push(*byte);
            self.collect_replies();
        }
    }

    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    pub(crate) fn drain_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    pub(crate) fn text(&self) -> String {
        self.parser.screen().contents()
    }

    pub(crate) fn raw(&self) -> &[u8] {
        &self.raw
    }

    pub(crate) fn state_size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    pub(crate) fn state(&self) -> ScreenState {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let cells = (0..rows)
            .flat_map(|row| {
                (0..cols).filter_map(move |column| {
                    let cell = screen.cell(row, column)?;
                    let text = cell.contents();
                    let width = if cell.is_wide() {
                        2
                    } else if cell.is_wide_continuation() {
                        0
                    } else {
                        1
                    };
                    (!text.is_empty() || width != 1).then_some(ScreenCell {
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
        ScreenState {
            rows,
            cols,
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
            wrapped_rows: (0..rows).filter(|row| screen.row_wrapped(*row)).collect(),
            scrollback_lines: history.scrollback(),
            scrollback_text: history.contents(),
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
            self.query_tail.drain(..self.query_tail.len() - 16);
        }
    }
}
