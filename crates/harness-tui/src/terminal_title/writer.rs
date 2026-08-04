use std::fmt::{Display, Formatter};
use std::io::Write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleWriteError {
    IoError(String),
    SanitizationFailed,
}

impl Display for TitleWriteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(message) => write!(formatter, "terminal title write failed: {message}"),
            Self::SanitizationFailed => formatter.write_str("terminal title sanitization failed"),
        }
    }
}

impl std::error::Error for TitleWriteError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleWriter {
    suspended: bool,
    written: bool,
}

impl TitleWriter {
    pub fn new() -> Self {
        Self {
            suspended: false,
            written: false,
        }
    }

    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    pub fn resume(&mut self) {
        self.suspended = false;
    }

    pub fn write_title(
        &mut self,
        title: &str,
        out: &mut impl Write,
    ) -> Result<bool, TitleWriteError> {
        if self.suspended {
            return Ok(false);
        }
        let sanitized = super::sanitize::sanitize_title(title);
        out.write_all(format!("\x1b]2;{sanitized}\x07").as_bytes())
            .map_err(|error| TitleWriteError::IoError(error.to_string()))?;
        self.written = true;
        Ok(true)
    }

    pub fn reset(&mut self, out: &mut impl Write) -> Result<bool, TitleWriteError> {
        if !self.written {
            return Ok(false);
        }
        out.write_all(b"\x1b]2;\x07")
            .map_err(|error| TitleWriteError::IoError(error.to_string()))?;
        self.written = false;
        Ok(true)
    }
}

impl Default for TitleWriter {
    fn default() -> Self {
        Self::new()
    }
}
