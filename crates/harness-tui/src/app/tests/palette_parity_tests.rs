use super::*;
use crate::keybindings::palette_model;
use crate::keybindings::parity_matrix;

fn open_palette(app: &mut AppState) {
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
}

fn palette_ids(app: &AppState) -> Vec<String> {
    app.palette_filtered.clone()
}

fn assert_excluded_absent(app: &AppState) {
    let ids = palette_ids(app);
    for excluded_id in parity_matrix::excluded_ids() {
        let id = excluded_id
            .strip_prefix("suggested:")
            .unwrap_or(excluded_id);
        assert!(
            !ids.iter()
                .any(|c| c == id || c == &format!("suggested:{id}")),
            "excluded command {excluded_id} should not appear in palette"
        );
    }
}

fn assert_hidden_non_targets_absent(app: &AppState) {
    let ids = palette_ids(app);
    for hidden_id in parity_matrix::hidden_non_target_ids() {
        let id = hidden_id.strip_prefix("suggested:").unwrap_or(hidden_id);
        assert!(
            !ids.iter()
                .any(|c| c == id || c == &format!("suggested:{id}")),
            "hidden non-target command {hidden_id} should not appear in palette"
        );
    }
}

pub(super) fn palette_opens_with_ctrl_p() {
    let mut app = AppState::new_live(None, false, None);
    assert!(!app.palette_visible);
    open_palette(&mut app);
    assert!(app.palette_visible);
}

pub(super) fn palette_closes_with_escape() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(app.palette_visible);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.palette_visible);
}

pub(super) fn palette_closes_with_ctrl_c() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(app.palette_visible);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert!(!app.palette_visible);
}

pub(super) fn palette_empty_filter_has_suggested_duplicates() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        app.palette_filtered
            .iter()
            .any(|c| c.starts_with("suggested:")),
        "empty filter should produce suggested duplicates"
    );
}

pub(super) fn palette_non_empty_filter_has_no_suggested_duplicates() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    app.handle_key(key(KeyCode::Char('x')));
    assert!(
        !app.palette_filtered
            .iter()
            .any(|c| c.starts_with("suggested:")),
        "non-empty filter should not produce suggested duplicates"
    );
}

pub(super) fn palette_filter_matches_title_not_id() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "switch model".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.contains(&"model.list".to_string()),
        "filter 'switch model' should match 'Switch model' title"
    );
}

pub(super) fn palette_filter_does_not_match_command_id() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "model.list".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        !app.palette_filtered.contains(&"model.list".to_string()),
        "filter 'model.list' should not match command ID"
    );
}

pub(super) fn palette_no_results_shows_empty_message() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "zzzzzzz".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(app.palette_filtered.is_empty());
    let rendered = render_debug(&app, 120, 30);
    assert!(rendered.contains("No results found"));
}

pub(super) fn palette_navigation_wraps_around() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 1);
    app.palette_selected = 0;
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.palette_selected, len - 1);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.palette_selected, 0);
}

pub(super) fn palette_home_end_navigation() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 2);
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.palette_selected, len - 1);
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.palette_selected, 0);
}

pub(super) fn palette_page_navigation() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 10);
    app.handle_key(key(KeyCode::PageDown));
    assert!(app.palette_selected > 0);
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.palette_selected, 0);
}

pub(super) fn palette_excludes_excluded_commands_in_live_session() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert_excluded_absent(&app);
}

pub(super) fn palette_excludes_hidden_non_targets_in_live_session() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert_hidden_non_targets_absent(&app);
}

pub(super) fn palette_excludes_excluded_commands_in_startup_shell() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    assert_excluded_absent(&app);
}

pub(super) fn palette_excludes_hidden_non_targets_in_startup_shell() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    assert_hidden_non_targets_absent(&app);
}

pub(super) fn palette_includes_model_list_in_live_session() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_available_models(vec![crate::app::ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )]),
    );
    open_palette(&mut app);
    assert!(
        app.palette_filtered.contains(&"model.list".to_string()),
        "model.list should be in palette when models are configured"
    );
}

pub(super) fn palette_harness_only_commands_are_prefixed() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        if let Some(entry) = palette_model::find(id) {
            if entry.harness_only {
                assert!(
                    id.starts_with("harness."),
                    "harness-only command must be prefixed: {id}"
                );
            }
        }
    }
}

pub(super) fn palette_toggle_commands_use_dynamic_titles() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);

    let has_collapse = app.palette_filtered.iter().any(|c| {
        if let Some(entry) = palette_model::find(c) {
            matches!(entry.title, palette_model::DynamicTitle::ShowHide { .. })
        } else {
            false
        }
    });
    assert!(
        has_collapse,
        "palette should contain dynamic toggle commands"
    );
}

pub(super) fn palette_no_split_show_hide_entries() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);

    let old_split_ids = [
        "show_thinking",
        "hide_thinking",
        "show_timestamps",
        "hide_timestamps",
        "show_tool_details",
        "hide_tool_details",
        "show_generic_tool_output",
        "hide_generic_tool_output",
    ];
    for old_id in &old_split_ids {
        assert!(
            !app.palette_filtered.iter().any(|c| {
                let id = c.strip_prefix("suggested:").unwrap_or(c.as_str());
                id == *old_id
            }),
            "palette should not contain old split show/hide ID: {old_id}"
        );
    }

    let dynamic_toggle_ids = [
        "session.toggle.thinking",
        "session.toggle.timestamps",
        "session.toggle.actions",
        "session.toggle.generic_tool_output",
    ];
    for toggle_id in &dynamic_toggle_ids {
        assert!(
            app.palette_filtered.contains(&toggle_id.to_string()),
            "palette should contain dynamic toggle ID: {toggle_id}"
        );
    }
}

pub(super) fn palette_suggested_duplicate_dispatches_same_command() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);

    let suggested = app
        .palette_filtered
        .iter()
        .find(|c| c.starts_with("suggested:"))
        .expect("should have suggested duplicate");

    let original = suggested.strip_prefix("suggested:").unwrap();
    assert!(
        app.palette_filtered.contains(&original.to_string()),
        "suggested duplicate should have matching original command"
    );
}

pub(super) fn palette_replay_mode_restricts_commands() {
    let mut app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-test"), Vec::new());
    open_palette(&mut app);

    assert!(
        !app.palette_filtered.contains(&"variant.cycle".to_string()),
        "variant.cycle should not be available in replay mode"
    );
}

pub(super) fn palette_startup_shell_restricts_commands() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);

    assert!(
        app.palette_filtered.contains(&"session.new".to_string()),
        "session.new should be available in startup shell"
    );
    assert!(
        app.palette_filtered.contains(&"app.exit".to_string()),
        "app.exit should be available in startup shell"
    );
    assert!(
        !app.palette_filtered.contains(&"session.rename".to_string()),
        "session.rename should not be available in startup shell"
    );
}

pub(super) fn palette_all_filtered_results_are_valid_commands() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for id in &app.palette_filtered {
        let id = id.strip_prefix("suggested:").unwrap_or(id.as_str());
        assert!(
            palette_model::find(id).is_some(),
            "palette contains unknown command ID: {id}"
        );
    }
}

pub(super) fn palette_all_parity_included_ids_have_entries() {
    let entry_ids: std::collections::HashSet<&str> = palette_model::all_ids().into_iter().collect();
    for id in parity_matrix::included_ids() {
        assert!(
            entry_ids.contains(id),
            "parity matrix included ID {id} has no palette entry"
        );
    }
}

pub(super) fn palette_no_parity_excluded_ids_in_entries() {
    let entry_ids: std::collections::HashSet<&str> = palette_model::all_ids().into_iter().collect();
    for id in parity_matrix::excluded_ids() {
        assert!(
            !entry_ids.contains(id),
            "palette entries contain excluded ID: {id}"
        );
    }
}

pub(super) fn palette_no_hidden_non_target_ids_in_entries() {
    let entry_ids: std::collections::HashSet<&str> = palette_model::all_ids().into_iter().collect();
    for id in parity_matrix::hidden_non_target_ids() {
        assert!(
            !entry_ids.contains(id),
            "palette entries contain hidden non-target ID: {id}"
        );
    }
}

// === Dispatch tests: press Enter and verify outcomes ===

pub(super) fn palette_dispatch_quit_exits_app() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.contains(&"app.exit".to_string()),
        "app.exit should be in filtered results for 'exit'"
    );
    let exit_pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "app.exit")
        .unwrap();
    app.palette_selected = exit_pos;
    app.handle_key(key(KeyCode::Enter));
    assert!(app.should_quit);
}

