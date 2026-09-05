use crossterm::event::{self, Event, KeyEventKind, MouseEvent};

pub enum TuiEvent {
    Key(event::KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize(u16, u16),
    FocusGained,
    FocusLost,
}

pub fn normalize_event(ev: Event) -> Option<TuiEvent> {
    match ev {
        Event::Key(key) => {
            if key.kind == KeyEventKind::Press {
                Some(TuiEvent::Key(key))
            } else {
                None
            }
        }
        Event::Paste(text) => Some(TuiEvent::Paste(text)),
        Event::Resize(w, h) => Some(TuiEvent::Resize(w, h)),
        Event::Mouse(mouse) => Some(TuiEvent::Mouse(mouse)),
        Event::FocusGained => Some(TuiEvent::FocusGained),
        Event::FocusLost => Some(TuiEvent::FocusLost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn paste_events_are_preserved_for_prompt_insertion() {
        let event = normalize_event(Event::Paste("alpha\nbeta".to_string())).unwrap_or_abort();

        match event {
            TuiEvent::Paste(text) => assert_eq!(text, "alpha\nbeta"),
            _ => panic!("expected paste event"),
        }
    }
}
