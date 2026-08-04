use super::{PagerError, SavedState, TerminalControl};

pub fn restore_terminal_state(mut saved: SavedState) -> Result<(), PagerError> {
    let mut terminal = super::SystemTerminal;
    restore_terminal_state_with(&mut terminal, &mut saved)
}

pub fn restore_terminal_state_with<T: TerminalControl>(
    terminal: &mut T,
    saved: &mut SavedState,
) -> Result<(), PagerError> {
    if saved.restored {
        return Ok(());
    }
    saved.restored = true;
    let state = saved.state.clone();
    let mut first_error = None;
    for result in [
        terminal.reinitialize_capabilities(),
        terminal.restore_cursor(state.cursor),
        terminal.restore_media(state.media),
        terminal.restore_title(state.title.as_deref()),
        terminal.restore_mouse(state.mouse),
        terminal.restore_raw_mode(state.raw_mode),
    ] {
        if let Err(error) = result {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

pub struct RestoreGuard<'a, T: TerminalControl> {
    terminal: Option<&'a mut T>,
    saved: Option<SavedState>,
}

impl<'a, T: TerminalControl> RestoreGuard<'a, T> {
    pub(crate) fn new(terminal: &'a mut T, saved: SavedState) -> Self {
        Self {
            terminal: Some(terminal),
            saved: Some(saved),
        }
    }

    pub fn restore(&mut self) -> Result<(), PagerError> {
        let Some(terminal) = self.terminal.as_deref_mut() else {
            return Ok(());
        };
        let Some(saved) = self.saved.as_mut() else {
            return Ok(());
        };
        restore_terminal_state_with(terminal, saved)
    }
}

impl<T: TerminalControl> Drop for RestoreGuard<'_, T> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
