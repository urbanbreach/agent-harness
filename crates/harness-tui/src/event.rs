use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind, MouseEvent};
use std::cell::RefCell;
use std::time::Duration;

thread_local! {
    static PENDING_EVENT: RefCell<Option<TuiEvent>> = const { RefCell::new(None) };
}

pub enum TuiEvent {
    Key(event::KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

pub fn poll(timeout: Duration) -> Result<Option<TuiEvent>> {
    if let Some(event) = PENDING_EVENT.with(|pending| pending.borrow_mut().take()) {
        return Ok(Some(event));
    }

    if event::poll(timeout)? {
        let ev = event::read()?;
        return normalize_event(ev);
    }
    Ok(None)
}

fn normalize_event(ev: Event) -> Result<Option<TuiEvent>> {
    match ev {
        Event::Key(key) => {
            if key.kind == KeyEventKind::Press {
                Ok(Some(TuiEvent::Key(key)))
            } else {
                Ok(None)
            }
        }
        Event::Resize(w, h) => coalesce_resize_events(w, h),
        Event::Mouse(mouse) => coalesce_mouse_events(mouse),
        _ => Ok(None),
    }
}

fn coalesce_resize_events(mut width: u16, mut height: u16) -> Result<Option<TuiEvent>> {
    while event::poll(Duration::ZERO)? {
        let next = event::read()?;
        match next {
            Event::Resize(next_width, next_height) => {
                width = next_width;
                height = next_height;
            }
            other => {
                stash_event(other);
                break;
            }
        }
    }

    Ok(Some(TuiEvent::Resize(width, height)))
}

fn coalesce_mouse_events(mut mouse: MouseEvent) -> Result<Option<TuiEvent>> {
    let event::MouseEventKind::Drag(button) = mouse.kind else {
        return Ok(Some(TuiEvent::Mouse(mouse)));
    };

    while event::poll(Duration::ZERO)? {
        let next = event::read()?;
        match next {
            Event::Mouse(next_mouse) if next_mouse.kind == event::MouseEventKind::Drag(button) => {
                mouse = next_mouse;
            }
            other => {
                stash_event(other);
                break;
            }
        }
    }

    Ok(Some(TuiEvent::Mouse(mouse)))
}

fn stash_event(ev: Event) {
    if let Some(event) = stashable_event(ev) {
        PENDING_EVENT.with(|pending| {
            *pending.borrow_mut() = Some(event);
        });
    }
}

fn stashable_event(ev: Event) -> Option<TuiEvent> {
    match ev {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(TuiEvent::Key(key)),
        Event::Mouse(mouse) => Some(TuiEvent::Mouse(mouse)),
        Event::Resize(width, height) => Some(TuiEvent::Resize(width, height)),
        _ => None,
    }
}
