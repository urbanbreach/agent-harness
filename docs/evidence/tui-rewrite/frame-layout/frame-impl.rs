impl FrameLayoutPlan {
    pub fn for_app(app: &AppState, area: Rect) -> Self {
        let theme = app.theme();
        let heights = theme.token_families().live_shell.spacing.heights;
        let layout = theme.live_shell_layout(area.width, area.height);
        let session_contract = session_geometry_contract(area, layout);
        let startup = app.startup_shell_visible();
        let child = app.current_subagent_session_present();
        let child_footer = app.review_surface().is_none() && !startup && app.active_permission().is_none() && child;
        let header_height = if startup || (app.review_surface().is_none() && (!app.replay_mode || child)) { 0 } else { heights.header };
        let footer_height = if child_footer { 3 } else if startup { 2 } else if app.replay_mode { heights.footer } else { 0 };
        let [header, content, footer] = surfaces::split_rows(area, header_height, footer_height);
        let shell = if app.replay_mode && !child { content } else { centered_live_shell_area(content, layout) };
        let header_text = if app.replay_mode { header } else { Rect::new(shell.x, header.y, shell.width, header.height) };
        let text_height = heights.footer.min(footer.height);
        let text_y = if startup { footer.y } else { footer.y.saturating_add(footer.height.saturating_sub(text_height) / 2) };
        let footer_text = if app.replay_mode { footer } else { Rect::new(shell.x, text_y, shell.width, text_height) };
        let mut plan = Self {
            root: area, shell, header, header_text, content, live_anchor: None,
            transcript: None, todo: None, model_prompt_notice: None, terminal_panel: None,
            operator_sidebar: None, dock: None, status: None, composer: None, disclosure: None,
            footer, footer_text, details_overlay: None, palette_overlay: None, slash_overlay: None,
            wheel_hit_areas: WheelHitAreas::default(), session_contract,
        };
        session::project(app, &mut plan, layout, child_footer);
        match app.overlay_stack().top() {
            Some(OverlayKind::CommandPalette | OverlayKind::TogglesMenu | OverlayKind::LineageBrowser | OverlayKind::ForkSelector) => {
                plan.palette_overlay = command_palette_overlay_area(area, theme, layout, session_contract, app);
            }
            Some(OverlayKind::SlashCommands | OverlayKind::FileMentions) => {
                plan.slash_overlay = plan.composer.and_then(|composer| slash_command_overlay_area(composer, theme, session_contract, app));
            }
            _ => {}
        }
        if !app.replay_mode && !child && !app.activities.iter().any(|entry| entry.user_message.is_some()) {
            if let (Some(message), Some(transcript)) = (&app.model_prompt_notice, plan.transcript.as_mut()) {
                let width = transcript.width.saturating_sub(4);
                let rows = crate::ui::wrap_completion_text(message, usize::from(width)).len();
                let height = u16::try_from(rows).unwrap_or(u16::MAX).min(transcript.height.saturating_sub(1));
                plan.model_prompt_notice = Some(Rect::new(transcript.x.saturating_add(2), transcript.bottom().saturating_sub(height), width, height));
                transcript.height = transcript.height.saturating_sub(height);
                plan.wheel_hit_areas.transcript = Some(*transcript);
            }
        }
        plan
    }
}