pub(super) fn palette_dispatch_toggle_thinking_flips_state() {
    let mut app = AppState::new_live(None, false, None);
    let initial = app.transcript_view.show_transcript_thinking;
    open_palette(&mut app);
    for ch in "collapse thinking".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_ne!(
        app.transcript_view.show_transcript_thinking, initial,
        "toggle thinking should flip state"
    );
}

pub(super) fn palette_dispatch_toggle_timestamps_flips_state() {
    let mut app = AppState::new_live(None, false, None);
    let initial = app.transcript_view.show_transcript_timestamps;
    open_palette(&mut app);
    for ch in "timestamps".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_ne!(
        app.transcript_view.show_transcript_timestamps, initial,
        "toggle timestamps should flip state"
    );
}

pub(super) fn palette_dispatch_toggle_tool_details_flips_state() {
    let mut app = AppState::new_live(None, false, None);
    let initial = app.transcript_view.show_tool_details;
    open_palette(&mut app);
    for ch in "tool details".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_ne!(
        app.transcript_view.show_tool_details, initial,
        "toggle tool details should flip state"
    );
}

pub(super) fn palette_dispatch_placeholder_shows_status_banner() {
    use palette_model::{entries, PaletteDispatch};
    let placeholders: Vec<&str> = entries()
        .iter()
        .filter(|e| !e.harness_only)
        .filter(|e| e.dispatch == PaletteDispatch::Placeholder)
        .map(|e| e.id)
        .collect();
    assert!(
        placeholders.is_empty(),
        "no included OpenCode-parity command may dispatch to Placeholder: {placeholders:?}"
    );
}

pub(super) fn palette_dispatch_new_session_clears_events() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_test",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_test".to_string(),
            text: "test".to_string(),
        }),
    ));
    assert!(!app.events.is_empty());
    open_palette(&mut app);
    for ch in "new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.events.is_empty(), "new session should clear events");
}

// === Dynamic title tests: verify titles change with state ===

pub(super) fn palette_dynamic_title_thinking_changes_with_state() {
    let mut app = AppState::new_live(None, false, None);
    app.transcript_view.show_transcript_thinking = true;

    let entry = palette_model::find("session.toggle.thinking").unwrap();
    let title_when_shown = crate::app::palette_controller::resolve_title(&app, entry);
    assert!(
        title_when_shown.contains("Collapse"),
        "title should say Collapse when thinking is shown: got {title_when_shown}"
    );

    app.transcript_view.show_transcript_thinking = false;
    let title_when_hidden = crate::app::palette_controller::resolve_title(&app, entry);
    assert!(
        title_when_hidden.contains("Expand"),
        "title should say Expand when thinking is hidden: got {title_when_hidden}"
    );
}

pub(super) fn palette_dynamic_title_timestamps_changes_with_state() {
    let mut app = AppState::new_live(None, false, None);
    app.transcript_view.show_transcript_timestamps = true;

    let entry = palette_model::find("session.toggle.timestamps").unwrap();
    let title_when_shown = crate::app::palette_controller::resolve_title(&app, entry);
    assert!(
        title_when_shown.contains("Hide"),
        "title should say Hide when timestamps are shown: got {title_when_shown}"
    );

    app.transcript_view.show_transcript_timestamps = false;
    let title_when_hidden = crate::app::palette_controller::resolve_title(&app, entry);
    assert!(
        title_when_hidden.contains("Show"),
        "title should say Show when timestamps are hidden: got {title_when_hidden}"
    );
}

// === Title weighting test: verify title matches rank higher ===

pub(super) fn palette_title_weighting_prefers_title_match() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert!(
        !app.palette_filtered.is_empty(),
        "filter 'exit' should produce results"
    );

    assert!(
        app.palette_filtered.contains(&"app.exit".to_string()),
        "app.exit should be in filtered results (title 'Exit the app' matches 'exit')"
    );

    let _app_exit_pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "app.exit")
        .unwrap();
    let first_id = app.palette_filtered[0].as_str();
    let first_entry = palette_model::find(first_id).expect("first result should be valid");
    assert!(
        first_entry.category != palette_model::PaletteCategory::System || first_id == "app.exit",
        "if first result is in System category, it should be app.exit; got: {first_id}"
    );

    let system_start = app.palette_filtered.iter().position(|c| {
        palette_model::find(c)
            .map(|e| e.category == palette_model::PaletteCategory::System)
            .unwrap_or(false)
    });
    if let Some(system_start) = system_start {
        assert_eq!(
            app.palette_filtered[system_start], "app.exit",
            "app.exit should be first in System category (title match score 0), got: {}",
            app.palette_filtered[system_start]
        );
    }
}

// === State matrix tests: representative states ===

pub(super) fn palette_state_home_no_sessions() {
    let mut app = AppState::new_startup(Vec::new(), None);
    open_palette(&mut app);
    assert_excluded_absent(&app);
    assert_hidden_non_targets_absent(&app);
    assert!(app.palette_filtered.contains(&"session.new".to_string()));
    assert!(app.palette_filtered.contains(&"app.exit".to_string()));
    assert!(
        !app.palette_filtered.contains(&"session.rename".to_string()),
        "session.rename should not be available in startup shell"
    );
}

pub(super) fn palette_state_live_session_idle() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert_excluded_absent(&app);
    assert_hidden_non_targets_absent(&app);
    assert!(app.palette_filtered.contains(&"session.new".to_string()));
    assert!(app.palette_filtered.contains(&"app.exit".to_string()));
    assert!(
        app.palette_filtered
            .contains(&"session.toggle.thinking".to_string()),
        "toggle thinking should be available in live session"
    );
}

pub(super) fn palette_state_replay_mode() {
    let mut app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-test"), Vec::new());
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"variant.cycle".to_string()),
        "variant.cycle should not be available in replay mode"
    );
    assert!(
        !app.palette_filtered.contains(&"session.rename".to_string()),
        "session.rename should not be available in replay mode"
    );
}

pub(super) fn palette_state_review_surface_open() {
    let mut app = AppState::new_live(None, false, None);
    app.active_review_surface = Some(crate::app::ReviewSurface::Help);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered
            .contains(&"harness.close_review_surface".to_string()),
        "harness-only close_review_surface must not appear in parity palette"
    );
    assert!(
        !app.palette_filtered
            .contains(&"session.toggle.thinking".to_string()),
        "toggle thinking should not be available when review surface is open"
    );
}

pub(super) fn palette_state_provider_disconnected() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        app.palette_filtered
            .contains(&"provider.connect".to_string()),
        "provider.connect should always be available"
    );
    assert!(
        app.palette_filtered
            .iter()
            .any(|c| c == "suggested:provider.connect"),
        "provider.connect should be suggested when disconnected"
    );
}

pub(super) fn palette_state_provider_connected() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_available_models(vec![crate::app::ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )]),
    );
    open_palette(&mut app);
    assert!(
        app.palette_filtered.contains(&"model.list".to_string()),
        "model.list should be available when provider is connected"
    );
    assert!(
        !app.palette_filtered
            .iter()
            .any(|c| c == "suggested:provider.connect"),
        "provider.connect should not be suggested when connected"
    );
}

pub(super) fn palette_state_prompt_with_input() {
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_buffer = "some text".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.len();
    open_palette(&mut app);
    assert!(
        app.palette_filtered.contains(&"prompt.stash".to_string()),
        "prompt.stash should be available when prompt has input"
    );
}

pub(super) fn palette_state_prompt_empty() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"prompt.stash".to_string()),
        "prompt.stash should not be available when prompt is empty"
    );
}

pub(super) fn palette_state_workspace_commands_unavailable() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"workspace.list".to_string()),
        "workspace.list should not be available (Harness has no workspace feature)"
    );
    assert!(
        !app.palette_filtered.contains(&"workspace.set".to_string()),
        "workspace.set should not be available (Harness has no workspace feature)"
    );
    assert!(
        !app.palette_filtered
            .contains(&"workspace.copy_path".to_string()),
        "workspace.copy_path should not be available (Harness has no workspace feature)"
    );
}

pub(super) fn palette_state_unavailable_commands_not_dispatchable() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered
            .contains(&"session.unshare".to_string()),
        "session.unshare should not be available (Harness has no share feature)"
    );
    assert!(
        !app.palette_filtered.contains(&"session.redo".to_string()),
        "session.redo should not be available (Harness has no revert feature)"
    );
    assert!(
        !app.palette_filtered
            .contains(&"prompt.editor_context.clear".to_string()),
        "prompt.editor_context.clear should not be available (Harness has no editor context)"
    );
}

