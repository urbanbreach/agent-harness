//! Raw ANSI/xterm byte-stream decoder.
//!
//! Owns escape framing (control bytes, `ESC O` SS3, `CSI … final`, SGR/legacy
//! mouse, focus, and bracketed paste) and delegates key semantics to
//! [`super::key`] and mouse payloads to [`crate::mouse`]. The decoder is
//! streaming: an incomplete trailing sequence is retained until the next
//! [`Decoder::feed`] or flushed at end of input via [`Decoder::flush`].

use crate::mouse::{decode_legacy, decode_sgr};

use super::event::{FocusEvent, KeyCode, KeyEvent, KeyModifiers, ResizeEvent, TerminalInputEvent};
use super::key;

const ESC: u8 = 0x1B;
const PASTE_END: [u8; 6] = [ESC, b'[', b'2', b'0', b'1', b'~'];

/// Streaming decoder for a terminal input byte stream.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    paste: Option<String>,
}

#[derive(Debug)]
enum Step {
    Emit(TerminalInputEvent),
    Skip,
    Await,
    End,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed raw bytes and return every event that is now fully decodable.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<TerminalInputEvent> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut i = 0usize;
        loop {
            match self.step(&mut i) {
                Step::Emit(ev) => out.push(ev),
                Step::Skip => {}
                Step::Await | Step::End => break,
            }
        }
        self.buf.drain(..i);
        out
    }

    /// Convert a terminal resize report into an event (resize arrives out of
    /// band, e.g. from SIGWINCH, not the byte stream).
    pub fn resize(cols: u16, rows: u16) -> TerminalInputEvent {
        TerminalInputEvent::Resize(ResizeEvent::new(cols, rows))
    }

    /// Flush any buffered bytes at end of input. A lone `ESC` becomes a bare
    /// Escape key; anything else is surfaced verbatim as [`TerminalInputEvent::Unknown`].
    pub fn flush(&mut self) -> Vec<TerminalInputEvent> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        let ev = if self.buf.len() == 1 && self.buf[0] == ESC {
            TerminalInputEvent::Key(KeyEvent::plain(KeyCode::Esc))
        } else {
            TerminalInputEvent::Unknown(std::mem::take(&mut self.buf))
        };
        self.buf.clear();
        vec![ev]
    }

    fn step(&mut self, i: &mut usize) -> Step {
        if self.paste.is_some() {
            return self.collect_paste(i);
        }
        if *i >= self.buf.len() {
            return Step::End;
        }
        let b = self.buf[*i];
        match b {
            ESC => self.parse_escape(i),
            0x00..=0x1F | 0x7F => {
                *i += 1;
                Step::Emit(TerminalInputEvent::Key(key::key_from_control_byte(b)))
            }
            _ => self.parse_char(i),
        }
    }

    fn parse_char(&self, i: &mut usize) -> Step {
        match std::str::from_utf8(&self.buf[*i..]) {
            Ok(s) => emit_char(s, i),
            Err(e) => {
                if e.valid_up_to() > 0 {
                    let end = *i + e.valid_up_to();
                    return match std::str::from_utf8(&self.buf[*i..end]) {
                        Ok(valid) => emit_char(valid, i),
                        Err(_) => Step::Await,
                    };
                }
                if e.error_len().is_none() {
                    return Step::Await;
                }
                let bad = self.buf[*i];
                *i += 1;
                Step::Emit(TerminalInputEvent::Unknown(vec![bad]))
            }
        }
    }

    fn parse_escape(&mut self, i: &mut usize) -> Step {
        if *i + 1 >= self.buf.len() {
            return Step::Await;
        }
        match self.buf[*i + 1] {
            b'[' => self.parse_csi(i),
            b'O' => {
                if *i + 2 >= self.buf.len() {
                    return Step::Await;
                }
                let byte = self.buf[*i + 2];
                *i += 3;
                match key::key_from_ss3(byte) {
                    Some(ev) => Step::Emit(TerminalInputEvent::Key(ev)),
                    None => Step::Emit(TerminalInputEvent::Unknown(vec![ESC, b'O', byte])),
                }
            }
            // ESC + printable => Meta/Alt + char.
            0x20..=0x7E => {
                let c = char::from(self.buf[*i + 1]);
                *i += 2;
                Step::Emit(TerminalInputEvent::Key(KeyEvent::new(
                    KeyCode::Char(c),
                    KeyModifiers::ALT,
                )))
            }
            _ => {
                *i += 1;
                Step::Emit(TerminalInputEvent::Key(KeyEvent::plain(KeyCode::Esc)))
            }
        }
    }

    fn parse_csi(&mut self, i: &mut usize) -> Step {
        let start = *i + 2;
        let mut j = start;
        while j < self.buf.len() && !(0x40..=0x7E).contains(&self.buf[j]) {
            j += 1;
        }
        if j >= self.buf.len() {
            return Step::Await;
        }
        let final_byte = self.buf[j];
        let payload = self.buf[start..j].to_vec();

        if !payload.is_empty() && payload[0] == b'<' {
            let step = self.decode_sgr_mouse(&payload, final_byte, j);
            *i = j + 1;
            return step;
        }

        if final_byte == b'M' && payload.is_empty() {
            if j + 4 > self.buf.len() {
                return Step::Await;
            }
            let ev = decode_legacy(self.buf[j + 1], self.buf[j + 2], self.buf[j + 3]);
            *i = j + 4;
            return Step::Emit(TerminalInputEvent::Mouse(ev));
        }

        self.decode_csi_payload(&payload, final_byte, i, j)
    }

    fn decode_sgr_mouse(&self, payload: &[u8], final_byte: u8, _j: usize) -> Step {
        if final_byte != b'M' && final_byte != b'm' {
            return Step::Emit(TerminalInputEvent::Unknown(payload.to_vec()));
        }
        let nums = parse_nums(&payload[1..]);
        if nums.len() < 3 {
            return Step::Emit(TerminalInputEvent::Unknown(payload.to_vec()));
        }
        let press = final_byte == b'M';
        let ev = decode_sgr(nums[0], nums[1], nums[2], press);
        Step::Emit(TerminalInputEvent::Mouse(ev))
    }

    fn decode_csi_payload(
        &mut self,
        payload: &[u8],
        final_byte: u8,
        i: &mut usize,
        j: usize,
    ) -> Step {
        match final_byte {
            b'I' if payload.is_empty() => {
                *i = j + 1;
                Step::Emit(TerminalInputEvent::Focus(FocusEvent::Gained))
            }
            b'O' if payload.is_empty() => {
                *i = j + 1;
                Step::Emit(TerminalInputEvent::Focus(FocusEvent::Lost))
            }
            b'~' => {
                let params = parse_nums(payload);
                if params.first() == Some(&200) {
                    *i = j + 1;
                    self.paste = Some(String::new());
                    return Step::Skip;
                }
                *i = j + 1;
                self.finish_key(&params, final_byte, payload)
            }
            _ => {
                let params = parse_nums(payload);
                *i = j + 1;
                self.finish_key(&params, final_byte, payload)
            }
        }
    }

    fn finish_key(&self, params: &[u16], final_byte: u8, payload: &[u8]) -> Step {
        match key::key_from_csi(params, final_byte) {
            Some(ev) => Step::Emit(TerminalInputEvent::Key(ev)),
            None => {
                let mut raw = payload.to_vec();
                raw.push(final_byte);
                Step::Emit(TerminalInputEvent::Unknown(raw))
            }
        }
    }

    fn collect_paste(&mut self, i: &mut usize) -> Step {
        let rest = self.buf[*i..].to_vec();
        if let Some(pos) = find_subslice(&rest, &PASTE_END) {
            let Some(mut content) = self.paste.take() else {
                return Step::End;
            };
            content.push_str(&String::from_utf8_lossy(&rest[..pos]));
            *i += pos + PASTE_END.len();
            return Step::Emit(TerminalInputEvent::Paste(content));
        }
        let keep = PASTE_END.len() - 1;
        if rest.len() > keep {
            let take = rest.len() - keep;
            if let Some(buffer) = &mut self.paste {
                buffer.push_str(&String::from_utf8_lossy(&rest[..take]));
            }
            *i += take;
        }
        Step::Await
    }
}

fn emit_char(s: &str, i: &mut usize) -> Step {
    match s.chars().next() {
        Some(c) => {
            *i += c.len_utf8();
            Step::Emit(TerminalInputEvent::Key(KeyEvent::char(c)))
        }
        None => Step::End,
    }
}

fn parse_nums(payload: &[u8]) -> Vec<u16> {
    payload.split(|&b| b == b';').map(parse_one_num).collect()
}

fn parse_one_num(bytes: &[u8]) -> u16 {
    if bytes.is_empty() {
        return 1;
    }
    let mut value: u32 = 0;
    for &b in bytes {
        if b.is_ascii_digit() {
            value = value.saturating_mul(10).saturating_add(u32::from(b - b'0'));
        }
    }
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Decode a complete byte buffer into all terminal input events it contains.
///
/// Convenience wrapper over [`Decoder::feed`] + [`Decoder::flush`] for
/// non-streaming callers and tests.
pub fn decode_all(bytes: &[u8]) -> Vec<TerminalInputEvent> {
    let mut decoder = Decoder::new();
    let mut events = decoder.feed(bytes);
    events.extend(decoder.flush());
    events
}
