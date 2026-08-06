use std::io::{self, Write};

use crossterm::cursor::{Hide, Show};
use crossterm::event::DisableMouseCapture;
use crossterm::terminal::SetTitle;

use super::PagerError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalState {
    pub raw_mode: bool,
    pub mouse: bool,
    pub cursor: bool,
    pub title: Option<String>,
    pub media: bool,
    pub columns: u16,
    pub rows: u16,
}

impl TerminalState {
    pub fn active(columns: u16, rows: u16) -> Self {
        Self {
            raw_mode: true,
            mouse: true,
            cursor: true,
            title: Some("Harness".to_owned()),
            media: true,
            columns,
            rows,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
    SuspendRawMode,
    SuspendMouse,
    SuspendTitle,
    SuspendMedia,
    SuspendCursor,
    RestoreCapabilities,
    RestoreCursor,
    RestoreMedia,
    RestoreTitle,
    RestoreMouse,
    RestoreRawMode,
}

pub trait TerminalControl {
    fn state(&self) -> TerminalState;
    fn suspend_raw_mode(&mut self) -> Result<(), PagerError>;
    fn suspend_mouse(&mut self) -> Result<(), PagerError>;
    fn reset_title(&mut self) -> Result<(), PagerError>;
    fn pause_media(&mut self) -> Result<(), PagerError>;
    fn suspend_cursor(&mut self) -> Result<(), PagerError>;
    fn reinitialize_capabilities(&mut self) -> Result<(), PagerError>;
    fn restore_cursor(&mut self, visible: bool) -> Result<(), PagerError>;
    fn restore_media(&mut self, active: bool) -> Result<(), PagerError>;
    fn restore_title(&mut self, title: Option<&str>) -> Result<(), PagerError>;
    fn restore_mouse(&mut self, active: bool) -> Result<(), PagerError>;
    fn restore_raw_mode(&mut self, active: bool) -> Result<(), PagerError>;
}

#[derive(Debug)]
pub struct SavedState {
    pub(crate) state: TerminalState,
    pub(crate) restored: bool,
}

impl SavedState {
    pub(crate) fn new(state: TerminalState) -> Self {
        Self {
            state,
            restored: false,
        }
    }
}

pub fn suspend_terminal_state() -> SavedState {
    let mut terminal = SystemTerminal;
    let state = terminal.state();
    match suspend_terminal_state_with_state(&mut terminal, state.clone()) {
        Ok(saved) => saved,
        Err(_) => SavedState::new(state),
    }
}

pub fn suspend_terminal_state_with<T: TerminalControl>(
    terminal: &mut T,
) -> Result<SavedState, PagerError> {
    let state = terminal.state();
    suspend_terminal_state_with_state(terminal, state)
}

fn suspend_terminal_state_with_state<T: TerminalControl>(
    terminal: &mut T,
    state: TerminalState,
) -> Result<SavedState, PagerError> {
    let mut saved = SavedState::new(state.clone());
    let result = (|| {
        if state.raw_mode {
            terminal.suspend_raw_mode()?;
        }
        if state.mouse {
            terminal.suspend_mouse()?;
        }
        if state.title.is_some() {
            terminal.reset_title()?;
        }
        if state.media {
            terminal.pause_media()?;
        }
        if state.cursor {
            terminal.suspend_cursor()?;
        }
        Ok::<(), PagerError>(())
    })();
    if let Err(error) = result {
        let _ = super::restore_terminal_state_with(terminal, &mut saved);
        return Err(error);
    }
    Ok(saved)
}

#[derive(Debug)]
pub struct SystemTerminal;

impl TerminalControl for SystemTerminal {
    fn state(&self) -> TerminalState {
        let (columns, rows) = crossterm::terminal::size().unwrap_or((0, 0));
        TerminalState::active(columns, rows)
    }

    fn suspend_raw_mode(&mut self) -> Result<(), PagerError> {
        crossterm::terminal::disable_raw_mode().map_err(|error| terminal_error("raw mode", error))
    }

    fn suspend_mouse(&mut self) -> Result<(), PagerError> {
        execute_stdout(DisableMouseCapture, "mouse")
    }

    fn reset_title(&mut self) -> Result<(), PagerError> {
        execute_stdout(SetTitle(""), "title")
    }

    fn pause_media(&mut self) -> Result<(), PagerError> {
        io::stdout()
            .flush()
            .map_err(|error| terminal_error("media", error))
    }

    fn suspend_cursor(&mut self) -> Result<(), PagerError> {
        execute_stdout(Hide, "cursor")
    }

    fn reinitialize_capabilities(&mut self) -> Result<(), PagerError> {
        io::stdout()
            .flush()
            .map_err(|error| terminal_error("capabilities", error))
    }

    fn restore_cursor(&mut self, visible: bool) -> Result<(), PagerError> {
        if visible {
            execute_stdout(Show, "cursor")
        } else {
            execute_stdout(Hide, "cursor")
        }
    }

    fn restore_media(&mut self, _active: bool) -> Result<(), PagerError> {
        io::stdout()
            .flush()
            .map_err(|error| terminal_error("media", error))
    }

    fn restore_title(&mut self, title: Option<&str>) -> Result<(), PagerError> {
        execute_stdout(SetTitle(title.unwrap_or("")), "title")
    }

    fn restore_mouse(&mut self, _active: bool) -> Result<(), PagerError> {
        io::stdout()
            .flush()
            .map_err(|error| terminal_error("mouse", error))
    }

    fn restore_raw_mode(&mut self, active: bool) -> Result<(), PagerError> {
        if active {
            crossterm::terminal::enable_raw_mode()
                .map_err(|error| terminal_error("raw mode", error))
        } else {
            Ok(())
        }
    }
}

fn execute_stdout<T: crossterm::Command>(
    command: T,
    operation: &'static str,
) -> Result<(), PagerError> {
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, command).map_err(|error| terminal_error(operation, error))
}

fn terminal_error(operation: &'static str, error: impl std::fmt::Display) -> PagerError {
    PagerError::Terminal {
        operation,
        detail: error.to_string(),
    }
}
