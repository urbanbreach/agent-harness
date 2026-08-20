// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::permission_prompt::{PermissionPointerDown, PermissionPointerTarget};
use super::*;
use ratatui::text::Line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PermissionPromptHitRegion {
    target: PermissionPointerTarget,
    area: Rect,
}

fn permission_prompt_hit_regions(
    app: &AppState,
    frame_area: Rect,
) -> Vec<PermissionPromptHitRegion> {
    let Some(permission) = app.active_permission_view() else {
        return Vec::new();
    };
    let Some(status_area) = crate::layout::FrameLayoutPlan::for_app(app, frame_area).status else {
        return Vec::new();
    };

    if permission.question_prompts.is_some() {
        question_prompt_hit_regions(app, status_area, &permission)
    } else {
        permission_choice_hit_regions(app, status_area, &permission)
    }
}

fn permission_choice_hit_regions(
    app: &AppState,
    status_area: Rect,
    permission: &ActivePermissionView,
) -> Vec<PermissionPromptHitRegion> {
    let measure = crate::layout::permission_dock_measure(
        app,
        status_area.width,
        status_area.height,
        permission,
    );
    let tray = crate::layout::permission_dock_geometry(status_area, measure).options;
    if tray.width == 0 || tray.height == 0 {
        return Vec::new();
    }

    match app.permission_modal_stage(&permission.permission_id) {
        PermissionModalStage::Decision => [
            PermissionModalSelection::AllowAlways,
            PermissionModalSelection::AllowSession,
            PermissionModalSelection::AllowOnce,
            PermissionModalSelection::Reject,
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(index, selection)| {
            let y = tray
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            (y < tray.bottom()).then_some(PermissionPromptHitRegion {
                target: PermissionPointerTarget::Decision(selection),
                area: Rect::new(tray.x, y, tray.width, 1),
            })
        })
        .collect(),
        PermissionModalStage::AlwaysConfirm => {
            let mut x = tray.x;
            [
                (PermissionConfirmSelection::Confirm, "Confirm"),
                (PermissionConfirmSelection::Cancel, "Cancel"),
            ]
            .into_iter()
            .filter_map(|(selection, label)| {
                let width = u16::try_from(Line::from(format!(" ● {label} ")).width())
                    .unwrap_or(u16::MAX)
                    .min(tray.right().saturating_sub(x));
                let region = (width > 0).then_some(PermissionPromptHitRegion {
                    target: PermissionPointerTarget::Confirm(selection),
                    area: Rect::new(x, tray.y, width, 1),
                });
                x = x.saturating_add(width).saturating_add(1);
                region
            })
            .collect()
        }
    }
}

fn question_prompt_hit_regions(
    app: &AppState,
    status_area: Rect,
    permission: &ActivePermissionView,
) -> Vec<PermissionPromptHitRegion> {
    let Some(prompts) = permission.question_prompts.as_deref() else {
        return Vec::new();
    };
    if prompts.is_empty() || app.question_prompt_editing(&permission.permission_id) {
        return Vec::new();
    }

    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len());
    if tab >= prompts.len() {
        return Vec::new();
    }
    let Some(prompt) = prompts.get(tab.min(prompts.len().saturating_sub(1))) else {
        return Vec::new();
    };
    let dock_area = if status_area.height > crate::layout::QUESTION_OUTER_FOOTER_ROWS {
        Rect::new(
            status_area.x,
            status_area.y,
            status_area.width,
            status_area
                .height
                .saturating_sub(crate::layout::QUESTION_OUTER_FOOTER_ROWS),
        )
    } else {
        status_area
    };
    let measure = crate::layout::question_dock_measure(
        app,
        status_area.width,
        status_area.height,
        permission,
    );
    let geometry = crate::layout::question_dock_geometry(dock_area, &measure);
    let visible_bottom = measure
        .scroll_offset
        .saturating_add(geometry.options.height);
    let mut regions = measure
        .option_ranges
        .into_iter()
        .filter_map(|range| {
            let visible_start = range.start.max(measure.scroll_offset);
            let visible_end = range.end.min(visible_bottom);
            (visible_start < visible_end).then_some(PermissionPromptHitRegion {
                target: PermissionPointerTarget::QuestionChoice(range.index),
                area: Rect::new(
                    geometry.options.x,
                    geometry
                        .options
                        .y
                        .saturating_add(visible_start.saturating_sub(measure.scroll_offset)),
                    geometry.options.width,
                    visible_end.saturating_sub(visible_start),
                ),
            })
        })
        .collect::<Vec<_>>();
    if prompt.custom && geometry.sticky.height > 0 {
        regions.push(PermissionPromptHitRegion {
            target: PermissionPointerTarget::QuestionChoice(prompt.options.len()),
            area: Rect::new(
                geometry.sticky.x,
                geometry.sticky.y,
                geometry.sticky.width,
                1,
            ),
        });
    }
    if let Some(scrollbar) = geometry.scrollbar {
        regions.insert(
            0,
            PermissionPromptHitRegion {
                target: PermissionPointerTarget::QuestionScrollbar,
                area: scrollbar,
            },
        );
    }
    regions
}

