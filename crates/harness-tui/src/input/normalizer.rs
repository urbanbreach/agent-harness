use std::fmt::{Display, Formatter};
use std::time::Duration;

use crate::mouse::MouseEvent;
use crate::terminal::{FocusEvent, KeyCode, KeyEvent, ResizeEvent, TerminalInputEvent};

use super::ctrl_c::{CtrlCAction, CtrlCTracker};
use super::esc::{EscAction, EscRouter};
use super::key::{normalize_key, NormalizedKey};
use super::paste::{NormalizedPaste, PasteDetector, PasteKind, PasteOutput};
use super::resize::ResizeDebouncer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedInput {
    Key(NormalizedKey),
    Paste(NormalizedPaste),
    Resize(ResizeEvent),
    Mouse(MouseEvent),
    Focus(FocusEvent),
    Escape(EscAction),
    CtrlC(CtrlCAction),
    Unknown(Vec<u8>),
}

impl NormalizedInput {
    pub fn paste(text: impl Into<String>, kind: PasteKind) -> Self {
        Self::Paste(NormalizedPaste::new(text, kind))
    }

    pub const fn interrupt(input_nonempty: bool) -> Self {
        Self::CtrlC(CtrlCAction::Interrupt { input_nonempty })
    }

    pub const fn kill() -> Self {
        Self::CtrlC(CtrlCAction::Kill)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizerError {
    NonMonotonicTimestamp {
        previous: Duration,
        current: Duration,
    },
}

impl Display for NormalizerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonMonotonicTimestamp { previous, current } => write!(
                formatter,
                "input timestamp moved backwards: previous={}ms, current={}ms",
                previous.as_millis(),
                current.as_millis()
            ),
        }
    }
}

impl std::error::Error for NormalizerError {}

#[derive(Debug, Default)]
pub struct InputNormalizer {
    paste: PasteDetector,
    resize: ResizeDebouncer,
    ctrl_c: CtrlCTracker,
    esc: EscRouter,
    composer_nonempty: bool,
    last_at: Option<Duration>,
}

impl InputNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ingest(
        &mut self,
        event: TerminalInputEvent,
    ) -> Result<Vec<NormalizedInput>, NormalizerError> {
        let at = self.last_at.map_or(Duration::ZERO, |last| {
            last.saturating_add(Duration::from_millis(1))
        });
        self.ingest_at(at, event)
    }

    pub fn ingest_at(
        &mut self,
        at: Duration,
        event: TerminalInputEvent,
    ) -> Result<Vec<NormalizedInput>, NormalizerError> {
        self.validate_timestamp(at)?;
        let mut output = self.resize_output(at);
        match event {
            TerminalInputEvent::Key(event) => output.extend(self.key_output(at, event)),
            TerminalInputEvent::Paste(text) => {
                output.extend(self.flush_paste());
                output.push(NormalizedInput::paste(text, PasteKind::Bracketed));
            }
            TerminalInputEvent::Resize(event) => {
                output.extend(self.flush_paste());
                self.resize.push(at, event);
            }
            TerminalInputEvent::Mouse(event) => {
                output.extend(self.flush_paste());
                output.push(NormalizedInput::Mouse(event));
            }
            TerminalInputEvent::Focus(event) => {
                output.extend(self.flush_paste());
                output.push(NormalizedInput::Focus(event));
            }
            TerminalInputEvent::Unknown(bytes) => {
                output.extend(self.flush_paste());
                output.push(NormalizedInput::Unknown(bytes));
            }
        }
        Ok(output)
    }

    pub fn ingest_bytes_at(
        &mut self,
        at: Duration,
        bytes: &[u8],
    ) -> Result<Vec<NormalizedInput>, NormalizerError> {
        let mut output = Vec::new();
        for event in crate::terminal::decode_all(bytes) {
            output.extend(self.ingest_at(at, event)?);
        }
        output.extend(self.flush_at(at)?);
        Ok(output)
    }

    pub fn flush_at(&mut self, at: Duration) -> Result<Vec<NormalizedInput>, NormalizerError> {
        self.validate_timestamp(at)?;
        let mut output = self.resize_output(at);
        let paste_output = self.paste.flush_all();
        output.extend(self.paste_output(paste_output));
        Ok(output)
    }

    pub fn set_composer_nonempty(&mut self, nonempty: bool) {
        self.composer_nonempty = nonempty;
    }

    pub fn esc_mut(&mut self) -> &mut EscRouter {
        &mut self.esc
    }

    fn key_output(&mut self, at: Duration, event: KeyEvent) -> Vec<NormalizedInput> {
        let key = normalize_key(event);
        if key.is_ctrl_c() {
            let mut output = self.flush_paste();
            output.push(NormalizedInput::CtrlC(
                self.ctrl_c.press(at, self.composer_nonempty),
            ));
            return output;
        }
        if key.is_escape() {
            let mut output = self.flush_paste();
            output.push(NormalizedInput::Escape(self.esc.handle()));
            return output;
        }
        if matches!(key.code, KeyCode::Char(_)) && key.modifiers.is_empty() {
            let paste_output = self.paste.ingest_key(at, key);
            return self.paste_output(paste_output);
        }
        let mut output = self.flush_paste();
        output.push(NormalizedInput::Key(key));
        output
    }

    fn resize_output(&mut self, at: Duration) -> Vec<NormalizedInput> {
        self.resize
            .flush_due(at)
            .map_or_else(Vec::new, |event| vec![NormalizedInput::Resize(event)])
    }

    fn flush_paste(&mut self) -> Vec<NormalizedInput> {
        let paste_output = self.paste.flush_all();
        self.paste_output(paste_output)
    }

    fn paste_output(&self, output: Vec<PasteOutput>) -> Vec<NormalizedInput> {
        output
            .into_iter()
            .map(|item| match item {
                PasteOutput::Key(key) => NormalizedInput::Key(key),
                PasteOutput::Paste(paste) => NormalizedInput::Paste(paste),
            })
            .collect()
    }

    fn validate_timestamp(&mut self, at: Duration) -> Result<(), NormalizerError> {
        if let Some(previous) = self.last_at {
            if at < previous {
                return Err(NormalizerError::NonMonotonicTimestamp {
                    previous,
                    current: at,
                });
            }
        }
        self.last_at = Some(at);
        Ok(())
    }
}