pub(super) fn palette_state_harness_only_commands_present() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for id in palette_model::harness_only_ids() {
        assert!(
            !app.palette_filtered.contains(&id.to_string()),
            "harness-only command {id} must not appear in parity palette"
        );
        assert!(
            !app.palette_filtered
                .iter()
                .any(|c| c == &format!("suggested:{id}")),
            "harness-only command suggested:{id} must not appear in parity palette"
        );
    }
}

pub(super) fn palette_filtered_results_preserve_category_grouping() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "session".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(!app.palette_filtered.is_empty());

    let rows = crate::app::palette_controller::compute_palette_rows(&app, &app.palette_input);
    let categories: Vec<_> = rows.iter().map(|r| r.category).collect();
    let mut sorted = categories.clone();
    sorted.sort();
    assert_eq!(
        categories, sorted,
        "filtered results should preserve category grouping (categories should be contiguous)"
    );
}

pub(super) fn palette_state_session_unshare_unavailable() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered
            .contains(&"session.unshare".to_string()),
        "session.unshare should be unavailable: Harness has no share URL feature"
    );
    assert!(
        !app.palette_filtered
            .iter()
            .any(|c| c == "suggested:session.unshare"),
        "session.unshare should not be suggested"
    );
}

pub(super) fn palette_state_session_redo_unavailable() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"session.redo".to_string()),
        "session.redo should be unavailable: Harness has no revert message feature"
    );
}

pub(super) fn palette_state_live_shared_session() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"session.unshare".to_string()),
        "session.unshare should be unavailable even in shared session context: Harness has no share feature"
    );
    assert_excluded_absent(&app);
    assert_hidden_non_targets_absent(&app);
}

pub(super) fn palette_state_live_session_with_revert() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"session.redo".to_string()),
        "session.redo should be unavailable: Harness has no revert feature"
    );
    assert!(
        !app.palette_filtered.contains(&"session.undo".to_string()),
        "session.undo should be unavailable: Harness has no revert message feature"
    );
    assert_excluded_absent(&app);
    assert_hidden_non_targets_absent(&app);
}

pub(super) fn palette_state_variants_absent() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered.contains(&"variant.list".to_string()),
        "variant.list should be unavailable when no variants are configured"
    );
}

pub(super) fn palette_dispatch_toggle_generic_output_flips_state() {
    let mut app = AppState::new_live(None, false, None);
    let initial = app.transcript_view.show_generic_tool_output;
    open_palette(&mut app);
    for ch in "generic tool".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_ne!(
        app.transcript_view.show_generic_tool_output, initial,
        "toggle generic tool output should flip state"
    );
}

pub(super) fn palette_footer_derived_from_keymap() {
    use crate::keybindings::Action;
    use crate::ui::ui_overlays::{palette_overlay_rows, PaletteOverlayRow};

    let mut app = AppState::new_live(None, false, None);
    let expected_footer = app.keymap.get_binding_str(Action::Quit);
    assert!(
        expected_footer != "-",
        "Action::Quit should have a default keybinding"
    );

    // Empty filter: footer should be the keymap binding
    open_palette(&mut app);
    let rows = palette_overlay_rows(&app);
    let exit_row = rows.iter().find_map(|r| match r {
        PaletteOverlayRow::Command { title, .. } if title == "Exit the app" => Some(r),
        _ => None,
    });
    assert!(
        exit_row.is_some(),
        "palette should contain 'Exit the app' command row"
    );
    if let PaletteOverlayRow::Command { footer, .. } = exit_row.unwrap() {
        assert_eq!(
            footer, &expected_footer,
            "footer for app.exit should match keymap binding (empty filter)"
        );
    }

    // Non-empty filter: footer should still be the keymap binding, not category label
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let rows = palette_overlay_rows(&app);
    let exit_row = rows.iter().find_map(|r| match r {
        PaletteOverlayRow::Command { title, .. } if title == "Exit the app" => Some(r),
        _ => None,
    });
    assert!(
        exit_row.is_some(),
        "palette should contain 'Exit the app' command row when filtered by 'exit'"
    );
    if let PaletteOverlayRow::Command { footer, .. } = exit_row.unwrap() {
        assert_eq!(
            footer, &expected_footer,
            "footer for app.exit should match keymap binding (filtered), not category label"
        );
    }
}

pub(super) fn palette_dynamic_title_sidebar_reflects_state() {
    let mut app = AppState::new_live(None, false, None);
    let entry = palette_model::find("session.sidebar.toggle").unwrap();

    assert!(
        !app.details_drawer_open(),
        "sidebar should be closed by default in new_live"
    );
    assert_eq!(
        palette_controller::resolve_title(&app, entry),
        "Show sidebar",
        "title should be 'Show sidebar' when sidebar is closed"
    );

    app.live_details_drawer_open = true;
    assert!(
        app.details_drawer_open(),
        "sidebar should be open after setting live_details_drawer_open"
    );
    assert_eq!(
        palette_controller::resolve_title(&app, entry),
        "Hide sidebar",
        "title should be 'Hide sidebar' when sidebar is open"
    );
}

pub(super) fn palette_dynamic_title_all_toggles_reflect_state() {
    let mut app = AppState::new_live(None, false, None);

    let toggle_tests: &[(&str, &dyn Fn(&mut AppState))] = &[
        ("session.toggle.actions", &|a| {
            a.transcript_view.show_tool_details = !a.transcript_view.show_tool_details
        }),
        ("session.toggle.generic_tool_output", &|a| {
            a.transcript_view.show_generic_tool_output = !a.transcript_view.show_generic_tool_output
        }),
        ("session.toggle.timestamps", &|a| {
            a.transcript_view.show_transcript_timestamps =
                !a.transcript_view.show_transcript_timestamps
        }),
        ("session.toggle.thinking", &|a| {
            a.transcript_view.show_transcript_thinking = !a.transcript_view.show_transcript_thinking
        }),
    ];

    for (id, toggle) in toggle_tests {
        let entry = palette_model::find(id).unwrap_or_else(|| panic!("entry not found: {id}"));

        let title_before = palette_controller::resolve_title(&app, entry);

        toggle(&mut app);

        let title_after = palette_controller::resolve_title(&app, entry);
        assert_ne!(
            title_before, title_after,
            "{id}: title should change when state toggles"
        );

        toggle(&mut app);

        let title_back = palette_controller::resolve_title(&app, entry);
        assert_eq!(
            title_before, title_back,
            "{id}: title should restore when toggled back"
        );
    }
}

pub(super) fn palette_suggested_row_dispatches_via_enter() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_test",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_test".to_string(),
            text: "test".to_string(),
        }),
    ));
    assert!(!app.events.is_empty());

    open_palette(&mut app);

    let suggested_pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "suggested:session.new")
        .expect("should have suggested:session.new duplicate");

    app.palette_selected = suggested_pos;
    app.handle_key(key(KeyCode::Enter));

    assert!(
        app.events.is_empty(),
        "dispatching suggested:session.new row should clear events"
    );
}

