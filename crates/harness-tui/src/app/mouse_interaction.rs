// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::*;

impl AppState {
    pub(in crate::app) fn set_transcript_selection(
        &mut self,
        anchor: TranscriptSelectionCell,
        focus: TranscriptSelectionCell,
    ) {
        self.transcript_view.transcript_selection = Some(TranscriptSelection { anchor, focus });
    }

    pub(in crate::app) fn clear_transcript_selection(&mut self) {
        self.transcript_view.transcript_selection = None;
        self.transcript_view.transcript_selection_dragging = false;
    }

    pub(in crate::app) fn set_operator_sidebar_selection(
        &mut self,
        anchor: OperatorSidebarSelectionCell,
        focus: OperatorSidebarSelectionCell,
    ) {
        self.secondary_surfaces.selection = Some(OperatorSidebarSelection { anchor, focus });
    }

    pub(in crate::app) fn clear_operator_sidebar_selection(&mut self) {
        self.secondary_surfaces.selection = None;
        self.secondary_surfaces.selection_dragging = false;
        self.secondary_surfaces.pending_click = None;
    }

    fn copy_transcript_selection(&mut self, frame_area: Rect) -> bool {
        let Some(selection) = self.transcript_view.transcript_selection else {
            return false;
        };
        let Some(text) = ui::transcript_selection_text(self, frame_area, selection) else {
            return false;
        };

        match clipboard::copy(&text) {
            Ok(()) => self.show_toast("Copied to clipboard", ToastVariant::Info),
            Err(err) => {
                self.show_toast(format!("clipboard copy failed: {err}"), ToastVariant::Error)
            }
        }
        true
    }

    fn copy_operator_sidebar_selection(&mut self, frame_area: Rect) -> bool {
        let Some(selection) = self.secondary_surfaces.selection else {
            return false;
        };
        let Some(text) = ui::operator_sidebar_selection_text(self, frame_area, selection) else {
            return false;
        };

        match clipboard::copy(&text) {
            Ok(()) => self.show_toast("Copied to clipboard", ToastVariant::Info),
            Err(err) => {
                self.show_toast(format!("clipboard copy failed: {err}"), ToastVariant::Error)
            }
        }
        true
    }

    pub(in crate::app) fn copy_active_selection(&mut self, frame_area: Rect) -> bool {
        self.copy_operator_sidebar_selection(frame_area)
            || self.copy_transcript_selection(frame_area)
    }

    fn maybe_clear_empty_transcript_selection(&mut self, frame_area: Rect) {
        if self
            .transcript_view
            .transcript_selection
            .and_then(|selection| ui::transcript_selection_text(self, frame_area, selection))
            .is_none()
        {
            self.clear_transcript_selection();
        }
    }

    fn operator_sidebar_selection_has_text(&self, frame_area: Rect) -> bool {
        self.secondary_surfaces
            .selection
            .and_then(|selection| ui::operator_sidebar_selection_text(self, frame_area, selection))
            .is_some()
    }

    fn activate_operator_sidebar_pending_click(&mut self) -> bool {
        let Some(target) = self.secondary_surfaces.pending_click.take() else {
            return false;
        };
        match target {
            OperatorSidebarPendingClick::Section(section) => {
                self.toggle_operator_sidebar_section(section)
            }
            OperatorSidebarPendingClick::SubagentGroup(agent_name) => {
                self.toggle_operator_sidebar_subagent_group(agent_name)
            }
            OperatorSidebarPendingClick::SubagentSession(session_id) => {
                self.navigate_to_child_session_id(session_id)
            }
        }
        true
    }

    fn activate_subagent_footer_target(&mut self, target: SubagentFooterTarget) {
        match target {
            SubagentFooterTarget::Parent => self.navigate_to_parent_session(),
            SubagentFooterTarget::Previous => self.navigate_to_child_sibling(true),
            SubagentFooterTarget::Next => self.navigate_to_child_sibling(false),
        }
    }

    pub(in crate::app) fn operator_sidebar_keyboard_active(&self) -> bool {
        self.active_review_surface.is_none()
            && self.focus == Focus::List
            && !self.startup_shell_visible()
            && !self.post_run_handoff_visible()
            && self.session_shell_operator_rail_interactive()
    }

    pub(in crate::app) fn operator_sidebar_keyboard_targets(
        &self,
    ) -> Vec<OperatorSidebarKeyboardTarget> {
        ui::operator_sidebar_keyboard_targets(self, self.last_frame_area)
    }

