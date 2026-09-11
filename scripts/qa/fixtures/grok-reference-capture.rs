//! Local evidence driver. Calls the unmodified public reference renderer; performs no provider requests.
use std::{fs, path::Path, time::{Duration, Instant}};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
use xai_grok_pager::{app::{app_view::{AuthState, TrustState}, consent::ConsentState}, views::{welcome::{render_welcome, WelcomeRenderParams, WelcomePromptFocus}, prompt_widget::PromptWidget, picker::PickerState}};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("output directory required")?;
    fs::create_dir_all(&output)?;
    let auth_state = &AuthState::Done;
    let trust_state = &TrustState::Done;
    let mut params = WelcomeRenderParams {
            prompt_focus: WelcomePromptFocus::Unfocused,
            auth_state,
            trust_state,
            consent_state: &ConsentState::Done,
            consent_hover_link: None,
            login_label: None,
            auth_code_input: "",
            auth_code_cursor_byte: 0,
            clipboard_delivery: None,
            show_raw_url: false,
            announcement: None,
            tip: None,
            model_name: "test",
            flags: &[],
            selected: None,
            team_name: None,
            has_access: true,
            has_claude_import: false,
            mouse_pos: None,
            is_zdr_blocked: false,
            session_picker: None,
            session_picker_loading: false,
            compact: false,
            pending_hint: None,
            startup_warnings: &[],
            pending_update_version: None,
            foreign_resume_hint: None,
            is_api_key_auth: false,
            session_picker_content_results: None,
            session_picker_content_loading: false,
            session_picker_entries_query: None,
            welcome_tick: 0,
            gate: None,
            subscription_tier: None,
            session_picker_grouped: false,
            session_picker_source_filter: xai_grok_pager::views::session_picker::SourceFilter::default(),
            session_picker_pending_delete: false,
            chat_mode: false,
            cwd: std::path::Path::new("/repo"),
            credit_balance: None,
            auto_topup: None,
            usage_visible: true,
            changelog_bullets: &[],
            changelog_has_full_notes: false,
            welcome_announcement_expanded: false,
            upgrade_cta: None,
            privacy_banner: false,
        };
    let notes = vec!["Review changes and recorded tool results.".into(), "Continue sessions and manage agents.".into(), "Explore the command palette.".into()];
    params.changelog_bullets = &notes;
    params.changelog_has_full_notes = true;
    let epoch = Instant::now();
    let mut timings = Vec::new();
    for ms in [0u64, 100, 300, 1300, 4000] {
        if let Some(wait) = Duration::from_millis(ms).checked_sub(epoch.elapsed()) { std::thread::sleep(wait); }
        for (width,height) in [(80,24),(120,40),(160,50),(89,32),(90,32),(90,24),(120,32),(200,60)] {
            let mut bytes = Vec::new();
            let mut prompt = PromptWidget::new();
            let mut picker = PickerState::default();
            let mut terminal = Terminal::with_options(CrosstermBackend::new(&mut bytes), TerminalOptions {viewport: Viewport::Fixed(Rect::new(0,0,width,height))})?;
            terminal.draw(|frame| {render_welcome(frame.area(),frame.buffer_mut(),&params,&mut prompt,&mut picker);})?;
            drop(terminal);
            let name = format!("welcome-{width}x{height}-motion-{ms}ms.ansi");
            fs::write(Path::new(&output).join(&name),bytes)?;
            timings.push(serde_json::json!({"name":name,"target_ms":ms,"actual_elapsed_ms":epoch.elapsed().as_millis()}));
        }
    }
    fs::write(Path::new(&output).join("runtime-timing.json"),serde_json::to_vec_pretty(&timings)?)?;
    Ok(())
}