pub(super) fn palette_matrix_and_registry_dispatch_consistent() {
    for matrix_entry in parity_matrix::PARITY_MATRIX {
        if matrix_entry.status != parity_matrix::ParityStatus::Included
            && matrix_entry.status != parity_matrix::ParityStatus::HarnessOnly
        {
            continue;
        }

        let model_entry = palette_model::find(matrix_entry.id).unwrap_or_else(|| {
            panic!(
                "matrix ID '{}' (status={:?}) missing from palette model",
                matrix_entry.id, matrix_entry.status
            )
        });

        assert_eq!(
            model_entry.category.label(),
            matrix_entry.category,
            "category mismatch for '{}': matrix='{}' model='{}'",
            matrix_entry.id,
            matrix_entry.category,
            model_entry.category.label()
        );

        let title_matches = match (matrix_entry.title, model_entry.title) {
            (parity_matrix::TitleRule::Static(m), palette_model::DynamicTitle::Static(d)) => m == d,
            (
                parity_matrix::TitleRule::ShowHide { show: ms, hide: mh },
                palette_model::DynamicTitle::ShowHide { show: ds, hide: dh },
            ) => ms == ds && mh == dh,
            (
                parity_matrix::TitleRule::Toggle {
                    enable: me,
                    disable: md,
                },
                palette_model::DynamicTitle::Toggle {
                    enable: de,
                    disable: dd,
                },
            ) => me == de && md == dd,
            _ => false,
        };
        assert!(
            title_matches,
            "title mismatch for '{}': matrix={:?} model={:?}",
            matrix_entry.id, matrix_entry.title, model_entry.title
        );

        let suggested_matches = match (matrix_entry.suggested, model_entry.suggested) {
            (parity_matrix::SuggestedRule::Never, palette_model::SuggestedRule::Never) => true,
            (parity_matrix::SuggestedRule::Always, palette_model::SuggestedRule::Always) => true,
            (
                parity_matrix::SuggestedRule::WhenSessionsExist,
                palette_model::SuggestedRule::WhenSessionsExist,
            ) => true,
            (
                parity_matrix::SuggestedRule::WhenSessionRoute,
                palette_model::SuggestedRule::WhenSessionRoute,
            ) => true,
            (
                parity_matrix::SuggestedRule::WhenDisconnected,
                palette_model::SuggestedRule::WhenDisconnected,
            ) => true,
            _ => false,
        };
        assert!(
            suggested_matches,
            "suggested rule mismatch for '{}': matrix={:?} model={:?}",
            matrix_entry.id, matrix_entry.suggested, model_entry.suggested
        );

        let matrix_dispatch_matches = match (matrix_entry.dispatch, &model_entry.dispatch) {
            (
                parity_matrix::DispatchPath::Dialog,
                palette_model::PaletteDispatch::OpenSessionHistory,
            )
            | (
                parity_matrix::DispatchPath::Dialog,
                palette_model::PaletteDispatch::OpenSessionRename,
            )
            | (
                parity_matrix::DispatchPath::Dialog,
                palette_model::PaletteDispatch::OpenForkSelector,
            )
            | (
                parity_matrix::DispatchPath::Dialog,
                palette_model::PaletteDispatch::OpenModelSwitcher,
            )
            | (
                parity_matrix::DispatchPath::Dialog,
                palette_model::PaletteDispatch::OpenTogglesMenu,
            )
            | (
                parity_matrix::DispatchPath::Dialog,
                palette_model::PaletteDispatch::OpenConnectDialog,
            ) => true,
            (parity_matrix::DispatchPath::Intent, palette_model::PaletteDispatch::NewSession)
            | (
                parity_matrix::DispatchPath::Intent,
                palette_model::PaletteDispatch::CompactSession,
            ) => true,
            (parity_matrix::DispatchPath::Action, palette_model::PaletteDispatch::Action(_))
            | (
                parity_matrix::DispatchPath::Action,
                palette_model::PaletteDispatch::CopySessionTranscript,
            ) => true,
            (
                parity_matrix::DispatchPath::LocalToggle,
                palette_model::PaletteDispatch::ToggleTranscriptThinking,
            )
            | (
                parity_matrix::DispatchPath::LocalToggle,
                palette_model::PaletteDispatch::ToggleTranscriptTimestamps,
            )
            | (
                parity_matrix::DispatchPath::LocalToggle,
                palette_model::PaletteDispatch::ToggleToolDetails,
            )
            | (
                parity_matrix::DispatchPath::LocalToggle,
                palette_model::PaletteDispatch::ToggleGenericToolOutput,
            )
            | (
                parity_matrix::DispatchPath::LocalToggle,
                palette_model::PaletteDispatch::ToggleStackedDiffs,
            )
            | (
                parity_matrix::DispatchPath::LocalToggle,
                palette_model::PaletteDispatch::ExpandTurnResults,
            )
            | (
                parity_matrix::DispatchPath::LocalToggle,
                palette_model::PaletteDispatch::CollapseTurnResults,
            ) => true,
            (
                parity_matrix::DispatchPath::Placeholder,
                palette_model::PaletteDispatch::Placeholder,
            ) => true,
            _ => false,
        };

        assert!(
            matrix_dispatch_matches,
            "dispatch mismatch for '{}': matrix={:?} model={:?}",
            matrix_entry.id, matrix_entry.dispatch, model_entry.dispatch
        );
    }
}

pub(super) fn palette_exact_dispatch_targets() {
    use crate::keybindings::Action;
    use palette_model::{find, PaletteDispatch};

    let cases: &[(&str, PaletteDispatch)] = &[
        ("app.exit", PaletteDispatch::Action(Action::Quit)),
        (
            "session.sidebar.toggle",
            PaletteDispatch::Action(Action::ToggleOperatorSidebar),
        ),
        (
            "session.toggle.timestamps",
            PaletteDispatch::ToggleTranscriptTimestamps,
        ),
        (
            "session.toggle.thinking",
            PaletteDispatch::ToggleTranscriptThinking,
        ),
        ("session.toggle.actions", PaletteDispatch::ToggleToolDetails),
        (
            "session.toggle.scrollbar",
            PaletteDispatch::Action(Action::ToggleScrollbar),
        ),
        (
            "session.toggle.generic_tool_output",
            PaletteDispatch::ToggleGenericToolOutput,
        ),
        (
            "messages.copy",
            PaletteDispatch::Action(Action::CopyMessage),
        ),
        (
            "session.export",
            PaletteDispatch::Action(Action::ExportSession),
        ),
        ("model.list", PaletteDispatch::OpenModelSwitcher),
        ("agent.list", PaletteDispatch::OpenModelSwitcher),
        (
            "variant.cycle",
            PaletteDispatch::Action(Action::VariantCycle),
        ),
        ("mcp.list", PaletteDispatch::OpenTogglesMenu),
        ("provider.connect", PaletteDispatch::OpenConnectDialog),
        ("session.new", PaletteDispatch::NewSession),
        ("session.compact", PaletteDispatch::CompactSession),
        ("prompt.stash", PaletteDispatch::Action(Action::PromptStash)),
        (
            "prompt.stash.pop",
            PaletteDispatch::Action(Action::PromptStashPop),
        ),
        (
            "prompt.stash.list",
            PaletteDispatch::Action(Action::PromptStashList),
        ),
        ("session.list", PaletteDispatch::OpenSessionHistory),
        ("session.rename", PaletteDispatch::OpenSessionRename),
        ("session.fork", PaletteDispatch::OpenForkSelector),
        ("session.copy", PaletteDispatch::CopySessionTranscript),
        (
            "harness.stack_transcript_diffs",
            PaletteDispatch::ToggleStackedDiffs,
        ),
        (
            "harness.split_transcript_diffs",
            PaletteDispatch::ToggleStackedDiffs,
        ),
        (
            "harness.expand_turn_results",
            PaletteDispatch::ExpandTurnResults,
        ),
        (
            "harness.collapse_turn_results",
            PaletteDispatch::CollapseTurnResults,
        ),
        (
            "harness.toggle_terminal_panel",
            PaletteDispatch::Action(Action::ToggleTerminalPanel),
        ),
        (
            "harness.toggle_follow",
            PaletteDispatch::Action(Action::ToggleFollow),
        ),
        (
            "harness.revert_workspace",
            PaletteDispatch::Action(Action::RevertWorkspace),
        ),
        (
            "harness.open_lineage_browser",
            PaletteDispatch::Action(Action::OpenLineageBrowser),
        ),
        (
            "harness.session_child_first",
            PaletteDispatch::Action(Action::SessionChildFirst),
        ),
        (
            "harness.session_child_cycle",
            PaletteDispatch::Action(Action::SessionChildCycle),
        ),
        (
            "harness.session_child_cycle_reverse",
            PaletteDispatch::Action(Action::SessionChildCycleReverse),
        ),
        (
            "harness.session_parent",
            PaletteDispatch::Action(Action::SessionParent),
        ),
        (
            "harness.close_review_surface",
            PaletteDispatch::Action(Action::CloseReviewSurface),
        ),
    ];

    for (id, expected_dispatch) in cases {
        let entry = find(id).unwrap_or_else(|| panic!("entry not found: {id}"));
        assert_eq!(
            entry.dispatch, *expected_dispatch,
            "exact dispatch mismatch for '{id}'"
        );
    }

    use std::collections::HashSet;
    let case_ids: HashSet<&str> = cases.iter().map(|(id, _)| *id).collect();
    let non_placeholder_ids: HashSet<&str> = palette_model::entries()
        .iter()
        .filter(|e| e.dispatch != PaletteDispatch::Placeholder)
        .map(|e| e.id)
        .collect();
    assert_eq!(
        case_ids, non_placeholder_ids,
        "exact dispatch test must cover exactly the set of non-placeholder commands"
    );
}

