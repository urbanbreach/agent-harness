use super::*;
use crate::transcript_selection::{CellPoint, NavigationKey, Viewport};

impl AppState {
    pub(crate) fn resize_transcript_viewer(&mut self, area: Rect) {
        let theme = *self.theme();
        if let Some(viewer) = self
            .transcript_integration
            .as_mut()
            .and_then(TranscriptComposite::viewer_mut)
        {
            let _ = viewer.set_theme(theme);
            let _ = viewer.resize(
                usize::from(area.width.saturating_sub(2).max(1)),
                usize::from(area.height.saturating_sub(3).max(1)),
            );
        }
    }

    pub(crate) fn handle_transcript_viewer_key(&mut self, key: KeyEvent) -> bool {
        let Some(viewer) = self
            .transcript_integration
            .as_mut()
            .and_then(TranscriptComposite::viewer_mut)
        else {
            return false;
        };
        if viewer.search_editing() {
            let mut query = viewer.search().query().to_owned();
            match key.code {
                KeyCode::Esc | KeyCode::Enter => viewer.set_search_editing(false),
                KeyCode::Backspace => {
                    let _ = query.pop();
                    let _ = viewer.set_search_query(&query);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    query.push(c);
                    let _ = viewer.set_search_query(&query);
                }
                _ => {}
            }
            return true;
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let navigation = match key.code {
            KeyCode::Left => Some(NavigationKey::Left),
            KeyCode::Right => Some(NavigationKey::Right),
            KeyCode::Up => Some(NavigationKey::Up),
            KeyCode::Down => Some(NavigationKey::Down),
            KeyCode::Home => Some(NavigationKey::Home),
            KeyCode::End => Some(NavigationKey::End),
            _ => None,
        };
        if let Some(navigation) = navigation.filter(|_| shift) {
            viewer.move_cursor(navigation, true);
            viewer.reveal_cursor();
            return true;
        }
        let page = f64::from(
            u32::try_from(viewer.viewport_height().saturating_sub(1).max(1)).unwrap_or(u32::MAX),
        );
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.close_transcript_viewer();
            }
            KeyCode::Char('f') if key.modifiers == KeyModifiers::CONTROL => {
                self.close_transcript_viewer();
            }
            KeyCode::Char('/') => {
                viewer.set_search_editing(true);
                let _ = viewer.set_search_query("");
            }
            KeyCode::Char('r' | 'R') => {
                let _ = viewer.toggle_mode();
            }
            KeyCode::Char('n' | 'N') => {
                if shift || key.code == KeyCode::Char('N') {
                    let _ = viewer.search_backward();
                } else {
                    let _ = viewer.search_forward();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let _ = viewer.scroll_by(-1.0);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let _ = viewer.scroll_by(1.0);
            }
            KeyCode::PageUp => {
                let _ = viewer.scroll_by(-page);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                let _ = viewer.scroll_by(page);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                let _ = viewer.scroll_by(-f64::from(u32::MAX));
            }
            KeyCode::End | KeyCode::Char('G') => {
                let _ = viewer.scroll_by(f64::from(u32::MAX));
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Ok(text) = viewer.copy_selection_text() {
                    match clipboard::copy(&text) {
                        Ok(()) => self.show_toast("Copied to clipboard", ToastVariant::Info),
                        Err(error) => self.show_toast(
                            format!("clipboard copy failed: {error}"),
                            ToastVariant::Error,
                        ),
                    }
                }
            }
            _ => {}
        }
        // A viewer owns the whole surface even for keys it does not bind.
        true
    }

    pub(crate) fn handle_transcript_viewer_mouse(&mut self, mouse: MouseEvent, area: Rect) -> bool {
        let transcript = area;
        let Some(viewer) = self
            .transcript_integration
            .as_mut()
            .and_then(TranscriptComposite::viewer_mut)
        else {
            return false;
        };
        let point = CellPoint::new(
            viewer.scroll_top() + usize::from(mouse.row.saturating_sub(transcript.y + 1)),
            usize::from(mouse.column.saturating_sub(transcript.x + 1)),
        );
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                let _ = viewer.scroll_by(-3.0);
            }
            MouseEventKind::ScrollDown => {
                let _ = viewer.scroll_by(3.0);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.transcript_view.viewer_pointer_anchor = Some(point);
                let _ = viewer.mouse_drag(
                    point,
                    point,
                    Viewport {
                        top: viewer.scroll_top(),
                        height: viewer.viewport_height(),
                    },
                );
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(anchor) = self.transcript_view.viewer_pointer_anchor {
                    let _ = viewer.mouse_drag(
                        anchor,
                        point,
                        Viewport {
                            top: viewer.scroll_top(),
                            height: viewer.viewport_height(),
                        },
                    );
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.transcript_view.viewer_pointer_anchor = None;
            }
            _ => {}
        }
        true
    }
}
