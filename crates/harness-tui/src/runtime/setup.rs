use super::TuiMode;
use crate::{
    app::{AppState, UiIntent},
    runtime_live_updates::LiveUpdateReceiver,
    terminal::TerminalProfile,
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn app_for_mode(
    mode: TuiMode,
    exit_on_finish: bool,
    on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>>,
    keybindings: Option<&BTreeMap<String, String>>,
) -> (AppState, Option<LiveUpdateReceiver>) {
    match mode {
        TuiMode::Startup {
            session_history_entries,
            prompt_history_path,
            update_rx,
        } => {
            let mut app = AppState::new_startup_with_prompt_history_path(
                session_history_entries,
                on_ui_intent,
                prompt_history_path,
            );
            app.should_quit = exit_on_finish;
            if let Some(bindings) = keybindings {
                app.apply_keybindings(bindings.clone());
            }
            (app, Some(update_rx))
        }
        TuiMode::Replay { run_dir, events } => {
            let mut app = AppState::new_replay(run_dir, events);
            // Replay workspace authority comes exclusively from replayed RunStarted events.
            // The CWD-based workspace root provider must never substitute missing event authority.
            app.disable_cwd_workspace_root_provider();
            if let Some(on_ui_intent) = on_ui_intent {
                app.enable_replay_navigation_handoff(on_ui_intent);
            }
            if let Some(launch_metadata) = super::contracts::take_replay_metadata() {
                app.set_launch_metadata(launch_metadata);
            }
            if let Some(bindings) = keybindings {
                app.apply_keybindings(bindings.clone());
            }
            (app, None)
        }
        TuiMode::Live {
            run_dir,
            historical_events,
            session_history_entries,
            prompt_history_path,
            update_rx,
            compact_session_supported,
        } => {
            let crash_report = harness_core::crash_recovery::inspect_previous_crash(&run_dir);
            let starting_session_seed = historical_events.is_empty();
            let mut app = AppState::new_live_with_session_history_and_prompt_history_path(
                Some(run_dir.clone()),
                exit_on_finish,
                on_ui_intent,
                session_history_entries,
                prompt_history_path,
            );
            app.set_starting_session_seed(
                starting_session_seed && app.composer.prompt_buffer.is_empty(),
            );
            app.set_compact_session_supported(compact_session_supported);
            if let Some(launch_metadata) = super::contracts::take_replay_metadata() {
                app.set_launch_metadata(launch_metadata);
            }
            if let Some(bindings) = keybindings {
                app.apply_keybindings(bindings.clone());
            }
            for event in historical_events {
                app.ingest_historical_event(event);
            }
            // History can supply parent lineage when session metadata is unavailable.
            app.load_session_lineage();
            if let Some(message) = crash_report.recovery_message {
                let banner = match crash_report.recovery_action {
                    Some(action) => {
                        let run_id = run_dir
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("session");
                        format!("{message} Action: {}", action.operator_hint(run_id))
                    }
                    None => message,
                };
                app.set_status_banner(Some(banner));
            }
            (app, Some(update_rx))
        }
    }
}
pub(super) fn configure(app: &mut AppState, profile: &TerminalProfile) -> bool {
    app.shortcuts_ctrl_dot = profile.context.brand.supports_enhanced_keyboard()
        && !profile.context.ctrl_dot_unreliable()
        && !profile.context.multiplexer.intercepts_csi_queries();
    let ssh = ["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"]
        .iter()
        .any(|key| std::env::var_os(key).is_some());
    apply_startup_capability_notice(
        app,
        crate::terminal::startup_diagnostics::clipboard_warning_required(profile.context, ssh),
    );
    app.set_color_level(crate::theme::detect_color_level(
        std::env::var("NO_COLOR").ok().as_deref(),
        std::env::var("COLORTERM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    ));
    app.set_glyph_mode(profile.matrix.classified_by().glyph_mode());
    let reduced = std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some()
        || std::env::var("HARNESS_TUI_REDUCED_MOTION").is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });
    app.set_reduced_motion(reduced);
    reduced
}

pub(crate) fn apply_startup_capability_notice(app: &mut AppState, warning: bool) {
    if warning && app.startup_shell_visible() && app.status_banner.is_none() {
        app.set_status_banner(Some("Clipboard may be unreachable.".into()));
    }
}