    fn selected_operator_sidebar_keyboard_target(
        &mut self,
    ) -> Option<OperatorSidebarKeyboardTargetKind> {
        let targets = self.operator_sidebar_keyboard_targets();
        if targets.is_empty() {
            self.secondary_surfaces.keyboard_index = None;
            return None;
        }

        let index = self
            .secondary_surfaces
            .keyboard_index
            .unwrap_or(0)
            .min(targets.len().saturating_sub(1));
        self.secondary_surfaces.keyboard_index = Some(index);
        targets.get(index).map(|target| target.kind.clone())
    }

    fn move_operator_sidebar_keyboard_selection(&mut self, reverse: bool) -> bool {
        let targets = self.operator_sidebar_keyboard_targets();
        if targets.is_empty() {
            self.secondary_surfaces.keyboard_index = None;
            return false;
        }

        let next = match self.secondary_surfaces.keyboard_index {
            Some(index) if reverse => index.saturating_sub(1),
            Some(index) => (index + 1).min(targets.len().saturating_sub(1)),
            None if reverse => targets.len().saturating_sub(1),
            None => 0,
        };
        self.secondary_surfaces.keyboard_index = Some(next);
        self.details_scroll =
            u16::try_from(targets[next].top_row.min(usize::from(u16::MAX))).unwrap_or(u16::MAX);
        true
    }

    fn activate_operator_sidebar_keyboard_selection(&mut self) -> bool {
        let Some(target) = self.selected_operator_sidebar_keyboard_target() else {
            return false;
        };

        match target {
            OperatorSidebarKeyboardTargetKind::Section(section) => {
                self.toggle_operator_sidebar_section(section);
            }
            OperatorSidebarKeyboardTargetKind::SubagentGroup(agent_name) => {
                self.toggle_operator_sidebar_subagent_group(agent_name);
            }
            OperatorSidebarKeyboardTargetKind::SubagentSession(session_id) => {
                self.navigate_to_child_session_id(session_id);
            }
        }
        true
    }

    pub(in crate::app) fn handle_operator_sidebar_action(&mut self, action: Action) -> bool {
        if !self.operator_sidebar_keyboard_active() {
            return false;
        }

        match action {
            Action::MoveDown | Action::HistoryDown => {
                self.move_operator_sidebar_keyboard_selection(false)
            }
            Action::MoveUp | Action::HistoryUp => {
                self.move_operator_sidebar_keyboard_selection(true)
            }
            Action::SubmitPrompt => self.activate_operator_sidebar_keyboard_selection(),
            _ => false,
        }
    }

    pub(crate) fn set_frame_area(&mut self, area: Rect) {
        self.last_frame_area = Some(area);
    }

    pub(crate) fn last_frame_area(&self) -> Option<Rect> {
        self.last_frame_area
    }