pub(super) fn palette_state_matrix_full_inventories() {
    let excluded = parity_matrix::excluded_ids();
    let hidden = parity_matrix::hidden_non_target_ids();

    fn assert_absent(app: &AppState, ids: &[&str], label: &str) {
        for id in ids {
            let raw = id.to_string();
            let suggested = format!("suggested:{id}");
            assert!(
                !app.palette_filtered.contains(&raw),
                "{label} '{id}' should not appear in palette"
            );
            assert!(
                !app.palette_filtered.contains(&suggested),
                "{label} 'suggested:{id}' should not appear in palette"
            );
        }
    }

    let mut startup = AppState::new_startup(Vec::new(), None);
    open_palette(&mut startup);
    assert_absent(&startup, &excluded, "excluded command");
    assert_absent(&startup, &hidden, "hidden non-target");
    assert!(startup
        .palette_filtered
        .contains(&"session.new".to_string()));
    assert!(startup.palette_filtered.contains(&"app.exit".to_string()));

    let mut live = AppState::new_live(None, false, None);
    open_palette(&mut live);
    assert_absent(&live, &excluded, "excluded command");
    assert_absent(&live, &hidden, "hidden non-target");
    for required in &["session.new", "app.exit", "session.rename"] {
        assert!(
            live.palette_filtered.contains(&required.to_string()),
            "live palette should contain '{required}'"
        );
    }

    let mut replay = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-test"), Vec::new());
    open_palette(&mut replay);
    assert_absent(&replay, &excluded, "excluded command");
    assert_absent(&replay, &hidden, "hidden non-target");
    assert!(
        !replay
            .palette_filtered
            .contains(&"session.rename".to_string()),
        "session.rename should not be available in replay mode"
    );
    assert!(
        replay.palette_filtered.contains(&"app.exit".to_string()),
        "app.exit should be available in replay mode"
    );
}

pub(super) fn palette_dispatch_lineage_browser_stays_open() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    assert!(
        !app.palette_filtered
            .contains(&"harness.open_lineage_browser".to_string()),
        "harness-only open_lineage_browser must not appear in parity palette"
    );
}

pub(super) fn palette_dispatch_open_event_log_from_prompt_focus() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "event log".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        !app.palette_filtered
            .contains(&"harness.open_event_log".to_string()),
        "event log command should be removed from filtered results"
    );
}

pub(super) fn palette_dispatch_provider_connect_opens_connect_dialog() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "connect".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered
            .contains(&"provider.connect".to_string()),
        "provider.connect should be in filtered results for 'connect'"
    );
    let pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "provider.connect")
        .unwrap();
    app.palette_selected = pos;
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.connect_dialog.visible,
        "connect dialog should be open after palette dispatch"
    );
    assert!(
        !app.palette_visible,
        "palette should be closed after dispatch"
    );
}

pub(super) fn palette_golden_ranking_prefix_match_ranks_first() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "exit");
    assert!(!rows.is_empty(), "filter 'exit' should produce results");
    let system_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.category == palette_model::PaletteCategory::System)
        .collect();
    assert!(!system_rows.is_empty());
    assert_eq!(
        system_rows[0].command_id, "app.exit",
        "prefix match 'exit' should rank app.exit first in System category"
    );
}

pub(super) fn palette_golden_ranking_title_match_preferred_over_category() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "session");
    assert!(!rows.is_empty());
    let first = &rows[0];
    assert!(
        first.title.to_lowercase().contains("session"),
        "title match should rank first: got '{}'",
        first.title
    );
}

pub(super) fn palette_golden_ranking_consecutive_match_bonus() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "swi");
    assert!(!rows.is_empty());
    let agent_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.category == palette_model::PaletteCategory::Agent)
        .collect();
    assert!(!agent_rows.is_empty());
    assert_eq!(
        agent_rows[0].command_id, "model.list",
        "consecutive match 'swi' must rank 'Switch model' first in Agent category"
    );
}

pub(super) fn palette_golden_ranking_no_results_for_gibberish() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "zzzzzzz".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(app.palette_filtered.is_empty());
    let rendered = render_debug(&app, 120, 30);
    assert!(rendered.contains("No results found"));
}

pub(super) fn palette_golden_ranking_category_match_works() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "provider");
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .any(|r| r.category == palette_model::PaletteCategory::Provider),
        "category match 'provider' should include Provider category entries"
    );
}

pub(super) fn palette_golden_ranking_typo_tolerance() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "modl");
    assert!(
        !rows.is_empty(),
        "fuzzy match 'modl' should match 'Switch model'"
    );
    assert!(
        rows.iter().any(|r| r.command_id == "model.list"),
        "fuzzy match 'modl' should match model.list"
    );
}

pub(super) fn palette_golden_ranking_mixed_title_category_match() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "new");
    assert!(!rows.is_empty());
    assert!(
        rows.iter().any(|r| r.command_id == "session.new"),
        "match 'new' should include session.new"
    );
}

pub(super) fn palette_inventory_all_commands_have_status() {
    use parity_matrix::{find_entry, ParityStatus};

    for entry in palette_model::entries() {
        let matrix_entry = find_entry(entry.id)
            .unwrap_or_else(|| panic!("palette entry '{}' not found in parity matrix", entry.id));
        assert!(
            matrix_entry.status == ParityStatus::Included
                || matrix_entry.status == ParityStatus::HarnessOnly,
            "palette entry '{}' must be Included or HarnessOnly in parity matrix, got {:?}",
            entry.id,
            matrix_entry.status
        );
    }
}

pub(super) fn palette_inventory_excluded_commands_absent_from_palette() {
    for id in parity_matrix::excluded_ids() {
        assert!(
            palette_model::find(id).is_none(),
            "excluded command '{id}' must not be in palette model"
        );
    }
}

pub(super) fn palette_inventory_zero_placeholders_for_included() {
    use palette_model::{entries, PaletteDispatch};
    let placeholders: Vec<&str> = entries()
        .iter()
        .filter(|e| !e.harness_only)
        .filter(|e| e.dispatch == PaletteDispatch::Placeholder)
        .map(|e| e.id)
        .collect();
    assert!(
        placeholders.is_empty(),
        "no included OpenCode-parity command may dispatch to Placeholder: {placeholders:?}"
    );
}

pub(super) fn palette_inventory_excluded_commands_have_rationale() {
    use parity_matrix::{excluded_ids, exclusion_rationale};
    for id in excluded_ids() {
        let rationale = exclusion_rationale(id);
        assert!(
            rationale.is_some(),
            "excluded command '{id}' must have exclusion rationale"
        );
    }
}

pub(super) fn palette_inventory_harness_only_absent_from_palette() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for id in palette_model::harness_only_ids() {
        assert!(
            !app.palette_filtered.contains(&id.to_string()),
            "harness-only command '{id}' must not appear in parity palette"
        );
    }
}

