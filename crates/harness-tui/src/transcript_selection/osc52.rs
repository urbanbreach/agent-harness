use std::fmt::{Display, Formatter};

pub const OSC52_MAX_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmuxSequence {
    Direct,
    Tmux,
}

#[derive(Debug)]
pub enum Osc52Error {
    TooLarge { bytes: usize, max: usize },
    ClipboardDenied,
}

impl Osc52Error {
    pub const fn is_too_large(&self) -> bool {
        matches!(self, Self::TooLarge { .. })
    }
}

impl Display for Osc52Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes, max } => {
                write!(formatter, "OSC52 payload is {bytes} bytes; limit is {max}")
            }
            Self::ClipboardDenied => formatter.write_str("clipboard route was denied"),
        }
    }
}

impl std::error::Error for Osc52Error {}

pub fn build_osc52(text: &str, route: TmuxSequence) -> Result<String, Osc52Error> {
    if text.len() > OSC52_MAX_BYTES {
        return Err(Osc52Error::TooLarge {
            bytes: text.len(),
            max: OSC52_MAX_BYTES,
        });
    }
    let sequence = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    Ok(match route {
        TmuxSequence::Direct => sequence,
        TmuxSequence::Tmux => wrap_tmux(&sequence),
    })
}

pub fn route_osc52(
    text: &str,
    terminal_available: bool,
    route: TmuxSequence,
) -> Result<String, Osc52Error> {
    if !terminal_available {
        return Err(Osc52Error::ClipboardDenied);
    }
    build_osc52(text, route)
}

pub fn wrap_tmux(sequence: &str) -> String {
    let escaped = sequence.replace('\x1b', "\x1b\x1b");
    format!("\x1bPtmux;\x1b{escaped}\x1b\\")
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk.first().copied().unwrap_or(0);
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(char::from(TABLE[usize::from(first >> 2)]));
        encoded.push(char::from(
            TABLE[usize::from((first << 4 | second >> 4) & 0x3f)],
        ));
        encoded.push(if chunk.len() > 1 {
            char::from(TABLE[usize::from((second << 2 | third >> 6) & 0x3f)])
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            char::from(TABLE[usize::from(third & 0x3f)])
        } else {
            '='
        });
    }
    encoded
}
