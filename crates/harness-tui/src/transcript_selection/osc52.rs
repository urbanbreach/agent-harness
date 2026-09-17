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
    let sequence = format!(
        "\x1b]52;c;{}\x07",
        crate::clipboard::encode_base64(text.as_bytes())
    );
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