impl AppState {
    pub(in crate::app) fn handle_connect_dialog_mouse(
        &mut self,
        mouse: MouseEvent,
        frame_area: Rect,
    ) -> bool {
        if !self.connect_dialog.visible
            || self.connect_dialog.step != auth_dialog::ConnectDialogStep::Waiting
        {
            return false;
        }
        if mouse.modifiers.contains(KeyModifiers::CONTROL) {
            self.connect_dialog.pointer_down = None;
            self.connect_dialog.pointer_dragged = false;
            return false;
        }
        let target = crate::ui::ui_overlays::waiting_authorization_detail_at(
            &self.connect_dialog,
            frame_area,
            mouse.column,
            mouse.row,
        );
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.connect_dialog.pointer_down = target;
                self.connect_dialog.pointer_dragged = false;
                target.is_some()
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.connect_dialog.pointer_dragged |= self.connect_dialog.pointer_down.is_some();
                self.connect_dialog.pointer_down.is_some()
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(pointer_down) = self.connect_dialog.pointer_down.take() else {
                    return false;
                };
                let dragged = std::mem::take(&mut self.connect_dialog.pointer_dragged);
                if dragged {
                    match pointer_down {
                        auth_dialog::AuthorizationDetail::Url => {
                            self.copy_connect_authorization_url()
                        }
                        auth_dialog::AuthorizationDetail::Code => {
                            self.copy_connect_authorization_code()
                        }
                    }
                } else if target == Some(pointer_down) {
                    match pointer_down {
                        auth_dialog::AuthorizationDetail::Url => {
                            self.open_connect_authorization_url()
                        }
                        auth_dialog::AuthorizationDetail::Code => {
                            self.copy_connect_authorization_code()
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn handle_composer_mouse_event(
        &mut self,
        mouse: MouseEvent,
        frame_area: Rect,
    ) -> bool {
        if !matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left)
        ) {
            return false;
        }
        let actual_composer = crate::layout::FrameLayoutPlan::for_app(self, frame_area).composer;
        if !actual_composer.is_some_and(|area| rect_contains(area, mouse.column, mouse.row)) {
            return false;
        }
        self.focus = Focus::Prompt;
        true
    }

    pub(in crate::app) fn set_transcript_selection(
        &mut self,
        anchor: TranscriptSelectionCell,
        focus: TranscriptSelectionCell,
    ) {
        self.transcript_view.transcript_selection = Some(TranscriptSelection { anchor, focus });
        self.transcript_view.transcript_selection_anchors.set(None);
    }

    pub(in crate::app) fn clear_transcript_selection(&mut self) {
        self.transcript_view.transcript_selection = None;
        self.transcript_view.transcript_selection_anchors.set(None);
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
        let Some(text) = ui::transcript_selection_patch_text(self, frame_area, selection)
            .or_else(|| ui::transcript_selection_text(self, frame_area, selection))
        else {
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
        if self
            .last_frame_area
            .is_some_and(|previous| previous != area)
        {
            self.transcript_view.transcript_scrollbar_drag = None;
            self.transcript_view.hovered_transcript_target = None;
            self.transcript_view.transcript_selection_dragging = false;
            self.hovered_subagent_footer_target = None;
            self.hovered_live_turn_stop = false;
            self.hovered_live_turn_background = false;
            self.pending_subagent_footer_target = None;
            self.secondary_surfaces.selection_dragging = false;
            self.secondary_surfaces.pending_click = None;
            self.modal_interaction.invalidate();
        }
        self.last_frame_area = Some(area);
        if let Some(dashboard) = self.dashboard.as_mut() {
            let viewport = crate::dashboard_integration::dashboard_viewport(area).unwrap_or(area);
            if let Err(error) = dashboard.resize(viewport) {
                self.status_banner = Some(error.to_string());
            }
        }
        let transcript_area = crate::layout::FrameLayoutPlan::for_app(self, area)
            .transcript
            .unwrap_or(area);
        if let Some(composite) = self.transcript_integration.as_mut() {
            let _ = composite.resize(transcript_area);
        }
    }

    pub(crate) fn last_frame_area(&self) -> Option<Rect> {
        self.last_frame_area
    }

    fn handle_welcome_mouse(&mut self, frame_area: Rect, mouse: MouseEvent) -> bool {
        let startup_area = crate::layout::FrameLayoutPlan::for_app(self, frame_area)
            .transcript
            .unwrap_or(frame_area);
        let Some(hit) = self
            .welcome_hit_map(startup_area)
            .hit(mouse.column, mouse.row)
        else {
            return false;
        };
        match hit.region {
            crate::welcome_surface::WelcomeRegion::Prompt => {
                self.welcome
                    .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
                self.focus = Focus::Prompt;
            }
            crate::welcome_surface::WelcomeRegion::Menu => {
                if let Some(index) = hit.item_index {
                    let was_expanded = self.startup_welcome_expanded();
                    self.welcome.set_hovered_action(Some(index));
                    self.welcome.focus_menu_item(index);
                    if self.welcome.selected_action()
                        == Some(crate::welcome_surface::WelcomeAction::Changelog)
                    {
                        self.welcome
                            .handle(crate::welcome_surface::WelcomeInput::FocusPrompt);
                        self.focus = Focus::Prompt;
                        if was_expanded {
                            self.open_release_notes();
                        } else {
                            self.expand_startup_changelog();
                        }
                    } else {
                        self.execute_startup_launcher_action();
                    }
                } else {
                    self.welcome
                        .handle(crate::welcome_surface::WelcomeInput::FocusMenu);
                }
                if matches!(
                    self.welcome.focus(),
                    crate::welcome_surface::WelcomeFocus::Menu(_)
                ) {
                    self.focus = Focus::List;
                }
            }
            crate::welcome_surface::WelcomeRegion::StatusBar
            | crate::welcome_surface::WelcomeRegion::Hero
            | crate::welcome_surface::WelcomeRegion::Logo
            | crate::welcome_surface::WelcomeRegion::None => return false,
        }
        true
    }

    fn handle_welcome_pointer_completion(&mut self, mouse: MouseEvent) -> bool {
        match mouse.kind {
            MouseEventKind::Up(MouseButton::Left) => {
                self.welcome.take_pointer_press();
                false
            }
            MouseEventKind::Drag(MouseButton::Left) => self.welcome.cancel_pointer_press(),
            _ => false,
        }
    }

    fn clear_blocked_pointer_state(&mut self) -> bool {
        let changed = self.transcript_view.transcript_scrollbar_drag.is_some()
            || self.transcript_view.hovered_transcript_target.is_some()
            || self.hovered_subagent_footer_target.is_some()
            || self.hovered_live_turn_stop
            || self.hovered_live_turn_background
            || self.transcript_view.transcript_selection.is_some()
            || self.secondary_surfaces.selection.is_some();
        self.transcript_view.transcript_scrollbar_drag = None;
        self.transcript_view.hovered_transcript_target = None;
        self.hovered_subagent_footer_target = None;
        self.hovered_live_turn_stop = false;
        self.hovered_live_turn_background = false;
        self.pending_subagent_footer_target = None;
        self.clear_transcript_selection();
        self.clear_operator_sidebar_selection();
        changed
    }

    fn select_permission_pointer_target(
        &mut self,
        permission: &ActivePermissionView,
        target: PermissionPointerTarget,
    ) -> bool {
        match target {
            PermissionPointerTarget::Decision(selection)
                if permission.question_prompts.is_none()
                    && self.permission_modal_stage(&permission.permission_id)
                        == PermissionModalStage::Decision =>
            {
                self.permission_prompt.permission_id = Some(permission.permission_id.clone());
                self.permission_prompt.stage = PermissionModalStage::Decision;
                self.permission_prompt.selection = selection;
                true
            }
            PermissionPointerTarget::Confirm(selection)
                if permission.question_prompts.is_none()
                    && self.permission_modal_stage(&permission.permission_id)
                        == PermissionModalStage::AlwaysConfirm =>
            {
                self.permission_prompt.permission_id = Some(permission.permission_id.clone());
                self.permission_prompt.stage = PermissionModalStage::AlwaysConfirm;
                self.permission_prompt.confirm_selection = selection;
                true
            }
            PermissionPointerTarget::QuestionChoice(index) => {
                let Some(prompts) = permission.question_prompts.as_deref() else {
                    return false;
                };
                let tab = self
                    .question_prompt_tab(&permission.permission_id)
                    .min(prompts.len());
                let Some(prompt) = prompts.get(tab.min(prompts.len().saturating_sub(1))) else {
                    return false;
                };
                let choice_count = prompt
                    .options
                    .len()
                    .saturating_add(usize::from(prompt.custom));
                if index >= choice_count || self.question_prompt_editing(&permission.permission_id)
                {
                    return false;
                }
                if self.question_prompt.permission_id.as_deref()
                    != Some(permission.permission_id.as_str())
                {
                    self.handle_permission_modal_key(KeyEvent::new(
                        KeyCode::Null,
                        KeyModifiers::NONE,
                    ));
                }
                if self.question_prompt.permission_id.as_deref()
                    != Some(permission.permission_id.as_str())
                {
                    return false;
                }
                self.question_prompt.hovered = Some(index);
                self.question_prompt.selection = index;
                true
            }
            PermissionPointerTarget::Decision(_)
            | PermissionPointerTarget::Confirm(_)
            | PermissionPointerTarget::QuestionScrollbar => false,
        }
    }

    fn scroll_question_prompt(
        &mut self,
        frame_area: Rect,
        pointer_row: Option<u16>,
        delta: i16,
    ) -> bool {
        let Some(permission) = self.active_permission_view() else {
            return false;
        };
        let Some(status_area) = crate::layout::FrameLayoutPlan::for_app(self, frame_area).status
        else {
            return false;
        };
        let measure = crate::layout::question_dock_measure(
            self,
            status_area.width,
            status_area.height,
            &permission,
        );
        let dock_area = if status_area.height > crate::layout::QUESTION_OUTER_FOOTER_ROWS {
            Rect::new(
                status_area.x,
                status_area.y,
                status_area.width,
                status_area
                    .height
                    .saturating_sub(crate::layout::QUESTION_OUTER_FOOTER_ROWS),
            )
        } else {
            status_area
        };
        let geometry = crate::layout::question_dock_geometry(dock_area, &measure);
        if measure.max_scroll == 0 {
            return false;
        }
        if self.question_prompt.permission_id.as_deref() != Some(permission.permission_id.as_str())
        {
            self.handle_permission_modal_key(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE));
        }
        let tab = self.question_prompt_tab(&permission.permission_id);
        let next = pointer_row.map_or_else(
            || {
                let bounded = i32::from(measure.scroll_offset)
                    .saturating_add(i32::from(delta))
                    .clamp(0, i32::from(measure.max_scroll));
                u16::try_from(bounded).unwrap_or(measure.max_scroll)
            },
            |row| {
                let track_height = geometry.options.height.max(1);
                let relative = row
                    .saturating_sub(geometry.options.y)
                    .min(track_height.saturating_sub(1));
                relative
                    .saturating_mul(measure.max_scroll)
                    .checked_div(track_height.saturating_sub(1).max(1))
                    .unwrap_or(0)
            },
        );
        let Some(scroll) = self.question_prompt.scroll_offsets.get_mut(tab) else {
            return false;
        };
        let changed = *scroll != next;
        *scroll = next;
        changed
    }

    fn handle_permission_prompt_mouse(&mut self, mouse: MouseEvent, frame_area: Rect) -> bool {
        if mouse.modifiers.contains(KeyModifiers::CONTROL) {
            self.permission_prompt.pointer_down = None;
            return false;
        }

        match mouse.kind {
            MouseEventKind::Moved => {
                let hovered = permission_prompt_hit_regions(self, frame_area)
                    .into_iter()
                    .find(|region| rect_contains(region.area, mouse.column, mouse.row))
                    .and_then(|region| match region.target {
                        PermissionPointerTarget::QuestionChoice(index) => Some(index),
                        PermissionPointerTarget::Decision(_)
                        | PermissionPointerTarget::Confirm(_)
                        | PermissionPointerTarget::QuestionScrollbar => None,
                    });
                let active_question_id = self.active_permission_view().and_then(|permission| {
                    permission
                        .question_prompts
                        .is_some()
                        .then_some(permission.permission_id)
                });
                if hovered.is_some()
                    && self.question_prompt.permission_id.as_deref()
                        != active_question_id.as_deref()
                {
                    self.handle_permission_modal_key(KeyEvent::new(
                        KeyCode::Null,
                        KeyModifiers::NONE,
                    ));
                }
                let changed = self.question_prompt.hovered != hovered;
                self.question_prompt.hovered = hovered;
                changed
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let region = permission_prompt_hit_regions(self, frame_area)
                    .into_iter()
                    .find(|region| rect_contains(region.area, mouse.column, mouse.row));
                let Some(region) = region else {
                    self.permission_prompt.pointer_down = None;
                    return false;
                };
                let Some(permission) = self.active_permission_view() else {
                    self.permission_prompt.pointer_down = None;
                    return false;
                };
                if region.target == PermissionPointerTarget::QuestionScrollbar {
                    self.scroll_question_prompt(frame_area, Some(mouse.row), 0);
                    self.permission_prompt.pointer_down = Some(PermissionPointerDown {
                        permission_id: permission.permission_id,
                        target: region.target,
                        area: region.area,
                    });
                    return true;
                }
                if !self.select_permission_pointer_target(&permission, region.target) {
                    self.permission_prompt.pointer_down = None;
                    return false;
                }
                self.permission_prompt.pointer_down = Some(PermissionPointerDown {
                    permission_id: permission.permission_id,
                    target: region.target,
                    area: region.area,
                });
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(pointer_down) = self.permission_prompt.pointer_down.as_ref() else {
                    return false;
                };
                if pointer_down.target == PermissionPointerTarget::QuestionScrollbar {
                    return self.scroll_question_prompt(frame_area, Some(mouse.row), 0);
                }
                let remains_inside = rect_contains(pointer_down.area, mouse.column, mouse.row)
                    && self.active_permission_view().is_some_and(|permission| {
                        permission.permission_id == pointer_down.permission_id
                    });
                if !remains_inside {
                    self.permission_prompt.pointer_down = None;
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(pointer_down) = self.permission_prompt.pointer_down.take() else {
                    return false;
                };
                if pointer_down.target == PermissionPointerTarget::QuestionScrollbar {
                    self.scroll_question_prompt(frame_area, Some(mouse.row), 0);
                    return true;
                }
                if !rect_contains(pointer_down.area, mouse.column, mouse.row) {
                    return true;
                }
                let Some(permission) = self.active_permission_view() else {
                    return true;
                };
                if permission.permission_id != pointer_down.permission_id
                    || !self.select_permission_pointer_target(&permission, pointer_down.target)
                {
                    return true;
                }
                self.handle_permission_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                true
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let regions = permission_prompt_hit_regions(self, frame_area);
                let inside = regions.iter().any(|region| {
                    matches!(
                        region.target,
                        PermissionPointerTarget::QuestionChoice(_)
                            | PermissionPointerTarget::QuestionScrollbar
                    ) && rect_contains(region.area, mouse.column, mouse.row)
                });
                if !inside {
                    return false;
                }
                let delta = if mouse.kind == MouseEventKind::ScrollDown {
                    1
                } else {
                    -1
                };
                self.scroll_question_prompt(frame_area, None, delta)
            }
            _ => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn permission_prompt_hit_regions_for_test(
        &self,
        frame_area: Rect,
    ) -> Vec<(PermissionPointerTarget, Rect)> {
        permission_prompt_hit_regions(self, frame_area)
            .into_iter()
            .map(|region| (region.target, region.area))
            .collect()
    }

    pub(crate) fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        frame_area: Rect,
        hovered_wheel_target: Option<WheelTarget>,
        clicked_operator_sidebar_section: Option<OperatorSidebarSection>,
        transcript_scrollbar_hit: Option<TranscriptScrollbarHit>,
    ) -> bool {
        if let Some(changed) = self.handle_top_modal_mouse(mouse, frame_area) {
            self.welcome.take_pointer_press();
            let cleared = self.clear_blocked_pointer_state();
            return changed || cleared;
        }
        if self.handle_connect_dialog_mouse(mouse, frame_area) {
            return true;
        }
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

        if self.overlay_stack().top() == Some(OverlayKind::StatusDialog) {
            return self.handle_status_dashboard_mouse(mouse);
        }

        if self.overlay_stack().top() == Some(OverlayKind::PermissionModal) {
            let handled = self.handle_permission_prompt_mouse(mouse, frame_area);
            let cleared = self.clear_blocked_pointer_state();
            return handled || cleared;
        }

        if self.overlay_stack().blocks_pointer_interaction() {
            return self.clear_blocked_pointer_state();
        }

        self.set_frame_area(frame_area);

        if self.startup_shell_visible() && self.handle_welcome_pointer_completion(mouse) {
            return true;
        }

        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && ui::live_turn_watching_rect(self, frame_area)
                .is_some_and(|area| rect_contains(area, mouse.column, mouse.row))
        {
            self.open_status_dashboard_at(frame_area);
            return true;
        }

        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && ui::live_turn_background_rect(self, frame_area)
                .is_some_and(|area| rect_contains(area, mouse.column, mouse.row))
        {
            self.hovered_live_turn_background = true;
            return self.background_live_turn_child();
        }

        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && ui::live_turn_stop_rect(self, frame_area)
                .is_some_and(|area| rect_contains(area, mouse.column, mouse.row))
        {
            self.hovered_live_turn_stop = true;
            return self.interrupt_active_turn();
        }

        if self.handle_composer_mouse_event(mouse, frame_area) {
            return true;
        }

        if self.startup_shell_visible()
            && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self.handle_welcome_mouse(frame_area, mouse)
        {
            return true;
        }

        match mouse.kind {
            MouseEventKind::Moved => {
                let hovered_welcome_action = self
                    .startup_shell_visible()
                    .then(|| {
                        let startup_area =
                            crate::layout::FrameLayoutPlan::for_app(self, frame_area)
                                .transcript
                                .unwrap_or(frame_area);
                        self.welcome_hit_map(startup_area)
                            .hit(mouse.column, mouse.row)
                            .and_then(|hit| hit.item_index)
                    })
                    .flatten();
                let welcome_hover_changed = self.welcome.set_hovered_action(hovered_welcome_action);
                let hovered_live_turn_stop = ui::live_turn_stop_rect(self, frame_area)
                    .is_some_and(|area| rect_contains(area, mouse.column, mouse.row));
                let hovered_live_turn_background = ui::live_turn_background_rect(self, frame_area)
                    .is_some_and(|area| rect_contains(area, mouse.column, mouse.row));
                let hovered_subagent_footer_target =
                    ui::subagent_footer_target_at(self, frame_area, mouse.column, mouse.row);
                let hovered_transcript_target = if hovered_subagent_footer_target.is_none() {
                    ui::transcript_mouse_target(self, frame_area, mouse.column, mouse.row)
                } else {
                    None
                };
                let changed = welcome_hover_changed
                    || self.transcript_view.hovered_transcript_target != hovered_transcript_target
                    || self.hovered_subagent_footer_target != hovered_subagent_footer_target
                    || self.hovered_live_turn_stop != hovered_live_turn_stop
                    || self.hovered_live_turn_background != hovered_live_turn_background;
                self.transcript_view.hovered_transcript_target = hovered_transcript_target;
                self.hovered_subagent_footer_target = hovered_subagent_footer_target;
                self.hovered_live_turn_stop = hovered_live_turn_stop;
                self.hovered_live_turn_background = hovered_live_turn_background;
                changed
            }
            MouseEventKind::Down(MouseButton::Right) => {
                let copied = self.copy_active_selection(frame_area);
                self.clear_transcript_selection();
                self.clear_operator_sidebar_selection();
                copied
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.transcript_view.transcript_click_activated_on_down = false;
                if ui::transcript_return_to_live_hit(self, frame_area, mouse.column, mouse.row) {
                    self.transcript_view.transcript_click_activated_on_down = true;
                    self.scroll_goto_bottom();
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
                    return true;
                }
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

                if let Some(turn_id) =
                    ui::transcript_timeline_turn_at(self, frame_area, mouse.column, mouse.row)
                {
                    self.select_transcript_turn(turn_id);
                    self.clear_transcript_selection();
                    self.clear_operator_sidebar_selection();
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
                let hover_changed = self.transcript_view.hovered_transcript_target.is_some()
                    || self.hovered_subagent_footer_target.is_some()
                    || self.hovered_live_turn_stop
                    || self.hovered_live_turn_background;
                self.transcript_view.hovered_transcript_target = None;
                self.hovered_subagent_footer_target = None;
                self.hovered_live_turn_stop = false;
                self.hovered_live_turn_background = false;
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
                    hover_changed
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
                let transcript_selection_was_dragging =
                    self.transcript_view.transcript_selection_dragging;
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
                if transcript_selection_was_dragging {
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
        self.release_transcript_page_flip();
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
}