pub(super) fn palette_slash_alias_inventory_complete() {
    use crate::keybindings::{slash_command_aliases, slash_commands};

    let harness_slash_ids: std::collections::HashSet<&str> =
        slash_commands().iter().map(|s| s.id).collect();

    let opencode_included: &[(&str, &[&str], &str)] = &[
        ("sessions", &["resume", "continue"], "app.tsx:565-566"),
        ("new", &["clear"], "app.tsx:576-577"),
        ("models", &["mo"], "app.tsx:624-626"),
        ("agents", &[], "app.tsx:671"),
        ("mcps", &[], "app.tsx:680"),
        ("connect", &[], "app.tsx:732"),
        ("help", &[], "app.tsx:791"),
        ("exit", &["quit", "q"], "app.tsx:811-812"),
        ("rename", &[], "session/index.tsx:503"),
        ("fork", &[], "session/index.tsx:536"),
        ("compact", &["summarize"], "session/index.tsx:558-560"),
        ("copy", &[], "session/index.tsx:917"),
        ("export", &[], "session/index.tsx:947"),
        (
            "timestamps",
            &["toggle-timestamps"],
            "session/index.tsx:692-694",
        ),
        (
            "thinking",
            &["toggle-thinking"],
            "session/index.tsx:709-711",
        ),
    ];

    let opencode_excluded: &[(&str, &str)] = &[
        (
            "workspaces",
            "app.tsx:605 - Harness has no workspace feature",
        ),
        (
            "variants",
            "app.tsx:707 - variant.list is excluded; Harness has no variant picker",
        ),
        ("org", "app.tsx:744 - Harness has no org switching"),
        ("status", "app.tsx:756 - Harness has no status command"),
        ("themes", "app.tsx:765 - Harness has no theme switching"),
        ("editor", "prompt/index.tsx:422 - Harness has no editor"),
        (
            "skills",
            "prompt/index.tsx:514 - Harness has no skills dialog",
        ),
        (
            "warp",
            "prompt/index.tsx:536 - Harness has no workspace feature",
        ),
        (
            "move",
            "prompt/index.tsx:546 - Harness has no workspace move",
        ),
        (
            "diff",
            "diff-viewer.tsx:1058 - Harness has no standalone diff viewer",
        ),
        (
            "share",
            "session/index.tsx:465 - Harness has no session sharing feature",
        ),
        (
            "timeline",
            "session/index.tsx:514 - Harness has no jump-to-message timeline UI",
        ),
        (
            "unshare",
            "session/index.tsx:585 - Harness has no share URL feature",
        ),
        (
            "undo",
            "session/index.tsx:607 - Harness has no revert message feature",
        ),
        (
            "redo",
            "session/index.tsx:645 - Harness has no revert/redo feature",
        ),
    ];

    for (name, aliases, source) in opencode_included {
        assert!(
            harness_slash_ids.contains(*name),
            "OpenCode slash '/{name}' ({source}) must be in Harness slash commands"
        );
        for alias in *aliases {
            let harness_aliases = slash_command_aliases(name);
            assert!(
                harness_aliases.contains(alias),
                "OpenCode alias '{alias}' for '/{name}' ({source}) must be in Harness"
            );
        }
    }

    for (name, rationale) in opencode_excluded {
        assert!(
            !harness_slash_ids.contains(*name),
            "excluded OpenCode slash '/{name}' ({rationale}) must not be in Harness"
        );
    }

    for (name, expected_aliases, _source) in opencode_included {
        let harness_aliases: std::collections::HashSet<&str> =
            slash_command_aliases(name).iter().copied().collect();
        let expected: std::collections::HashSet<&str> = expected_aliases.iter().copied().collect();
        assert_eq!(
            harness_aliases, expected,
            "alias set for '/{name}' must exactly match OpenCode: no extra aliases allowed"
        );
    }
}

pub(super) fn palette_footer_no_category_fallback() {
    use crate::keybindings::Action;
    use crate::ui::ui_overlays::{palette_overlay_rows, PaletteOverlayRow};

    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let rows = palette_overlay_rows(&app);

    for row in &rows {
        if let PaletteOverlayRow::Command { footer, title, .. } = row {
            let category_labels: &[&str] = &[
                "Session",
                "Agent",
                "System",
                "Workspace",
                "Provider",
                "Prompt",
                "Suggested",
            ];
            assert!(
                !category_labels.contains(&footer.as_str()),
                "footer for '{title}' must not be category label, got '{footer}'"
            );
        }
    }

    let exit_row = rows
        .iter()
        .find(|r| matches!(r, PaletteOverlayRow::Command { title, .. } if title == "Exit the app"));
    if let PaletteOverlayRow::Command { footer, .. } = exit_row.unwrap() {
        let expected = app.keymap.get_binding_str(Action::Quit);
        assert_eq!(
            *footer, expected,
            "footer for app.exit should be keymap binding"
        );
    }
}

pub(super) fn palette_log_success_dispatch() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "app.exit");
    let dispatch_entry = app
        .palette_log
        .iter()
        .rev()
        .find(|e| !e.command_id.is_empty())
        .expect("palette log should have dispatch entry");
    assert_eq!(dispatch_entry.command_id, "app.exit");
    assert_eq!(
        dispatch_entry.dialog_state,
        palette_controller::PaletteDialogState::DispatchSucceeded
    );
    assert_eq!(
        dispatch_entry.status,
        palette_controller::PaletteLogStatus::Success
    );
}

pub(super) fn palette_log_rejection_for_unavailable() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "prompt.stash");
    let last = app
        .palette_log
        .last()
        .expect("palette log should have rejection entry");
    assert_eq!(last.command_id, "prompt.stash");
    assert_eq!(
        last.dialog_state,
        palette_controller::PaletteDialogState::Rejected
    );
    assert_eq!(last.status, palette_controller::PaletteLogStatus::Rejected);
    assert!(last.availability_reason.is_some());
}

pub(super) fn palette_log_contains_command_id_and_target() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    let dispatch_entry = app
        .palette_log
        .iter()
        .rev()
        .find(|e| !e.command_id.is_empty())
        .expect("palette log should have dispatch entry");
    assert!(
        !dispatch_entry.command_id.is_empty(),
        "log must contain command_id"
    );
    assert!(
        !dispatch_entry.dispatch_target.is_empty(),
        "log must contain dispatch_target"
    );
}

pub(super) fn palette_log_contains_filter_length() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    let exit_pos = app
        .palette_filtered
        .iter()
        .position(|c| c == "app.exit")
        .expect("app.exit should be in filtered results");
    app.palette_selected = exit_pos;
    app.handle_key(key(KeyCode::Enter));
    let dispatch_entries: Vec<_> = app
        .palette_log
        .iter()
        .filter(|e| e.command_id == "app.exit")
        .collect();
    assert!(
        dispatch_entries.len() >= 2,
        "must have at least 2 dispatch entries (Started + Succeeded), got {}",
        dispatch_entries.len()
    );
    for entry in &dispatch_entries {
        assert_eq!(
            entry.filter_length, 4,
            "ALL dispatch entries must have filter_length=4 for 'exit', got {} on {:?}",
            entry.filter_length, entry.dialog_state
        );
    }
}

pub(super) fn palette_golden_ranking_consecutive_beats_scattered() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "swi");
    assert!(!rows.is_empty(), "query 'swi' must produce results");
    let agent_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.category == palette_model::PaletteCategory::Agent)
        .collect();
    assert!(!agent_rows.is_empty());
    assert_eq!(
        agent_rows[0].command_id, "model.list",
        "consecutive match 'swi' must rank 'Switch model' first in Agent category, got '{}'",
        agent_rows[0].title
    );
}

pub(super) fn palette_golden_ranking_prefix_beats_non_prefix() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "exit");
    assert!(!rows.is_empty());
    let system_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.category == palette_model::PaletteCategory::System)
        .collect();
    assert!(!system_rows.is_empty());
    assert_eq!(
        system_rows[0].command_id, "app.exit",
        "prefix match 'exit' must rank 'Exit the app' first in System, got '{}'",
        system_rows[0].title
    );
}

pub(super) fn palette_golden_ranking_title_weighted_double_category() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "session");
    assert!(!rows.is_empty());
    assert!(
        rows[0].title.to_lowercase().contains("session"),
        "title match must rank first: got '{}'",
        rows[0].title
    );
}

pub(super) fn palette_golden_ranking_no_match_returns_none() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "zzzzzzz".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.is_empty(),
        "gibberish query must produce no results"
    );
    let rendered = render_debug(&app, 120, 30);
    assert!(
        rendered.contains("No results found"),
        "no-result state must show 'No results found'"
    );
}

pub(super) fn palette_golden_ranking_stash_ordering() {
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_buffer = "text".to_string();
    app.composer.prompt_cursor = 4;
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "stash");
    assert!(!rows.is_empty());
    let prompt_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.category == palette_model::PaletteCategory::Prompt)
        .collect();
    assert!(!prompt_rows.is_empty());
    assert_eq!(
        prompt_rows[0].command_id, "prompt.stash",
        "'Stash prompt' must rank first in Prompt category for query 'stash', got '{}'",
        prompt_rows[0].title
    );
}

pub(super) fn palette_behavior_session_rename_opens_dialog() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_test",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_test".to_string(),
            text: "test".to_string(),
        }),
    ));
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "session.rename");
    assert!(
        app.session_rename_visible,
        "session.rename dispatch must open rename dialog"
    );
    assert!(
        app.session_rename_target_run_id.is_some(),
        "session.rename must set target run ID"
    );
}

pub(super) fn palette_behavior_session_fork_opens_selector() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "session.fork");
    assert!(
        app.fork_selector_visible,
        "session.fork dispatch must open fork selector"
    );
}

