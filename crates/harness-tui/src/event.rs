use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use std::time::Duration;

pub enum TuiEvent {
    Key(event::KeyEvent),
    Resize(u16, u16),
}

pub fn poll(timeout: Duration) -> Result<Option<TuiEvent>> {
    if event::poll(timeout)? {
        let ev = event::read()?;
        return match ev {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    Ok(Some(TuiEvent::Key(key)))
                } else {
                    Ok(None)
                }
            }
            Event::Resize(w, h) => {
                // Coalesce resize events
                let mut current_w = w;
                let mut current_h = h;
                while event::poll(Duration::from_millis(0))? {
                    if let Event::Resize(new_w, new_h) = event::read()? {
                        current_w = new_w;
                        current_h = new_h;
                    }
                }
                Ok(Some(TuiEvent::Resize(current_w, current_h)))
            }
            _ => Ok(None),
        };
    }
    Ok(None)
}
