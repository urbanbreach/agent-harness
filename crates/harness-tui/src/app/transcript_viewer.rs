use super::*;
use crate::transcript_selection::{CellPoint, NavigationKey, Viewport};

impl AppState {
    pub(super) fn refresh_transcript_viewer(&mut self) {
        if self.transcript_viewer.is_none() {
            return;
        }
        let Some(entry) = self.selected_transcript_entry() else {
            return;
        };
        let content = self.selected_entry_content(&entry);
        let child_running = self
            .current_subagent_session_present()
            .then(|| self.child_viewer_entry_running(&entry));
        if let Some(viewer) = self.transcript_viewer.as_mut() {
            let _ = viewer.update_content(content);
            if let Some(running) = child_running {
                viewer.set_child_running(running);
            }
        }
    }

    fn transcript_viewer_area(&self, area: Rect) -> Rect {
        if self.current_subagent_session_present() {
            crate::layout::FrameLayoutPlan::for_app(self, area).shell
        } else {
            area
        }
    }

    pub(super) fn handle_transcript_viewer_paste(&mut self, text: &str) -> bool {
        let Some(viewer) = self.transcript_viewer.as_mut() else {
            return false;
        };
        if !viewer.input.editing {
            return true;
        }
        if let Err(error) = viewer.input.paste(text) {
            self.status_banner = Some(error.to_string());
        }
        viewer.apply_input();
        self.resize_transcript_viewer(self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)));
        true
    }

    fn quote_viewer_to_prompt(&mut self, quote: &str) {
        if self.current_subagent_session_present() {
            self.navigate_to_parent_session();
        }
        self.focus = Focus::Prompt;
        self.tasks_pane.focused = false;
        self.todo_pane.focused = false;
        let prefix = self
            .composer
            .prompt_buffer
            .chars()
            .take(
                self.composer
                    .selection_anchor
                    .unwrap_or(self.composer.prompt_cursor)
                    .min(self.composer.prompt_cursor),
            )
            .last();
        let separator = if prefix.is_some_and(|c| c != '\n') {
            "\n"
        } else {
            ""
        };
        self.handle_paste(&format!("{separator}{quote}\n\n"));
    }

    pub(crate) fn resize_transcript_viewer(&mut self, area: Rect) {
        let theme = *self.theme();
        let child = self.current_subagent_session_present();
        let layout =
            crate::transcript_block_viewer::viewer_layout(self.transcript_viewer_area(area));
        if let Some(viewer) = self.transcript_viewer.as_mut() {
            let _ = viewer.set_theme(theme);
            let body = if child {
                layout.child_content_body(viewer.input_active())
            } else {
                layout.content_body(viewer.input_active() || viewer.visual_mode())
            };
            let _ = viewer.resize(
                usize::from(body.width.max(1)),
                usize::from(body.height.max(1)),
            );
        }
    }

    fn handle_child_viewer_command(&mut self, key: KeyEvent) -> bool {
        if self.current_subagent_session_present()
            && key.code == KeyCode::Char('r')
            && key.modifiers.is_empty()
            && self
                .transcript_viewer
                .as_ref()
                .is_some_and(|viewer| !viewer.search_editing() && !viewer.filter_editing())
        {
            self.toggle_child_markdown();
            return true;
        }
        if self.current_subagent_session_present()
            && key.code == KeyCode::Char('f')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.transcript_viewer.is_some()
        {
            return self.close_transcript_viewer();
        }
        false
    }

    pub(crate) fn handle_transcript_viewer_key(&mut self, key: KeyEvent) -> bool {
        if self.handle_child_viewer_command(key) {
            return true;
        }
        let Some(viewer) = self.transcript_viewer.as_mut() else {
            return false;
        };
        if viewer.search_editing() || viewer.filter_editing() {
            let mode = viewer.input.mode;
            if let Err(error) = viewer.input.handle_key(key) {
                self.status_banner = Some(error.to_string());
            }
            viewer.input.mode = mode;
            viewer.apply_input();
            self.resize_transcript_viewer(self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)));
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
        if let Some(navigation) = navigation.filter(|_| shift || viewer.visual_mode()) {
            viewer.move_cursor(navigation, true);
            viewer.reveal_cursor();
            return true;
        }
        let page = f64::from(u32::try_from(viewer.viewport_height()).unwrap_or(u32::MAX));
        if key.modifiers == KeyModifiers::CONTROL {
            let delta = match key.code {
                KeyCode::Char('j') => Some(1.0),
                KeyCode::Char('k') => Some(-1.0),
                // Grok's outer command registry reserves Ctrl-D; the viewer
                // consumes it without moving its cursor or closing the modal.
                KeyCode::Char('d') => return true,
                KeyCode::Char('u') => Some(-(page / 2.0).floor()),
                _ => None,
            };
            if let Some(delta) = delta {
                let _ = viewer.scroll_keeping_cursor(delta);
                return true;
            }
        }
        match key.code {
            KeyCode::Enter
                if key.modifiers.is_empty()
                    && key.kind == crossterm::event::KeyEventKind::Press =>
            {
                let text = viewer.quote_text();
                let text = text.trim_end_matches('\n');
                if text.is_empty() {
                    return true;
                }
                let quote = text
                    .lines()
                    .map(|line| {
                        if line.is_empty() {
                            ">".to_owned()
                        } else {
                            format!("> {line}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.close_transcript_viewer();
                self.quote_viewer_to_prompt(&quote);
            }
            KeyCode::Char('f') if key.modifiers.is_empty() => {
                viewer.set_filter_editing(true);
                let _ = viewer.set_search_query("");
            }
            KeyCode::Char('F') => viewer.toggle_follow(),
            KeyCode::Char('v') => viewer.toggle_visual(),
            KeyCode::Char('w') => {
                let _ = viewer.toggle_wrap();
            }
            KeyCode::Char('y') if viewer.is_child() && key.modifiers.is_empty() => {
                self.yank_viewer_line();
            }
            KeyCode::Char('Y') => self.copy_viewer_command(),
            KeyCode::Esc if viewer.clear_pointer_selection() => {}
            KeyCode::Esc if viewer.visual_mode() => viewer.toggle_visual(),
            KeyCode::Esc | KeyCode::Char('q') => {
                self.close_transcript_viewer();
            }
            KeyCode::Char('f') if key.modifiers == KeyModifiers::CONTROL => {
                self.close_transcript_viewer();
            }
            KeyCode::Char('/') => {
                viewer.set_search_editing(true);
                if !viewer.filter_query().is_empty() {
                    let _ = viewer.set_filter_query(String::new());
                }
            }
            KeyCode::Char('r' | 'R') if !viewer.is_child() => {
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
                viewer.navigate_line(false);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                viewer.navigate_line(true);
            }
            KeyCode::PageUp => {
                let _ = viewer.scroll_keeping_cursor(-page);
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                let _ = viewer.scroll_keeping_cursor(page);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                viewer.select_edge(false);
            }
            KeyCode::End | KeyCode::Char('G') => {
                viewer.select_edge(true);
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
        self.resize_transcript_viewer(self.last_frame_area.unwrap_or(Rect::new(0, 0, 80, 24)));
        // A viewer owns the whole surface even for keys it does not bind.
        true
    }

    fn copy_viewer_command(&mut self) {
        let Some(text) = self
            .transcript_viewer
            .as_ref()
            .and_then(|viewer| viewer.command_text())
        else {
            return;
        };
        self.copy_viewer_text(&text);
    }

    fn copy_viewer_text(&mut self, text: &str) {
        if let Err(error) = clipboard::copy(text) {
            self.show_toast(
                format!("clipboard copy failed: {error}"),
                ToastVariant::Error,
            );
        }
    }

    fn yank_viewer_line(&mut self) {
        let now = self.now();
        let Some(viewer) = self.transcript_viewer.as_mut() else {
            return;
        };
        let text = viewer.yank_text();
        if text.is_empty() {
            return;
        }
        match clipboard::copy(&text) {
            Ok(()) => viewer.copied_until = Some(now + Duration::from_millis(500)),
            Err(error) => self.show_toast(
                format!("clipboard copy failed: {error}"),
                ToastVariant::Error,
            ),
        }
    }

    fn finish_viewer_pointer(&mut self, point: CellPoint) {
        let anchor = self.transcript_view.viewer_pointer_anchor.take();
        let Some(viewer) = self
            .transcript_viewer
            .as_mut()
            .filter(|viewer| viewer.is_child())
        else {
            return;
        };
        let Some(anchor) = anchor else {
            return;
        };
        let _ = viewer.mouse_drag(
            anchor,
            point,
            Viewport {
                top: viewer.scroll_top(),
                height: viewer.viewport_height(),
            },
        );
        viewer.finish_pointer_drag();
        if let Ok(text) = viewer.copy_selection_text() {
            self.copy_viewer_text(&text);
        }
    }

    pub(crate) fn handle_transcript_viewer_mouse(&mut self, mouse: MouseEvent, area: Rect) -> bool {
        let scroll_step = if self.current_subagent_session_present() {
            1.0
        } else {
            3.0
        };
        let layout =
            crate::transcript_block_viewer::viewer_layout(self.transcript_viewer_area(area));
        let position = (mouse.column, mouse.row).into();
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && (layout.close.contains(position) || !layout.popup.contains(position))
        {
            return self.close_transcript_viewer();
        }
        let now = self.now();
        let Some(viewer) = self.transcript_viewer.as_mut() else {
            return false;
        };
        let body = if viewer.is_child() {
            layout.child_content_body(viewer.input_active())
        } else {
            layout.content_body(viewer.input_active() || viewer.visual_mode())
        };
        if viewer.pointer_scrollbar(mouse, body) {
            self.transcript_view.viewer_pointer_anchor = None;
            return true;
        }
        if viewer.is_child() && matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
            let outside = if mouse.row < body.y {
                -f64::from((body.y - mouse.row).min(5))
            } else if mouse.row >= body.bottom() {
                f64::from((mouse.row - body.bottom() + 1).min(5))
            } else {
                0.0
            };
            let _ = viewer.scroll_by(outside);
        }
        let row = mouse
            .row
            .clamp(body.y, body.bottom().saturating_sub(1).max(body.y));
        let column = mouse
            .column
            .clamp(body.x, body.right().saturating_sub(1).max(body.x));
        let point = viewer.pointer_point(CellPoint::new(
            viewer.scroll_top() + usize::from(row - body.y),
            usize::from(column - body.x),
        ));
        match mouse.kind {
            MouseEventKind::Moved => viewer.set_close_hovered(layout.close.contains(position)),
            MouseEventKind::ScrollUp => {
                let _ = viewer.scroll_wheel(-scroll_step);
            }
            MouseEventKind::ScrollDown => {
                let _ = viewer.scroll_wheel(scroll_step);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if !body.contains(position) {
                    return true;
                }
                if viewer.is_child() {
                    if let Some(text) = viewer.pointer_down(point, now) {
                        let _ = clipboard::copy(&text);
                    }
                    self.transcript_view.viewer_pointer_anchor =
                        viewer.pointer_dragging().then_some(point);
                    return true;
                }
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
                self.finish_viewer_pointer(point);
            }
            _ => {}
        }
        true
    }
}