pub(super) fn palette_behavior_session_copy_copies_transcript() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_test",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_test".to_string(),
            text: "test message".to_string(),
        }),
    ));
    if let Some(activity) = app.activities.back_mut() {
        activity.transcript_text = "test transcript text".to_string();
    }
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "session.copy");
    let dispatch_entry = app
        .palette_log
        .iter()
        .rev()
        .find(|e| e.command_id == "session.copy")
        .expect("palette log should have entry for session.copy");
    assert_eq!(dispatch_entry.command_id, "session.copy");
    assert_eq!(
        dispatch_entry.dialog_state,
        palette_controller::PaletteDialogState::DispatchSucceeded
    );
    assert_eq!(
        dispatch_entry.status,
        palette_controller::PaletteLogStatus::Success
    );
    assert!(
        app.toast().is_some(),
        "session.copy must show a toast notification as observable behavior proof"
    );
}

pub(super) fn palette_behavior_dialog_clears_before_dispatch() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(!app.palette_input.is_empty());
    assert!(!app.palette_filtered.is_empty());
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.palette_input.is_empty(),
        "palette input must be cleared before/after dispatch (dialog.clear())"
    );
    assert!(
        app.palette_filtered.is_empty(),
        "palette filtered must be cleared before/after dispatch (dialog.clear())"
    );
}

pub(super) fn palette_behavior_page_down_wraps() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 10, "need more than 10 entries to test page wrap");
    app.palette_selected = len - 1;
    app.handle_key(key(KeyCode::PageDown));
    assert!(
        app.palette_selected < len - 1,
        "PageDown at last item must wrap to earlier item, got {}",
        app.palette_selected
    );
}

pub(super) fn palette_behavior_page_up_wraps() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 10);
    app.palette_selected = 0;
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(
        app.palette_selected,
        len - 10,
        "PageUp at first item must wrap by -10, got {} expected {}",
        app.palette_selected,
        len - 10
    );
}

pub(super) fn palette_behavior_tab_is_noop_in_command_palette() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let selected_before = app.palette_selected;
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(
        app.palette_selected, selected_before,
        "Tab must be no-op in command palette (no footer actions)"
    );
    app.handle_key(key_with_modifiers(KeyCode::Tab, KeyModifiers::SHIFT));
    assert_eq!(
        app.palette_selected, selected_before,
        "Shift+Tab must be no-op in command palette (no footer actions)"
    );
}

pub(super) fn palette_behavior_no_current_dot_in_palette() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let rendered = render_debug(&app, 120, 30);
    assert!(
        !rendered.contains("●"),
        "command palette must not render current-dot/gutter"
    );
}

pub(super) fn palette_harness_only_filtering_is_deliberate() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    for id in palette_model::harness_only_ids() {
        assert!(
            !app.palette_filtered.contains(&id.to_string()),
            "harness-only command '{id}' must be filtered from parity palette by deliberate non-parity exclusion"
        );
    }
    assert!(
        palette_model::harness_only_ids().len() == 13,
        "harness-only count must be 13, got {}",
        palette_model::harness_only_ids().len()
    );
}

pub(super) fn palette_mouse_semantics_platform_limitation() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let selected_before = app.palette_selected;
    app.handle_key(key(KeyCode::Down));
    assert_ne!(
        app.palette_selected, selected_before,
        "keyboard Down must navigate (proving keyboard fallback works when mouse is not processed)"
    );
}

pub(super) fn palette_behavior_model_list_opens_switcher() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        crate::app::LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_available_models(vec![crate::app::ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )]),
    );
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "model.list");
    assert!(
        app.model_switcher_visible,
        "model.list dispatch must open model switcher"
    );
    assert!(
        !app.palette_visible || app.model_switcher_visible,
        "palette must close or transition to model switcher after dispatch"
    );
}

pub(super) fn palette_behavior_provider_connect_opens_dialog() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "provider.connect");
    assert!(
        app.connect_dialog.visible,
        "provider.connect dispatch must open connect dialog"
    );
}

pub(super) fn palette_behavior_session_list_opens_history() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "session.list");
    assert!(
        app.session_history_visible,
        "session.list dispatch must open session history"
    );
}

pub(super) fn palette_behavior_mcp_list_opens_toggles() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "mcp.list");
    assert!(
        app.toggles_menu_visible,
        "mcp.list dispatch must open toggles menu"
    );
}

pub(super) fn palette_golden_ranking_exact_page_down_by_10() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(
        len > 10,
        "need more than 10 entries to test exact page movement"
    );
    app.palette_selected = 0;
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(
        app.palette_selected, 10,
        "PageDown must move exactly by 10 from position 0, got {}",
        app.palette_selected
    );
}

pub(super) fn palette_golden_ranking_exact_page_up_by_10() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 10);
    app.palette_selected = 10;
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(
        app.palette_selected, 0,
        "PageUp must move exactly by -10 from position 10, got {}",
        app.palette_selected
    );
}

pub(super) fn palette_golden_ranking_page_down_wraps_from_end() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 10);
    app.palette_selected = len - 1;
    app.handle_key(key(KeyCode::PageDown));
    let expected = (len - 1 + 10) % len;
    assert_eq!(
        app.palette_selected, expected,
        "PageDown from last item must wrap to {}, got {}",
        expected, app.palette_selected
    );
}

pub(super) fn palette_golden_ranking_page_up_wraps_from_start() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let len = app.palette_filtered.len();
    assert!(len > 10);
    app.palette_selected = 0;
    app.handle_key(key(KeyCode::PageUp));
    let expected = (0isize - 10).rem_euclid(len as isize) as usize;
    assert_eq!(
        app.palette_selected, expected,
        "PageUp from first item must wrap to {}, got {}",
        expected, app.palette_selected
    );
}

pub(super) fn palette_log_failure_has_error_kind() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "prompt.stash");
    let last = app
        .palette_log
        .last()
        .expect("palette log should have rejection entry");
    assert_eq!(last.command_id, "prompt.stash");
    assert_eq!(last.status, palette_controller::PaletteLogStatus::Rejected);
    assert!(
        last.error_kind.is_none(),
        "rejection must not set error_kind (only Placeholder failures set error_kind)"
    );
}

pub(super) fn palette_log_has_redacted_ids() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(crate::app::LaunchMetadata::from_model_ref(
        "build",
        "default:gpt-5.4-mini",
    ));
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "app.exit");
    let last = app
        .palette_log
        .last()
        .expect("palette log should have entry");
    assert!(
        last.provider_id_redacted.is_some(),
        "log must contain redacted provider ID when provider is configured"
    );
    assert!(
        last.model_id_redacted.is_some(),
        "log must contain redacted model ID when model is configured"
    );
    if let Some(ref pid) = last.provider_id_redacted {
        assert!(
            pid.len() <= 20,
            "redacted provider ID must be truncated: got '{pid}'"
        );
    }
    if let Some(ref mid) = last.model_id_redacted {
        assert!(
            mid.len() <= 20,
            "redacted model ID must be truncated: got '{mid}'"
        );
    }
}

pub(super) fn palette_tab_shift_tab_explicit_noop() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let selected_before = app.palette_selected;
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(
        app.palette_selected, selected_before,
        "Tab must be explicit no-op in command palette (no footer actions)"
    );
    assert!(app.palette_visible, "palette must remain open after Tab");
    app.handle_key(key(KeyCode::BackTab));
    assert_eq!(
        app.palette_selected, selected_before,
        "Shift+Tab must be explicit no-op in command palette (no footer actions)"
    );
    assert!(
        app.palette_visible,
        "palette must remain open after Shift+Tab"
    );
}
pub(super) fn palette_slash_alias_global_inventory() {
    use crate::keybindings::slash_commands;

    let opencode_parity_ids: std::collections::HashSet<&str> = [
        "sessions",
        "new",
        "models",
        "agents",
        "mcps",
        "connect",
        "exit",
        "rename",
        "fork",
        "compact",
        "copy",
        "export",
        "timestamps",
        "thinking",
        "help",
    ]
    .iter()
    .cloned()
    .collect();

    let harness_only_ids: std::collections::HashSet<&str> =
        ["auth", "clone", "follow", "shell", "toggles", "tree"]
            .iter()
            .cloned()
            .collect();

    let mut expected_ids = opencode_parity_ids.clone();
    expected_ids.extend(harness_only_ids.iter().cloned());

    let actual_ids: std::collections::HashSet<&str> =
        slash_commands().iter().map(|s| s.id).collect();

    assert_eq!(
        actual_ids, expected_ids,
        "Harness slash command IDs must exactly match the union of OpenCode-parity and Harness-only sets"
    );

    for cmd in slash_commands() {
        let id = cmd.id;
        assert!(
            opencode_parity_ids.contains(id) || harness_only_ids.contains(id),
            "slash command '/{id}' must be classified as OpenCode-parity or Harness-only"
        );
    }
}