    pub(crate) fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        frame_area: Rect,
        hovered_wheel_target: Option<WheelTarget>,
        clicked_operator_sidebar_section: Option<OperatorSidebarSection>,
        transcript_scrollbar_hit: Option<TranscriptScrollbarHit>,
    ) -> bool {
        if self.file_mention_visible {
            let mention_overlay =
                crate::layout::FrameLayoutPlan::for_app(self, frame_area).slash_overlay;
            if let Some(overlay) =
                mention_overlay.filter(|overlay| rect_contains(*overlay, mouse.column, mouse.row))
            {
                self.handle_file_mention_mouse(mouse, overlay);
                return true;
            }
        }

        if self.slash_visible {
            let slash_overlay =
                crate::layout::FrameLayoutPlan::for_app(self, frame_area).slash_overlay;
            if let Some(overlay) =
                slash_overlay.filter(|overlay| rect_contains(*overlay, mouse.column, mouse.row))
            {
                self.handle_slash_mouse(mouse, overlay);
                return true;
            }
        }

        if self.overlay_stack().blocks_pointer_interaction() {
            let changed = self.transcript_view.transcript_scrollbar_drag.is_some()
                || self.transcript_view.hovered_transcript_target.is_some()
                || self.hovered_subagent_footer_target.is_some()
                || self.transcript_view.transcript_selection.is_some()
                || self.secondary_surfaces.selection.is_some();
            self.transcript_view.transcript_scrollbar_drag = None;
            self.transcript_view.hovered_transcript_target = None;
            self.hovered_subagent_footer_target = None;
            self.pending_subagent_footer_target = None;
            self.clear_transcript_selection();
            self.clear_operator_sidebar_selection();
            return changed;
        }

        self.set_frame_area(frame_area);

        match mouse.kind {
            MouseEventKind::Moved => {
                let hovered_subagent_footer_target =
                    ui::subagent_footer_target_at(self, frame_area, mouse.column, mouse.row);
                let hovered_transcript_target = if hovered_subagent_footer_target.is_none() {
                    ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row)
                } else {
                    None
                };
                let changed = self.transcript_view.hovered_transcript_target
                    != hovered_transcript_target
                    || self.hovered_subagent_footer_target != hovered_subagent_footer_target;
                self.transcript_view.hovered_transcript_target = hovered_transcript_target;
                self.hovered_subagent_footer_target = hovered_subagent_footer_target;
                changed
            }
            MouseEventKind::Down(MouseButton::Right) => false,
            MouseEventKind::Down(MouseButton::Left) => {
                self.transcript_view.transcript_click_activated_on_down = false;
                self.hovered_subagent_footer_target =
                    ui::subagent_footer_target_at(self, frame_area, mouse.column, mouse.row);
                self.pending_subagent_footer_target = self.hovered_subagent_footer_target;
                self.transcript_view.hovered_transcript_target =
                    if self.hovered_subagent_footer_target.is_none() {
                        ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row)
                    } else {
                        None
                    };
                if self.hovered_subagent_footer_target.is_some() {
                    self.transcript_view.transcript_scrollbar_drag = None;
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    return true;
                }
                if let Some(scrollbar) = transcript_scrollbar_hit
                    .filter(|scrollbar| rect_contains(scrollbar.thumb, mouse.column, mouse.row))
                {
                    self.begin_transcript_scrollbar_drag(scrollbar, mouse.row);
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    return true;
                }

                self.transcript_view.transcript_scrollbar_drag = None;

                let plan = crate::layout::FrameLayoutPlan::for_app(self, frame_area);
                let operator_surface = plan.operator_sidebar.or(plan.details_overlay);
                let in_operator_surface = operator_surface
                    .is_some_and(|area| rect_contains(area, mouse.column, mouse.row));
                if in_operator_surface {
                    self.clear_transcript_selection();
                    let operator_sidebar_session = ui::operator_sidebar_subagent_session_hit_target(
                        self,
                        frame_area,
                        mouse.column,
                        mouse.row,
                    );
                    let operator_sidebar_group = ui::operator_sidebar_subagent_group_hit_target(
                        self,
                        frame_area,
                        mouse.column,
                        mouse.row,
                    );
                    let operator_sidebar_cell = ui::operator_sidebar_selection_cell(
                        self,
                        frame_area,
                        mouse.column,
                        mouse.row,
                    );
                    if let Some(cell) = operator_sidebar_cell {
                        self.set_operator_sidebar_selection(cell, cell);
                        self.secondary_surfaces.selection_dragging = true;
                        self.secondary_surfaces.pending_click = operator_sidebar_session
                            .map(OperatorSidebarPendingClick::SubagentSession)
                            .or(operator_sidebar_group
                                .map(OperatorSidebarPendingClick::SubagentGroup))
                            .or(clicked_operator_sidebar_section
                                .map(OperatorSidebarPendingClick::Section));
                        return true;
                    }
                    if let Some(agent_name) = operator_sidebar_group {
                        self.clear_operator_sidebar_selection();
                        self.toggle_operator_sidebar_subagent_group(agent_name);
                        return true;
                    }
                    if let Some(section) = clicked_operator_sidebar_section {
                        self.clear_operator_sidebar_selection();
                        self.toggle_operator_sidebar_section(section);
                    }
                    return true;
                }

                if let Some(section) = clicked_operator_sidebar_section {
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    self.toggle_operator_sidebar_section(section);
                    return true;
                }

                if let Some(target) =
                    ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row)
                {
                    self.activate_transcript_mouse_target(target);
                    self.transcript_view.transcript_click_activated_on_down = true;
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    return true;
                }
                let transcript_hit =
                    ui::transcript_selection_cell(self, frame_area, mouse.column, mouse.row);
                if let Some(cell) = transcript_hit {
                    self.set_transcript_selection(cell, cell);
                    self.transcript_view.transcript_selection_dragging = true;
                    self.clear_operator_sidebar_selection();
                    return true;
                }

                self.clear_transcript_selection();
                self.clear_operator_sidebar_selection();
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.transcript_view.transcript_scrollbar_drag.is_some() {
                    self.update_transcript_scrollbar_drag(mouse.row);
                    return true;
                }

                if self.transcript_view.transcript_selection_dragging {
                    let transcript_hit =
                        ui::transcript_selection_cell(self, frame_area, mouse.column, mouse.row);
                    if let Some(cell) = transcript_hit {
                        if let Some(selection) = self.transcript_view.transcript_selection {
                            self.set_transcript_selection(selection.anchor, cell);
                        }
                    }
                    true
                } else if self.secondary_surfaces.selection_dragging {
                    let sidebar_hit = ui::operator_sidebar_selection_cell(
                        self,
                        frame_area,
                        mouse.column,
                        mouse.row,
                    );
                    if let Some(cell) = sidebar_hit {
                        if let Some(selection) = self.secondary_surfaces.selection {
                            self.set_operator_sidebar_selection(selection.anchor, cell);
                        }
                    }
                    true
                } else {
                    if let Some(pending) = self.pending_subagent_footer_target {
                        let current = ui::subagent_footer_target_at(
                            self,
                            frame_area,
                            mouse.column,
                            mouse.row,
                        );
                        if current != Some(pending) {
                            self.pending_subagent_footer_target = None;
                        }
                    }
                    false
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let footer_target =
                    ui::subagent_footer_target_at(self, frame_area, mouse.column, mouse.row);
                let pending_footer_target = self.pending_subagent_footer_target.take();
                if let Some(target) =
                    footer_target.filter(|target| pending_footer_target == Some(*target))
                {
                    self.hovered_subagent_footer_target = Some(target);
                    self.activate_subagent_footer_target(target);
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    self.transcript_view.transcript_scrollbar_drag = None;
                    self.transcript_view.transcript_click_activated_on_down = false;
                    return true;
                }
                let operator_sidebar_was_dragging = self.secondary_surfaces.selection_dragging;
                if self.secondary_surfaces.selection_dragging {
                    let sidebar_hit = ui::operator_sidebar_selection_cell(
                        self,
                        frame_area,
                        mouse.column,
                        mouse.row,
                    );
                    if let Some(cell) = sidebar_hit {
                        if let Some(selection) = self.secondary_surfaces.selection {
                            self.set_operator_sidebar_selection(selection.anchor, cell);
                        }
                    }
                    self.secondary_surfaces.selection_dragging = false;
                    let copy_on_select_disabled = clipboard::copy_on_select_disabled();
                    if copy_on_select_disabled {
                        if self.operator_sidebar_selection_has_text(frame_area) {
                            self.secondary_surfaces.pending_click = None;
                        } else {
                            self.activate_operator_sidebar_pending_click();
                            self.clear_operator_sidebar_selection();
                        }
                    } else {
                        let copied = self.copy_operator_sidebar_selection(frame_area);
                        if copied {
                            self.clear_operator_sidebar_selection();
                        } else {
                            self.activate_operator_sidebar_pending_click();
                            self.clear_operator_sidebar_selection();
                        }
                    }
                }
                if self.transcript_view.transcript_selection_dragging {
                    let transcript_hit =
                        ui::transcript_selection_cell(self, frame_area, mouse.column, mouse.row);
                    if let Some(cell) = transcript_hit {
                        if let Some(selection) = self.transcript_view.transcript_selection {
                            self.set_transcript_selection(selection.anchor, cell);
                        }
                    }
                    self.transcript_view.transcript_selection_dragging = false;
                    let copy_on_select_disabled = clipboard::copy_on_select_disabled();
                    if copy_on_select_disabled {
                        self.maybe_clear_empty_transcript_selection(frame_area);
                    } else {
                        let copied = self.copy_transcript_selection(frame_area);
                        self.clear_transcript_selection();
                        if !copied {
                            self.clear_transcript_selection();
                        }
                    }
                }
                if self.transcript_view.transcript_click_activated_on_down {
                    self.transcript_view.transcript_click_activated_on_down = false;
                    self.transcript_view.transcript_scrollbar_drag = None;
                    return true;
                }
                if operator_sidebar_was_dragging {
                    self.transcript_view.transcript_scrollbar_drag = None;
                    return true;
                }
                if self.transcript_view.transcript_scrollbar_drag.is_none() {
                    if let Some(target) =
                        ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row)
                    {
                        self.activate_transcript_mouse_target(target);
                        self.clear_transcript_selection();
                        return true;
                    }
                }
                self.transcript_view.transcript_scrollbar_drag = None;
                true
            }
            MouseEventKind::ScrollUp => match hovered_wheel_target {
                Some(WheelTarget::Transcript) => {
                    self.scroll_transcript_up(self.mouse_wheel_lines_per_tick);
                    true
                }
                Some(WheelTarget::Terminal) => {
                    self.scroll_terminal_panel_up(self.mouse_wheel_lines_per_tick);
                    true
                }
                Some(WheelTarget::Inspector) => {
                    self.details_scroll = self
                        .details_scroll
                        .saturating_sub(self.mouse_wheel_lines_per_tick);
                    true
                }
                None => false,
            },
            MouseEventKind::ScrollDown => match hovered_wheel_target {
                Some(WheelTarget::Transcript) => {
                    self.scroll_transcript_down(self.mouse_wheel_lines_per_tick);
                    true
                }
                Some(WheelTarget::Terminal) => {
                    self.scroll_terminal_panel_down(self.mouse_wheel_lines_per_tick);
                    true
                }
                Some(WheelTarget::Inspector) => {
                    self.details_scroll = self
                        .details_scroll
                        .saturating_add(self.mouse_wheel_lines_per_tick);
                    true
                }
                None => false,
            },
            _ => false,
        }
    }

    pub(crate) fn operator_sidebar_section_collapsed(
        &self,
        section: OperatorSidebarSection,
    ) -> bool {
        self.secondary_surfaces
            .collapsed_sections
            .contains(&section)
    }

    pub(crate) fn operator_sidebar_subagent_group_expanded(&self, agent_name: &str) -> bool {
        self.secondary_surfaces
            .expanded_subagent_groups
            .contains(agent_name)
    }

    pub(crate) fn transcript_scrollbar_dragging(&self) -> bool {
        self.transcript_view.transcript_scrollbar_drag.is_some()
    }

    pub(crate) fn transcript_selection(&self) -> Option<TranscriptSelection> {
        self.transcript_view.transcript_selection
    }

    pub(crate) fn operator_sidebar_selection(&self) -> Option<OperatorSidebarSelection> {
        self.secondary_surfaces.selection
    }

    pub(crate) fn selected_operator_sidebar_keyboard_index(&self) -> Option<usize> {
        self.secondary_surfaces.keyboard_index
    }

    #[cfg(test)]
    pub(crate) fn selected_operator_sidebar_keyboard_index_for_test(&self) -> Option<usize> {
        self.secondary_surfaces.keyboard_index
    }

    fn toggle_operator_sidebar_section(&mut self, section: OperatorSidebarSection) {
        if !self.secondary_surfaces.collapsed_sections.insert(section) {
            self.secondary_surfaces.collapsed_sections.remove(&section);
        }
        self.details_scroll = 0;
    }

    fn toggle_operator_sidebar_subagent_group(&mut self, agent_name: String) {
        if !self
            .secondary_surfaces
            .expanded_subagent_groups
            .insert(agent_name.clone())
        {
            self.secondary_surfaces
                .expanded_subagent_groups
                .remove(&agent_name);
        }
        self.details_scroll = 0;
    }

    fn begin_transcript_scrollbar_drag(
        &mut self,
        scrollbar: TranscriptScrollbarHit,
        pointer_row: u16,
    ) {
        let pointer_offset_y = pointer_row
            .saturating_sub(scrollbar.thumb.y)
            .min(scrollbar.thumb.height.saturating_sub(1));
        self.transcript_view.transcript_scrollbar_drag = Some(TranscriptScrollbarDragState {
            track: scrollbar.track,
            thumb_height: scrollbar.thumb.height,
            pointer_offset_y,
            max_scroll: scrollbar.max_scroll,
        });
    }

    fn update_transcript_scrollbar_drag(&mut self, pointer_row: u16) {
        let Some(drag) = self.transcript_view.transcript_scrollbar_drag else {
            return;
        };

        let max_thumb_top = drag.track.height.saturating_sub(drag.thumb_height);
        let desired_thumb_top = pointer_row
            .saturating_sub(drag.pointer_offset_y)
            .clamp(drag.track.y, drag.track.y.saturating_add(max_thumb_top));
        let thumb_top = desired_thumb_top.saturating_sub(drag.track.y);
        let scroll_top = if drag.max_scroll == 0 || max_thumb_top == 0 {
            drag.max_scroll
        } else {
            ((usize::from(thumb_top) * drag.max_scroll) + usize::from(max_thumb_top) / 2)
                / usize::from(max_thumb_top)
        };
        self.set_transcript_scroll_from_top_with_max(
            scroll_top.min(drag.max_scroll),
            drag.max_scroll,
        );
    }

    fn set_transcript_scroll_from_top_with_max(&mut self, scroll_top: usize, max_scroll: usize) {
        let clamped = scroll_top.min(max_scroll);
        if max_scroll == 0 || clamped >= max_scroll {
            self.transcript_view.follow_mode = true;
            self.transcript_view.transcript_scroll = 0;
            return;
        }

        self.transcript_view.follow_mode = false;
        self.transcript_view.transcript_scroll = max_scroll.saturating_sub(clamped);
    }
}