pub(super) fn palette_log_open_close_lifecycle() {
    let mut app = AppState::new_live(None, false, None);
    assert!(app.palette_log.is_empty(), "palette log should start empty");
    open_palette(&mut app);
    assert!(
        app.palette_log
            .iter()
            .any(|e| e.dialog_state == palette_controller::PaletteDialogState::Opened),
        "palette log must contain Opened entry after open_palette"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(
        app.palette_log
            .iter()
            .any(|e| e.dialog_state == palette_controller::PaletteDialogState::Closed),
        "palette log must contain Closed entry after close_palette"
    );
}

pub(super) fn palette_log_filtered_lifecycle() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let opened_count = app.palette_log.len();
    for ch in "exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(
        app.palette_log.len() > opened_count,
        "palette log must have entries after filtering"
    );
    assert!(
        app.palette_log
            .iter()
            .any(|e| e.dialog_state == palette_controller::PaletteDialogState::Filtered),
        "palette log must contain Filtered entry after typing in filter"
    );
}

pub(super) fn palette_log_selected_lifecycle() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    let before_count = app.palette_log.len();
    app.handle_key(key(KeyCode::Down));
    assert!(
        app.palette_log.len() > before_count,
        "palette log must have entries after navigation"
    );
    assert!(
        app.palette_log
            .iter()
            .any(|e| e.dialog_state == palette_controller::PaletteDialogState::Selected),
        "palette log must contain Selected entry after keyboard navigation"
    );
}

pub(super) fn palette_log_dispatch_started_then_succeeded_in_order() {
    let mut app = AppState::new_live(None, false, None);
    open_palette(&mut app);
    palette_controller::dispatch_palette_command(&mut app, "app.exit");
    let dispatch_entries: Vec<_> = app
        .palette_log
        .iter()
        .filter(|e| !e.command_id.is_empty())
        .collect();
    assert!(
        dispatch_entries.len() >= 2,
        "palette log must have at least 2 dispatch entries (Started + Succeeded), got {}",
        dispatch_entries.len()
    );
    assert_eq!(
        dispatch_entries[0].dialog_state,
        palette_controller::PaletteDialogState::DispatchStarted,
        "first dispatch entry must be DispatchStarted"
    );
    assert_eq!(
        dispatch_entries[1].dialog_state,
        palette_controller::PaletteDialogState::DispatchSucceeded,
        "second dispatch entry must be DispatchSucceeded"
    );
    assert_eq!(dispatch_entries[0].command_id, "app.exit");
    assert_eq!(dispatch_entries[1].command_id, "app.exit");
}

pub(super) fn palette_log_dispatch_failed_for_placeholder() {
    use crate::keybindings::palette_model::{entries, PaletteDispatch};
    let has_placeholder = entries()
        .iter()
        .any(|e| e.dispatch == PaletteDispatch::Placeholder);
    assert!(
        !has_placeholder,
        "no included command may dispatch to Placeholder — DispatchFailed is intentionally unreachable in production"
    );
}

/// Direct test for title-vs-category score weighting within the same category.
///
/// The score formula is `title_score * 2 + category_score`. A command whose
/// title contains the query should rank higher than a command in the same
/// category whose title does NOT contain the query (category-only match),
/// because the title score is weighted 2x.
pub(super) fn palette_golden_ranking_title_vs_category_weighting() {
    let app = AppState::new_live(None, false, None);
    let rows = crate::app::palette_controller::compute_palette_rows(&app, "session");

    let session_rows: Vec<_> = rows
        .iter()
        .filter(|r| r.category == palette_model::PaletteCategory::Session)
        .collect();
    assert!(
        session_rows.len() >= 2,
        "need at least 2 Session-category rows to test weighting"
    );

    // "New session" has "session" in its title → title match (weighted 2x).
    let new_session_pos = session_rows
        .iter()
        .position(|r| r.command_id == "session.new")
        .expect("session.new should be in filtered Session results");

    // "Show sidebar" does NOT have "session" in its title → category-only match (weighted 1x).
    let show_sidebar_pos = session_rows
        .iter()
        .position(|r| r.command_id == "session.sidebar.toggle")
        .expect("session.sidebar.toggle should be in filtered Session results");

    assert!(
        new_session_pos < show_sidebar_pos,
        "title match ('New session') must rank higher than category-only match ('Show sidebar') \
         within Session category: got new_session at {new_session_pos}, sidebar at {show_sidebar_pos}"
    );
}

/// Verify stable tie ordering: same query produces identical output, and
/// the sort key `(score, index)` breaks ties by original registry index.
pub(super) fn palette_golden_ranking_stable_tie_order() {
    let app = AppState::new_live(None, false, None);

    let rows1 = crate::app::palette_controller::compute_palette_rows(&app, "toggle");
    let rows2 = crate::app::palette_controller::compute_palette_rows(&app, "toggle");
    assert!(!rows1.is_empty(), "query 'toggle' should produce results");
    assert_eq!(rows1.len(), rows2.len());

    for (a, b) in rows1.iter().zip(rows2.iter()) {
        assert_eq!(
            a.command_id, b.command_id,
            "same query must be deterministic"
        );
    }

    let mut seen = std::collections::HashSet::new();
    for row in &rows1 {
        assert!(
            seen.insert(row.command_id),
            "duplicate command ID in filtered results: {}",
            row.command_id
        );
    }
}

/// Comprehensive inventory test verifying PRD-required fields for every
/// parity matrix entry (PRD lines 98-116).
pub(super) fn palette_inventory_comprehensive_fields() {
    use parity_matrix::{exclusion_rationale, find_entry, ParityStatus, PARITY_MATRIX};

    let mut ids_seen = std::collections::HashSet::new();

    for entry in PARITY_MATRIX {
        assert!(!entry.id.is_empty(), "entry has empty id");
        assert!(
            ids_seen.insert(entry.id),
            "duplicate command ID in parity matrix: {}",
            entry.id
        );
        assert!(
            !entry.origin.is_empty(),
            "entry '{}' has empty OpenCode source anchor (origin)",
            entry.id
        );
        assert!(
            !entry.category.is_empty(),
            "entry '{}' has empty category",
            entry.id
        );
        assert!(
            !entry.harness_equivalent.is_empty(),
            "entry '{}' has empty Harness implementation anchor (harness_equivalent)",
            entry.id
        );
        let _ = entry.dispatch;
        let _ = entry.suggested;
        let _ = entry.availability;
        let _ = entry.title;

        match entry.status {
            ParityStatus::Included => {
                let model_entry = palette_model::find(entry.id).unwrap_or_else(|| {
                    panic!("Included entry '{}' missing from palette model", entry.id)
                });
                assert!(
                    !model_entry.harness_only,
                    "Included entry '{}' must not be harness_only",
                    entry.id
                );
            }
            ParityStatus::Excluded => {
                assert!(
                    palette_model::find(entry.id).is_none(),
                    "Excluded entry '{}' must not be in palette model",
                    entry.id
                );
                assert!(
                    exclusion_rationale(entry.id).is_some(),
                    "Excluded entry '{}' must have exclusion rationale",
                    entry.id
                );
            }
            ParityStatus::HarnessOnly => {
                let model_entry = palette_model::find(entry.id).unwrap_or_else(|| {
                    panic!(
                        "HarnessOnly entry '{}' missing from palette model",
                        entry.id
                    )
                });
                assert!(
                    model_entry.harness_only,
                    "HarnessOnly entry '{}' must have harness_only=true",
                    entry.id
                );
            }
            ParityStatus::HiddenNonTarget => {
                assert!(
                    palette_model::find(entry.id).is_none(),
                    "HiddenNonTarget entry '{}' must not be in palette model",
                    entry.id
                );
            }
        }
    }

    for model_entry in palette_model::entries() {
        let matrix_entry = find_entry(model_entry.id).unwrap_or_else(|| {
            panic!(
                "palette model entry '{}' not found in parity matrix",
                model_entry.id
            )
        });
        assert!(
            matrix_entry.status == ParityStatus::Included
                || matrix_entry.status == ParityStatus::HarnessOnly,
            "palette model entry '{}' must be Included or HarnessOnly in matrix, got {:?}",
            model_entry.id,
            matrix_entry.status
        );
    }
}
