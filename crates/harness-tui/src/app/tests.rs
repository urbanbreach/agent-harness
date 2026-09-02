#![allow(
    deprecated,
    reason = "deprecated compaction event variants kept for backward compatibility tests"
)]

use super::*;
use crate::layout::FrameLayoutPlan;
use crate::overlay::OverlayKind;
use crate::theme::Theme;
use crate::ui::{
    render_app, reset_transcript_selection_cache_metrics_for_test, subagent_footer_target_at,
    transcript_mouse_target, transcript_selection_cache_build_count_for_test,
    transcript_selection_cell, transcript_selection_debug_snapshot, SubagentFooterTarget,
    TranscriptMouseTarget, TranscriptScrollbarHit, WheelTarget,
};
use crate::UnwrapOrAbort;
use crossterm::event::{MouseButton, MouseEvent};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, CompactionWrittenEvent, EditAppliedEvent, EventActor,
    EventEnvelopeV1, EventV1, ExecutionTimingMetadata, PermissionRequestedEvent,
    PermissionResolvedEvent, ProviderReasoningDeltaEvent, ProviderRequestFinishedEvent,
    ProviderRequestFinishedMetadata, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, RunFinishedEvent, RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskScheduleState, TaskScheduledEvent,
    TaskTerminalScope, ToolCallFinishedEvent, ToolCallLifecycleState, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::proj::inspect_resume_plan;
use harness_providers::ProviderErrorCategory;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::{backend::TestBackend, Terminal};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const TEST_FRAME_AREA: Rect = Rect::new(0, 0, 140, 40);

struct ClipboardModeGuard;

impl ClipboardModeGuard {
    fn disabled_copy_on_select() -> Self {
        crate::clipboard::set_copy_on_select_disabled_override(Some(true));
        Self
    }
}

impl Drop for ClipboardModeGuard {
    fn drop(&mut self) {
        crate::clipboard::set_copy_override(None);
        crate::clipboard::set_copy_on_select_disabled_override(None);
    }
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_app_{seq:04}"),
        seq,
        run_id: "run_app_tests".into(),
        mono_ms: seq,
        ts: Some("2026-02-03T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("app-tests".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn envelope_with_actor(
    seq: u64,
    request_id: &str,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    let mut event = envelope(seq, request_id, payload);
    event.actor = actor;
    event
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn render_debug(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    format!("{:?}", terminal.backend().buffer())
}

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

macro_rules! delegate_test {
    ($name:ident => $target:path) => {
        #[test]
        fn $name() {
            $target();
        }
    };
}

#[cfg(test)]
#[path = "tests/toggles_menu_tests.rs"]
mod toggles_menu_tests;

#[cfg(test)]
#[path = "tests/theme_runtime_tests.rs"]
mod theme_runtime_tests;

#[cfg(test)]
#[path = "tests/live_turn_status_tests.rs"]
mod live_turn_status_tests;

#[cfg(test)]
#[path = "tests/live_turn_watcher_interaction_tests.rs"]
mod live_turn_watcher_interaction_tests;

#[cfg(test)]
#[path = "tests/help_browser_mouse_tests.rs"]
mod help_browser_mouse_tests;

#[cfg(test)]
#[path = "tests/modal_press_invalidation_tests.rs"]
mod modal_press_invalidation_tests;

#[cfg(test)]
#[path = "tests/p1_02_modal_chrome_tests.rs"]
mod p1_02_modal_chrome_tests;

#[cfg(test)]
#[path = "tests/transcript_return_to_live_tests.rs"]
mod transcript_return_to_live_tests;

delegate_test!(toggles_slash_command_opens_command_styled_menu => toggles_menu_tests::toggles_slash_command_opens_command_styled_menu);
delegate_test!(yolo_toggle_requires_confirmation_and_enables_entries => toggles_menu_tests::yolo_toggle_requires_confirmation_and_enables_entries);
delegate_test!(toggles_config_drops_primary_profiles_and_keeps_subagents => toggles_menu_tests::toggles_config_drops_primary_profiles_and_keeps_subagents);
delegate_test!(toggles_config_drops_primary_agents_and_keeps_subagents => toggles_menu_tests::toggles_config_drops_primary_agents_and_keeps_subagents);
delegate_test!(toggles_menu_sanitizes_config_derived_text => toggles_menu_tests::toggles_menu_sanitizes_config_derived_text);
delegate_test!(help_mouse_hover_preserves_keyboard_selection_and_click_opens_detail => help_browser_mouse_tests::help_mouse_hover_preserves_keyboard_selection_and_click_opens_detail);
delegate_test!(help_mouse_search_close_and_scroll_use_modal_hit_regions => help_browser_mouse_tests::help_mouse_search_close_and_scroll_use_modal_hit_regions);
delegate_test!(help_detail_mouse_wheel_uses_single_row_steps_and_clamps => help_browser_mouse_tests::help_detail_mouse_wheel_uses_single_row_steps_and_clamps);
delegate_test!(default_app_uses_harness_dark_theme => theme_runtime_tests::default_app_uses_harness_dark_theme);
delegate_test!(explicit_harness_dark_selection_uses_harness_dark_theme => theme_runtime_tests::explicit_harness_dark_selection_uses_harness_dark_theme);
delegate_test!(explicit_harness_dark_selection_remains_available => theme_runtime_tests::explicit_harness_dark_selection_remains_available);
delegate_test!(default_harness_dark_survives_color_level_changes => theme_runtime_tests::default_harness_dark_survives_color_level_changes);
delegate_test!(legacy_glyph_mode_survives_color_and_theme_changes => theme_runtime_tests::legacy_glyph_mode_survives_color_and_theme_changes);
delegate_test!(legacy_glyph_mode_reaches_permission_and_transcript_surfaces => theme_runtime_tests::legacy_glyph_mode_reaches_permission_and_transcript_surfaces);
delegate_test!(legacy_glyph_mode_reaches_question_permission_surfaces => theme_runtime_tests::legacy_glyph_mode_reaches_question_permission_surfaces);
delegate_test!(arbitrary_viewport_composer_mouse_uses_rendered_geometry => theme_runtime_tests::arbitrary_viewport_composer_mouse_uses_rendered_geometry);
delegate_test!(thinking_phase_clock_advances_between_provider_deltas => live_turn_status_tests::thinking_phase_clock_advances_between_provider_deltas);
delegate_test!(responding_phase_clock_advances_between_provider_deltas => live_turn_status_tests::responding_phase_clock_advances_between_provider_deltas);
delegate_test!(thinking_spinner_advances_on_animation_tick => live_turn_status_tests::thinking_spinner_advances_on_animation_tick);
delegate_test!(unrelated_request_delta_does_not_reset_active_phase_clock => live_turn_status_tests::unrelated_request_delta_does_not_reset_active_phase_clock);
delegate_test!(local_fresh_turn_resets_total_clock_before_request_id_arrives => live_turn_status_tests::local_fresh_turn_resets_total_clock_before_request_id_arrives);
delegate_test!(replay_loading_does_not_arm_live_turn_clocks => live_turn_status_tests::replay_loading_does_not_arm_live_turn_clocks);
delegate_test!(thinking_to_responding_keeps_shared_spinner_frame => live_turn_status_tests::thinking_to_responding_keeps_shared_spinner_frame);
delegate_test!(stop_affordance_is_hidden_without_cancellable_task => live_turn_status_tests::stop_affordance_is_hidden_without_cancellable_task);
delegate_test!(clicking_stop_affordance_interrupts_active_task => live_turn_status_tests::clicking_stop_affordance_interrupts_active_task);
delegate_test!(hovering_stop_affordance_updates_live_status_state => live_turn_status_tests::hovering_stop_affordance_updates_live_status_state);
delegate_test!(queued_follow_up_does_not_take_clock_from_streaming_turn => live_turn_status_tests::queued_follow_up_does_not_take_clock_from_streaming_turn);
delegate_test!(live_historical_restore_rearms_streaming_turn_clocks => live_turn_status_tests::live_historical_restore_rearms_streaming_turn_clocks);
delegate_test!(hidden_delegated_child_cannot_steal_rendered_parent_clock => live_turn_status_tests::hidden_delegated_child_cannot_steal_rendered_parent_clock);
delegate_test!(hidden_delegated_child_activation_does_not_steal_detached_page_flip => live_turn_status_tests::hidden_delegated_child_activation_does_not_steal_detached_page_flip);
delegate_test!(hidden_child_event_does_not_adopt_foreground_local_echo => live_turn_status_tests::hidden_child_event_does_not_adopt_foreground_local_echo);
delegate_test!(clicking_live_turn_watcher_opens_status_dashboard => live_turn_watcher_interaction_tests::clicking_live_turn_watcher_opens_status_dashboard);
delegate_test!(command_palette_mouse_hover_moves_keyboard_selection => interaction_tests::command_palette_mouse_hover_moves_keyboard_selection);
delegate_test!(command_palette_matching_mouse_release_activates_row => interaction_tests::command_palette_matching_mouse_release_activates_row);
delegate_test!(command_palette_outside_matching_mouse_release_dismisses_top_overlay => interaction_tests::command_palette_outside_matching_mouse_release_dismisses_top_overlay);
delegate_test!(release_notes_outside_mouse_down_dismisses_and_blocks_lower_surface => interaction_tests::release_notes_outside_mouse_down_dismisses_and_blocks_lower_surface);
delegate_test!(release_notes_close_target_uses_matching_release_and_restores_focus => interaction_tests::release_notes_close_target_uses_matching_release_and_restores_focus);
delegate_test!(release_notes_keyboard_scroll_supports_steps_pages_and_bounds => interaction_tests::release_notes_keyboard_scroll_supports_steps_pages_and_bounds);
delegate_test!(command_palette_drag_cancels_armed_row_activation => interaction_tests::command_palette_drag_cancels_armed_row_activation);
delegate_test!(command_palette_release_on_different_target_cancels_activation => interaction_tests::command_palette_release_on_different_target_cancels_activation);
delegate_test!(command_palette_wheel_outside_popup_does_not_scroll => interaction_tests::command_palette_wheel_outside_popup_does_not_scroll);
delegate_test!(command_palette_scrollbar_drag_is_anchored_and_never_selects_rows => interaction_tests::command_palette_scrollbar_drag_is_anchored_and_never_selects_rows);
delegate_test!(control_modified_release_invalidates_armed_modal_target => interaction_tests::control_modified_release_invalidates_armed_modal_target);
delegate_test!(modal_footer_matching_release_activates_action => interaction_tests::modal_footer_matching_release_activates_action);
delegate_test!(trust_folder_prompt_preempts_lower_pointer_targets => interaction_tests::trust_folder_prompt_preempts_lower_pointer_targets);
delegate_test!(modal_key_event_invalidates_armed_press => modal_press_invalidation_tests::modal_key_event_invalidates_armed_press);
delegate_test!(modal_resize_invalidates_armed_press => modal_press_invalidation_tests::modal_resize_invalidates_armed_press);
delegate_test!(modal_non_left_event_invalidates_armed_press => modal_press_invalidation_tests::modal_non_left_event_invalidates_armed_press);
delegate_test!(modal_owner_change_invalidates_armed_press => modal_press_invalidation_tests::modal_owner_change_invalidates_armed_press);
delegate_test!(command_palette_wheel_scrolls_three_rows_without_changing_selection => interaction_tests::command_palette_wheel_scrolls_three_rows_without_changing_selection);
delegate_test!(top_modal_preempts_pointer_targets_from_lower_overlays => interaction_tests::top_modal_preempts_pointer_targets_from_lower_overlays);
delegate_test!(modal_resize_invalidates_stale_close_hover_geometry => interaction_tests::modal_resize_invalidates_stale_close_hover_geometry);
delegate_test!(first_modal_pointer_contact_preserves_keyboard_derived_scroll => interaction_tests::first_modal_pointer_contact_preserves_keyboard_derived_scroll);
delegate_test!(toggles_wheel_offset_drives_rendered_rows => interaction_tests::toggles_wheel_offset_drives_rendered_rows);
delegate_test!(modal_keyboard_input_invalidates_pointer_owned_state => interaction_tests::modal_keyboard_input_invalidates_pointer_owned_state);
delegate_test!(yolo_footer_targets_match_visible_action_spans => interaction_tests::yolo_footer_targets_match_visible_action_spans);
delegate_test!(yolo_footer_remains_visible_when_filter_shrinks_parent => interaction_tests::yolo_footer_remains_visible_when_filter_shrinks_parent);
delegate_test!(error_footer_targets_match_visible_action_spans => interaction_tests::error_footer_targets_match_visible_action_spans);

#[test]
fn compaction_written_status_surfaces_deterministic_fallback() {
    // arrange
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "compaction:agent_000001",
        EventV1::CompactionWritten(CompactionWrittenEvent {
            checkpoint_id: "checkpoint_000001".to_string(),
            agent_id: "agent_000001".to_string(),
            artifact_path: "artifacts/compactions/agent_000001/checkpoint_000001.json".to_string(),
            artifact_digest: Some("digest-checkpoint".to_string()),
            artifact_bytes: 123,
            trigger_reason: "manual".to_string(),
            through_seq: 10,
            through_request_id: Some("req_000001".to_string()),
            provider_id: Some("mock".to_string()),
            model_id: Some("model-1".to_string()),
            tokens_before: Some(1000),
            tokens_before_estimate: Some(980),
            tokens_after_estimate: Some(400),
            summary_tokens_estimate: Some(80),
            compacted_turns: Some(3),
            reduction_tokens_estimate: Some(580),
            reduction_percent_estimate: Some(59),
            estimate_source: Some("provider_usage".to_string()),
            summary_source: Some(harness_core::agent::ProviderCompactionSummarySource {
                strategy: "model_backed_deterministic_fallback".to_string(),
                model_ref: "mock:model-1".to_string(),
                provider_id: Some("mock".to_string()),
                model_id: Some("model-1".to_string()),
                reasoning_effort: None,
                text_verbosity: None,
                previous_summary_used: false,
                model_backed: true,
                deterministic_fallback: true,
                summary_contract_version: Some(1),
                summary_contract_enforced: Some(true),
            }),
            preserved_turns: 1,
        }),
    ));

    // act
    let status = app.compaction_status().unwrap_or_abort();
    // assert
    assert_eq!(status.state, CompactionState::Written);
    assert!(status.message.contains("deterministic fallback"));
}

fn transcript_click_position(app: &AppState, needle: &str) -> (u16, u16) {
    transcript_click_position_in_area(app, TEST_FRAME_AREA, needle)
}

fn transcript_click_position_in_area(app: &AppState, area: Rect, needle: &str) -> (u16, u16) {
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();

    for y in 0..area.height {
        let row = (0..area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (u16::try_from(column + 1).unwrap_or_abort(), y);
        }
    }

    panic!("expected row containing {needle:?}");
}

fn rendered_cell_bg(app: &AppState, column: u16, row: u16) -> Color {
    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    terminal.backend().buffer()[(column, row)].bg
}

fn rendered_changelog_header_styles(app: &AppState, area: Rect) -> Vec<(Color, Modifier)> {
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let row = (0..area.height)
        .find(|row| {
            (23..32)
                .map(|column| buffer[(column, *row)].symbol())
                .collect::<String>()
                == "Changelog"
        })
        .unwrap_or_abort();
    (23..32)
        .map(|column| {
            let cell = &buffer[(column, row)];
            (cell.fg, cell.modifier)
        })
        .collect()
}

fn rendered_compact_changelog_section_styles(app: &AppState, area: Rect) -> Vec<(Color, Modifier)> {
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let (column, row) = (0..area.height)
        .flat_map(|row| {
            let rendered_row = (0..area.width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>();
            rendered_row
                .match_indices("Changelog")
                .map(move |(column, _)| (column, row))
                .collect::<Vec<_>>()
        })
        .nth(1)
        .unwrap_or_abort();
    let column = u16::try_from(column).unwrap_or_abort();
    (column..column.saturating_add(9))
        .map(|column| {
            let cell = &buffer[(column, row)];
            (cell.fg, cell.modifier)
        })
        .collect()
}

fn default_navigation_keybindings() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "session_child_first".to_string(),
            "<leader>down".to_string(),
        ),
        ("session_child_cycle".to_string(), "right".to_string()),
        (
            "session_child_cycle_reverse".to_string(),
            "left".to_string(),
        ),
        ("session_parent".to_string(), "up".to_string()),
        ("session_background".to_string(), "ctrl+b".to_string()),
        ("variant_cycle".to_string(), "tab".to_string()),
    ])
}

#[test]
fn session_background_emits_intent_from_default_prompt_focus() {
    // arrange
    let intents = Arc::new(Mutex::new(Vec::new()));
    let captured_intents = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            captured_intents.lock().unwrap_or_abort().push(intent);
        })),
    );
    app.apply_keybindings(default_navigation_keybindings());
    assert_eq!(app.focus, Focus::Prompt);

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    ));

    // assert
    assert_eq!(
        app.status_banner.as_deref(),
        Some("foreground subagent backgrounding requested")
    );
    assert!(matches!(
        intents.lock().unwrap_or_abort().as_slice(),
        [UiIntent::BackgroundForegroundSubagents]
    ));
}

#[test]
fn session_background_demotes_selected_activity_child_handle() {
    // arrange
    // act
    // assert
    // Given: live parent with a selected activity that spawned a child task
    let intents = Arc::new(Mutex::new(Vec::new()));
    let captured_intents = Arc::clone(&intents);
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            captured_intents.lock().unwrap_or_abort().push(intent);
        })),
    );
    app.apply_keybindings(default_navigation_keybindings());
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".into(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_demote",
        "agent_child",
        "req_child_demote",
    ));
    app.transcript_view.selected_activity_index = 0;
    assert_eq!(
        app.focused_demote_handle_id().as_deref(),
        Some("req_child_demote")
    );

    // When: Ctrl+B from prompt focus
    app.handle_key(key_with_modifiers(
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
    ));

    // Then: single-handle demote intent is emitted
    assert_eq!(
        app.status_banner.as_deref(),
        Some("foreground subagent demote requested (req_child_demote)")
    );
    assert!(matches!(
        intents.lock().unwrap_or_abort().as_slice(),
        [UiIntent::DemoteForegroundChildTask { handle_id }] if handle_id == "req_child_demote"
    ));
}

#[test]
fn provider_model_change_sets_fallback_status_banner() {
    // arrange
    // act
    // assert
    // Given: streaming activity for a request with model A
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_turn",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_turn".into(),
            text: "hello".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_turn", "default", "model-a"));
    assert_eq!(app.canonical_projection_error(), None);
    assert!(app.status_banner.is_none());

    // When: same request restarts with a different model id (fallback switch)
    app.ingest_event(provider_started(4, "req_turn", "default", "model-b"));
    assert_eq!(app.canonical_projection_error(), None);

    // Then: operator banner reports the model switch
    assert_eq!(
        app.status_banner.as_deref(),
        Some("provider fallback: model-a → model-b")
    );
}

#[cfg(test)]
#[path = "tests/tool_disclosure_tests.rs"]
mod tool_disclosure_tests;

delegate_test!(mouse_click_toggles_transcript_tool_disclosure => tool_disclosure_tests::mouse_click_toggles_transcript_tool_disclosure);
delegate_test!(palette_turn_result_commands_override_failed_output_default => tool_disclosure_tests::palette_turn_result_commands_override_failed_output_default);
delegate_test!(transcript_enter_toggles_effective_failed_output_state => tool_disclosure_tests::transcript_enter_toggles_effective_failed_output_state);
delegate_test!(explicit_tool_disclosure_survives_replay_replacement => tool_disclosure_tests::explicit_tool_disclosure_survives_replay_replacement);
delegate_test!(context_group_disclosure_preserves_detached_anchor => tool_disclosure_tests::context_group_disclosure_preserves_detached_anchor);
delegate_test!(mouse_click_toggles_apply_patch_file_disclosure => tool_disclosure_tests::mouse_click_toggles_apply_patch_file_disclosure);
delegate_test!(apply_patch_default_expansion_skips_deleted_files => tool_disclosure_tests::apply_patch_default_expansion_skips_deleted_files);

#[cfg(test)]
#[path = "tests/subagent_navigation_tests.rs"]
mod subagent_navigation_tests;

delegate_test!(mouse_click_on_task_inline_row_opens_subagent_session => subagent_navigation_tests::keyboard_mouse_click_on_task_inline_row_opens_subagent_session);
delegate_test!(keyboard_sidebar_subagent_selection_opens_child_session => subagent_navigation_tests::keyboard_keyboard_sidebar_subagent_selection_opens_child_session);
delegate_test!(live_subagent_hitbox_uses_rendered_transcript_area => subagent_navigation_tests::keyboard_live_subagent_hitbox_uses_rendered_transcript_area);
delegate_test!(disk_backed_child_navigation_stays_in_live_tui_stack => subagent_navigation_tests::keyboard_disk_backed_child_navigation_stays_in_live_tui_stack);
delegate_test!(mouse_click_on_task_inline_row_uses_task_row_child_session => subagent_navigation_tests::mouse_click_on_task_inline_row_uses_task_row_child_session);
delegate_test!(mouse_up_on_completed_general_task_row_opens_child_session => subagent_navigation_tests::mouse_up_on_completed_general_task_row_opens_child_session);
delegate_test!(mouse_click_on_task_row_uses_harness_session_metadata => subagent_navigation_tests::mouse_click_on_task_row_uses_harness_session_metadata);
delegate_test!(slash_exit_from_inline_subagent_restores_parent_before_quit => subagent_navigation_tests::slash_exit_from_inline_subagent_restores_parent_before_quit);

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    fs::create_dir_all(run_dir).unwrap_or_abort();
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn transcript_selection_test_app_with_text(transcript_text: &str) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "req_copy_select".to_string(),
        profile_label: "build".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(UserMessageSubmittedEvent {
            request_id: "req_copy_select".into(),
            text: "Select this".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: transcript_text.to_string(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;
    app
}

fn transcript_selection_test_app_with_reasoning(
    thinking_text: &str,
    transcript_text: &str,
) -> AppState {
    let mut app = transcript_selection_test_app_with_text(transcript_text);
    app.activities[0].thinking_text = thinking_text.to_string();
    app
}

fn transcript_selection_test_app() -> AppState {
    transcript_selection_test_app_with_text("Copy this exact reply")
}

fn shell_card_selection_test_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_shell_card_copy",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_shell_card_copy".into(),
            provider_id: "default".to_string(),
            model_id: "model-shell".to_string(),
            prompt_summary: "shell card copy".to_string(),
            request_digest: "digest-shell-card-copy".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_shell_card_copy",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_shell_card_copy".into(),
            tool_id: "bash".to_string(),
            args_summary:
                r#"{"command":"run-copy-command","description":"Run copy-safe shell card"}"#
                    .to_string(),
            args_digest: "digest-shell-card-copy-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_shell_card_copy",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_shell_card_copy".into(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_shell_card_copy",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_shell_card_copy".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("copy target output".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "command": "run-copy-command",
                "stdout": "copy target output\n",
                "stderr": "",
                "status": 0,
                "success": true,
            })),
            metadata: None,
        }),
    ));
    app.activate_transcript_mouse_target(TranscriptMouseTarget::Tool {
        tool_call_id: "tc_shell_card_copy".to_string(),
    });
    app.transcript_view.selected_activity_index = 0;
    app
}

fn operator_sidebar_selection_test_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;
    app.ingest_event(envelope(
        1,
        "req_sidebar_copy",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_sidebar_copy".into(),
            provider_id: "default".to_string(),
            model_id: "model-sidebar".to_string(),
            prompt_summary: "sidebar copy".to_string(),
            request_digest: "digest-sidebar-copy".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_sidebar_copy",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_sidebar_todo".into(),
            tool_id: "todo.write".to_string(),
            args_summary: "update todo list".to_string(),
            args_digest: "digest-sidebar-todo-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_sidebar_copy",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_sidebar_todo".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("todo list updated".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "todos": [
                    {"content": "Copy sidebar task", "status": "in_progress", "priority": "high"},
                    {"content": "Keep existing sidebar clicks", "status": "pending", "priority": "medium"}
                ]
            })),
            metadata: None,
        }),
    ));
    app
}

fn transcript_selection_text_position(app: &AppState, needle: &str) -> (u16, u16) {
    let snapshot = transcript_selection_debug_snapshot(app, TEST_FRAME_AREA).unwrap_or_abort();
    for (row_idx, row) in snapshot.rows.iter().enumerate() {
        if let Some(byte_index) = row.find(needle) {
            let display_column = ratatui::text::Line::from(&row[..byte_index]).width();
            return (
                snapshot.viewport.x + u16::try_from(display_column).unwrap_or_abort(),
                snapshot.viewport.y + u16::try_from(row_idx).unwrap_or_abort(),
            );
        }
    }

    panic!("missing transcript text: {needle}");
}

fn transcript_selection_text_bounds(app: &AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row) = transcript_selection_text_position(app, needle);
    (
        column,
        row,
        u16::try_from(needle.chars().count()).unwrap_or_abort(),
    )
}

fn operator_sidebar_text_bounds(app: &AppState, needle: &str) -> (u16, u16, u16) {
    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let plan = FrameLayoutPlan::for_app(app, TEST_FRAME_AREA);
    let sidebar = plan
        .operator_sidebar
        .or(plan.details_overlay)
        .unwrap_or_abort();

    for y in sidebar.y..sidebar.bottom() {
        let row = (sidebar.x..sidebar.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (
                sidebar
                    .x
                    .saturating_add(u16::try_from(row[..column].chars().count()).unwrap_or_abort()),
                y,
                u16::try_from(needle.chars().count()).unwrap_or_abort(),
            );
        }
    }

    panic!("missing rendered text: {needle}");
}

fn drag_transcript_selection_range(app: &mut AppState, start: (u16, u16), end: (u16, u16)) {
    let (start_column, start_row) = start;
    let (end_column, end_row) = end;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: start_column,
            row: start_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: end_column,
            row: end_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: end_column,
            row: end_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
}

fn drag_transcript_selection(app: &mut AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row, width) = transcript_selection_text_bounds(app, needle);
    drag_transcript_selection_range(app, (column, row), (column + width.saturating_sub(1), row));
    (column, row, width)
}

fn drag_operator_sidebar_selection(app: &mut AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row, width) = operator_sidebar_text_bounds(app, needle);
    drag_transcript_selection_range(app, (column, row), (column + width.saturating_sub(1), row));
    (column, row, width)
}

fn run_started(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_run_started",
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".into(),
            workspace_root: "/tmp/workspace".to_string(),
        }),
    )
}

fn agent_spawned(seq: u64, agent_id: &str, profile: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: None,
        }),
    )
}

fn child_agent_spawned(
    seq: u64,
    agent_id: &str,
    profile: &str,
    parent_agent_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: Some(parent_agent_id.to_string()),
        }),
    )
}

fn provider_started(seq: u64, request_id: &str, provider: &str, model: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: provider.to_string(),
            model_id: model.to_string(),
            prompt_summary: "prompt summary".to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    )
}

fn shell_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    args_summary: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: "bash".to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{tool_call_id}-args"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("bash".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn shell_finished(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status,
            output_summary: Some("shell output summary".to_string()),
            output_digest: Some(format!("digest-{tool_call_id}-output")),
            output_json: Some(output_json),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("bash".to_string()),
                timing: Some(ExecutionTimingMetadata {
                    elapsed_ms: Some(250),
                    ..ExecutionTimingMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn shell_test_events(
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            "req_shell_panel",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_shell_panel".into(),
                text: "Run a shell command".to_string(),
            }),
        ),
        provider_started(2, "req_shell_panel", "default", "model-1"),
        shell_requested(
            3,
            "req_shell_panel",
            "tc_shell_panel",
            r#"{"command":"cargo test -p harness-tui","description":"run TUI tests"}"#,
        ),
        envelope(
            4,
            "req_shell_panel",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_panel".into(),
            }),
        ),
        shell_finished(5, "req_shell_panel", "tc_shell_panel", status, output_json),
    ]
}

fn shell_run_test_events(
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            "req_shell_run_panel",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_shell_run_panel".into(),
                text: "Run shell.run".to_string(),
            }),
        ),
        provider_started(2, "req_shell_run_panel", "default", "model-1"),
        envelope(
            3,
            "req_shell_run_panel",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_shell_run_panel".into(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"bash","args":["-lc","printf shell-run"],"cwd":"."}"#
                    .to_string(),
                args_digest: "digest-tc-shell-run-args".to_string(),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("shell.run".to_string()),
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            4,
            "req_shell_run_panel",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_run_panel".into(),
            }),
        ),
        envelope(
            5,
            "req_shell_run_panel",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_shell_run_panel".into(),
                status,
                output_summary: Some("shell-run".to_string()),
                output_digest: Some("digest-tc-shell-run-output".to_string()),
                output_json: Some(output_json),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("shell.run".to_string()),
                    timing: Some(ExecutionTimingMetadata {
                        elapsed_ms: Some(42),
                        ..ExecutionTimingMetadata::default()
                    }),
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
    ]
}

fn child_link_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    child_session_id: Option<&str>,
    parent_session_id: Option<&str>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: "agent.spawn".to_string(),
            args_summary: "{}".to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_session_id: parent_session_id.map(str::to_string),
                    child_session_id: child_session_id.map(str::to_string),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn child_task_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    child_session_id: &str,
    child_request_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some(tool_call_id.to_string()),
                    parent_request_id: Some(request_id.to_string()),
                    child_session_id: Some(child_session_id.to_string()),
                    child_request_id: Some(child_request_id.to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

#[cfg(test)]
#[path = "tests/permission_projection_tests.rs"]
mod permission_projection_tests;
#[cfg(test)]
#[path = "tests/terminal_panel_tests.rs"]
mod terminal_panel_tests;

delegate_test!(terminal_panel_is_hidden_by_default_and_toggles_from_keybinding => terminal_panel_tests::terminal_panel_is_hidden_by_default_and_toggles_from_keybinding);
delegate_test!(terminal_panel_stays_hidden_for_live_bash_until_explicit_toggle => terminal_panel_tests::terminal_panel_stays_hidden_for_live_bash_until_explicit_toggle);
delegate_test!(terminal_panel_ignores_non_interactive_bash_output => terminal_panel_tests::terminal_panel_ignores_non_interactive_bash_output);
delegate_test!(terminal_panel_extracts_explicit_interactive_pty_output => terminal_panel_tests::terminal_panel_extracts_explicit_interactive_pty_output);
delegate_test!(terminal_panel_renders_failed_command_stderr_and_exit_status => terminal_panel_tests::terminal_panel_renders_failed_command_stderr_and_exit_status);
delegate_test!(terminal_panel_extracts_shell_run_direct_command_schema => terminal_panel_tests::terminal_panel_extracts_shell_run_direct_command_schema);
delegate_test!(terminal_panel_replay_reconstructs_from_events_without_execution => terminal_panel_tests::terminal_panel_replay_reconstructs_from_events_without_execution);
delegate_test!(terminal_panel_focus_scrolls_independently_from_transcript => terminal_panel_tests::terminal_panel_focus_scrolls_independently_from_transcript);

#[cfg(test)]
#[path = "tests/permission_modal_tests.rs"]
mod permission_modal_tests;

delegate_test!(overlay_stack_orders_details_palette_permission => permission_modal_tests::overlay_stack_orders_details_palette_permission);
delegate_test!(overlay_stack_orders_permission_above_commands_and_slash => permission_modal_tests::overlay_stack_orders_permission_above_commands_and_slash);
delegate_test!(permission_modal_preempts_palette => permission_modal_tests::permission_modal_preempts_palette);
delegate_test!(permission_modal_ignores_unmapped_chars_without_buffering => permission_modal_tests::permission_modal_ignores_unmapped_chars_without_buffering);
delegate_test!(permission_modal_escape_parks_and_tab_restores_without_answering => permission_modal_tests::permission_modal_escape_parks_and_tab_restores_without_answering);
delegate_test!(permission_modal_tab_walks_rows_and_modified_tab_is_inert => permission_modal_tests::permission_modal_tab_walks_rows_and_modified_tab_is_inert);
delegate_test!(permission_modal_ctrl_c_cancels_after_escape_only_parks => permission_modal_tests::permission_modal_ctrl_c_cancels_after_escape_only_parks);
delegate_test!(permission_option_enter_accepts_every_modifier => permission_modal_tests::permission_option_enter_accepts_every_modifier);
delegate_test!(permission_modal_ctrl_n_emits_deny_intent_without_hiding_pending_permission => permission_modal_tests::permission_modal_ctrl_n_emits_deny_intent_without_hiding_pending_permission);
delegate_test!(question_permission_modal_collects_answers_and_emits_reason_payload => permission_modal_tests::question_permission_modal_collects_answers_and_emits_reason_payload);
delegate_test!(question_permission_modal_tabs_walk_rows_and_arrows_switch_questions => permission_modal_tests::question_permission_modal_tabs_walk_rows_and_arrows_switch_questions);
delegate_test!(question_option_enter_accepts_modifiers_while_modified_tab_is_inert => permission_modal_tests::question_option_enter_accepts_modifiers_while_modified_tab_is_inert);
delegate_test!(question_escape_ladder_clears_then_parks_and_cancel_chords_deny => permission_modal_tests::question_escape_ladder_clears_then_parks_and_cancel_chords_deny);
delegate_test!(question_modal_ignores_digits_past_visible_choices => permission_modal_tests::question_modal_ignores_digits_past_visible_choices);
delegate_test!(question_modal_multi_custom_answer_coexists_with_fixed_options => permission_modal_tests::question_modal_multi_custom_answer_coexists_with_fixed_options);
delegate_test!(question_text_enter_modifiers_route_commit_newline_or_inert => permission_modal_tests::question_text_enter_modifiers_route_commit_newline_or_inert);
delegate_test!(permission_queue_restores_only_original_focus_and_preserved_draft => permission_modal_tests::permission_queue_restores_only_original_focus_and_preserved_draft);
delegate_test!(question_ctrl_c_cancels_answered_questions => permission_modal_tests::question_ctrl_c_cancels_answered_questions);
delegate_test!(question_y_copies_the_focused_option_label_and_description => permission_modal_tests::question_y_copies_the_focused_option_label_and_description);
delegate_test!(permission_modal_allow_always_requests_durable_run_grant => permission_modal_tests::permission_modal_allow_always_requests_durable_run_grant);
delegate_test!(always_approve_mode_auto_allows_subsequent_non_question_permission => permission_modal_tests::always_approve_mode_auto_allows_subsequent_non_question_permission);
delegate_test!(always_approve_mode_appends_composer_badge_suffix => permission_modal_tests::always_approve_mode_appends_composer_badge_suffix);
delegate_test!(permission_modal_ctrl_o_opens_always_approve_confirm => permission_modal_tests::permission_modal_ctrl_o_opens_always_approve_confirm);
delegate_test!(permission_modal_allow_session_requests_session_grant => permission_modal_tests::permission_modal_allow_session_requests_session_grant);
delegate_test!(permission_modal_restores_focus_after_authoritative_resolution => permission_modal_tests::permission_modal_restores_focus_after_authoritative_resolution);

#[cfg(test)]
#[path = "tests/model_context_tests.rs"]
mod model_context_tests;

delegate_test!(runtime_context_labels_distinguish_live_continue_and_replay => model_context_tests::runtime_context_labels_distinguish_live_continue_and_replay);
delegate_test!(composer_metadata_omits_profile_and_keeps_model_and_source_labels => model_context_tests::composer_metadata_omits_profile_and_keeps_model_and_source_labels);
delegate_test!(composer_metadata_source_label_uses_provider_display_label_only => model_context_tests::composer_metadata_source_label_uses_provider_display_label_only);
delegate_test!(live_switch_model_labels_next_turn_only => model_context_tests::live_switch_model_labels_next_turn_only);
delegate_test!(control_tab_does_not_cycle_named_profiles => model_context_tests::control_tab_does_not_cycle_named_profiles);
delegate_test!(submitted_turn_omits_named_profile_badge => model_context_tests::submitted_turn_omits_named_profile_badge);

#[cfg(test)]
#[path = "tests/interaction_tests.rs"]
mod interaction_tests;

#[cfg(test)]
#[path = "tests/secondary_surface_ownership_tests.rs"]
mod secondary_surface_ownership_tests;

delegate_test!(secondary_surface_toggle_does_not_mutate_session_projection => secondary_surface_ownership_tests::secondary_surface_toggle_does_not_mutate_session_projection);
delegate_test!(replay_activities_unchanged_when_opening_closing_status_dialog => secondary_surface_ownership_tests::replay_activities_unchanged_when_opening_closing_status_dialog);
delegate_test!(status_dialog_visibility_is_owned_by_secondary_surface_state => secondary_surface_ownership_tests::status_dialog_visibility_is_owned_by_secondary_surface_state);
delegate_test!(status_dashboard_opens_via_action_and_palette_dispatch => secondary_surface_ownership_tests::status_dashboard_opens_via_action_and_palette_dispatch);
delegate_test!(status_dashboard_opens_via_dashboard_slash => secondary_surface_ownership_tests::status_dashboard_opens_via_dashboard_slash);
delegate_test!(status_dashboard_allows_normal_quit_sequence => secondary_surface_ownership_tests::status_dashboard_allows_normal_quit_sequence);
delegate_test!(status_dashboard_h_key_opens_help => secondary_surface_ownership_tests::status_dashboard_h_key_opens_help);
delegate_test!(status_dashboard_help_stays_open_on_down_key => secondary_surface_ownership_tests::status_dashboard_help_stays_open_on_down_key);
delegate_test!(status_dashboard_renders_empty_sections_from_app_state => secondary_surface_ownership_tests::status_dashboard_renders_empty_sections_from_app_state);
delegate_test!(status_dashboard_renders_populated_sections_from_app_state => secondary_surface_ownership_tests::status_dashboard_renders_populated_sections_from_app_state);
delegate_test!(status_dashboard_captures_and_restores_detached_transcript_anchor => secondary_surface_ownership_tests::status_dashboard_captures_and_restores_detached_transcript_anchor);

#[cfg(test)]
#[path = "tests/render_purity_tests.rs"]
mod render_purity_tests;

delegate_test!(repeated_projection_and_render_leaves_intent_queue_empty_and_projection_unchanged => render_purity_tests::repeated_projection_and_render_leaves_intent_queue_empty_and_projection_unchanged);
delegate_test!(repeated_replay_projection_and_render_is_side_effect_free => render_purity_tests::repeated_replay_projection_and_render_is_side_effect_free);
delegate_test!(pure_view_model_adapters_are_deterministic => render_purity_tests::pure_view_model_adapters_are_deterministic);

delegate_test!(space_on_transcript_focus_focuses_prompt_for_typing => interaction_tests::space_on_transcript_focus_focuses_prompt_for_typing);
delegate_test!(letter_on_transcript_focus_focuses_prompt_and_inserts_char => interaction_tests::letter_on_transcript_focus_focuses_prompt_and_inserts_char);
delegate_test!(focus_returns_after_palette_close => interaction_tests::focus_returns_after_palette_close);
delegate_test!(welcome_mouse_move_applies_hover_state_to_the_action_row => interaction_tests::welcome_mouse_move_applies_hover_state_to_the_action_row);
delegate_test!(welcome_mouse_move_away_clears_hover_state_and_row_surface => interaction_tests::welcome_mouse_move_away_clears_hover_state_and_row_surface);
delegate_test!(welcome_changelog_mouse_down_expands_the_startup_panel => interaction_tests::welcome_changelog_mouse_down_expands_the_startup_panel);
delegate_test!(welcome_changelog_expanded_mouse_down_opens_release_notes_and_up_is_inert => interaction_tests::welcome_changelog_expanded_mouse_down_opens_release_notes_and_up_is_inert);
delegate_test!(welcome_changelog_release_away_cancels_modal_activation => interaction_tests::welcome_changelog_release_away_cancels_modal_activation);
delegate_test!(welcome_changelog_drag_cancels_modal_activation => interaction_tests::welcome_changelog_drag_cancels_modal_activation);
delegate_test!(welcome_changelog_keyboard_activation_opens_modal_and_restores_focus => interaction_tests::welcome_changelog_keyboard_activation_opens_modal_and_restores_focus);
delegate_test!(welcome_changelog_mouse_down_preserves_pointer_hover_for_inline_preview => interaction_tests::welcome_changelog_mouse_down_preserves_pointer_hover_for_inline_preview);
delegate_test!(welcome_changelog_mouse_down_renders_a_bright_expanded_header => interaction_tests::welcome_changelog_mouse_down_renders_a_bright_expanded_header);
delegate_test!(welcome_changelog_pointer_move_away_restores_the_dim_header => interaction_tests::welcome_changelog_pointer_move_away_restores_the_dim_header);
delegate_test!(welcome_changelog_keyboard_activation_does_not_synthesize_pointer_hover => interaction_tests::welcome_changelog_keyboard_activation_does_not_synthesize_pointer_hover);
delegate_test!(welcome_changelog_click_brightens_the_compact_section_header => interaction_tests::welcome_changelog_click_brightens_the_compact_section_header);

delegate_test!(details_drawer_toggles_without_stealing_transcript_state => interaction_tests::details_drawer_toggles_without_stealing_transcript_state);

#[cfg(test)]
#[path = "tests/lifecycle_shell_tests.rs"]
mod lifecycle_shell_tests;

#[cfg(test)]
#[path = "tests/lifecycle_shell_part2_test.rs"]
mod lifecycle_shell_part2_test;

#[cfg(test)]
#[path = "tests/lifecycle_shell_part3_test.rs"]
mod lifecycle_shell_part3_test;

delegate_test!(config_backed_live_launch_starts_in_session_shell_without_details_drawer => lifecycle_shell_part2_test::config_backed_live_launch_starts_in_session_shell_without_details_drawer);

delegate_test!(mouse_wheel_scrolls_transcript_without_stealing_focus => interaction_tests::mouse_wheel_scrolls_transcript_without_stealing_focus);
delegate_test!(resize_invalidates_geometry_dependent_pointer_state => interaction_tests::resize_invalidates_geometry_dependent_pointer_state);
delegate_test!(pointer_drag_suppresses_stale_hover_feedback => interaction_tests::pointer_drag_suppresses_stale_hover_feedback);

delegate_test!(transcript_navigation_keys_match_scroll_expectations => interaction_tests::transcript_navigation_keys_match_scroll_expectations);
delegate_test!(detached_page_flip_reconciles_when_resize_reaches_bottom => interaction_tests::detached_page_flip_reconciles_when_resize_reaches_bottom);
delegate_test!(detached_page_flip_survives_resize_with_remaining_overflow => interaction_tests::detached_page_flip_survives_resize_with_remaining_overflow);
delegate_test!(active_stream_more_below_click_returns_to_live => interaction_tests::active_stream_more_below_click_returns_to_live);
delegate_test!(completed_stream_more_below_affordance_is_actionable => interaction_tests::completed_stream_more_below_affordance_is_actionable);
delegate_test!(detached_measured_viewport_has_no_stale_timeline_targets => interaction_tests::detached_measured_viewport_has_no_stale_timeline_targets);
delegate_test!(vanished_selection_anchor_stays_closed_through_mouse_up => interaction_tests::vanished_selection_anchor_stays_closed_through_mouse_up);
delegate_test!(selection_mouse_up_does_not_activate_underlying_tool_target => interaction_tests::selection_mouse_up_does_not_activate_underlying_tool_target);

delegate_test!(shift_right_left_on_details_focus_navigates_user_turns => interaction_tests::shift_right_left_on_details_focus_navigates_user_turns);

delegate_test!(page_up_down_with_prompt_focus_scrolls_transcript_without_clearing_draft => interaction_tests::page_up_down_with_prompt_focus_scrolls_transcript_without_clearing_draft);
delegate_test!(ctrl_up_down_with_prompt_focus_scrolls_transcript_by_one_row => interaction_tests::ctrl_up_down_with_prompt_focus_scrolls_transcript_by_one_row);

delegate_test!(shift_left_on_prompt_focus_still_selects_chars => interaction_tests::shift_left_on_prompt_focus_still_selects_chars);

delegate_test!(mouse_wheel_scrolls_inspector_when_hovered => interaction_tests::mouse_wheel_scrolls_inspector_when_hovered);

delegate_test!(mouse_wheel_ignores_non_scrollable_areas => interaction_tests::mouse_wheel_ignores_non_scrollable_areas);

delegate_test!(mouse_click_toggles_operator_sidebar_section_without_stealing_focus => interaction_tests::mouse_click_toggles_operator_sidebar_section_without_stealing_focus);

delegate_test!(edit_applied_auto_opens_modified_files_section => interaction_tests::edit_applied_auto_opens_modified_files_section);

delegate_test!(diff_hunk_navigation_advances_and_retreats_between_hunks => interaction_tests::diff_hunk_navigation_advances_and_retreats_between_hunks);

delegate_test!(dragging_transcript_scrollbar_updates_scroll_position => interaction_tests::dragging_transcript_scrollbar_updates_scroll_position);

delegate_test!(clicking_transcript_scrollbar_track_without_thumb_does_not_start_drag => interaction_tests::clicking_transcript_scrollbar_track_without_thumb_does_not_start_drag);
delegate_test!(identical_local_prompt_echoes_adopt_request_ids_in_submission_order => interaction_tests::identical_local_prompt_echoes_adopt_request_ids_in_submission_order);

#[cfg(test)]
#[path = "tests/transcript_selection_tests.rs"]
mod transcript_selection_tests;

#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_copies_transcript_text_and_clears_selection => transcript_selection_tests::mouse_drag_copy_on_select_copies_transcript_text_and_clears_selection);
#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_copies_shell_card_text => transcript_selection_tests::mouse_drag_copy_on_select_copies_shell_card_text);
#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_copies_operator_sidebar_text => transcript_selection_tests::mouse_drag_copy_on_select_copies_operator_sidebar_text);
delegate_test!(disabled_copy_on_select_keeps_operator_sidebar_selection_until_right_click_copy => transcript_selection_tests::disabled_copy_on_select_keeps_operator_sidebar_selection_until_right_click_copy);
delegate_test!(mouse_drag_copy_on_select_surfaces_error_toast_when_copy_fails => transcript_selection_tests::mouse_drag_copy_on_select_surfaces_error_toast_when_copy_fails);
delegate_test!(mouse_drag_copy_on_select_preserves_multiline_text_without_render_padding => transcript_selection_tests::mouse_drag_copy_on_select_preserves_multiline_text_without_render_padding);
delegate_test!(disabled_copy_on_select_keeps_selection_until_right_click_copy => transcript_selection_tests::disabled_copy_on_select_keeps_selection_until_right_click_copy);
delegate_test!(disabled_copy_on_select_supports_ctrl_c_and_escape => transcript_selection_tests::disabled_copy_on_select_supports_ctrl_c_and_escape);
delegate_test!(expanded_edit_ctrl_c_copies_canonical_unified_patches => transcript_selection_tests::expanded_edit_ctrl_c_copies_canonical_unified_patches);
#[cfg(not(windows))]
delegate_test!(mouse_drag_copy_on_select_keeps_body_rows_aligned_after_reasoning_gap => transcript_selection_tests::mouse_drag_copy_on_select_keeps_body_rows_aligned_after_reasoning_gap);
delegate_test!(transcript_selection_hit_testing_reuses_cached_snapshot_during_drag => transcript_selection_tests::transcript_selection_hit_testing_reuses_cached_snapshot_during_drag);
delegate_test!(transcript_selection_snapshot_preserves_user_card_marker => transcript_selection_tests::transcript_selection_snapshot_preserves_user_card_marker);
delegate_test!(mouse_wheel_does_not_build_transcript_selection_snapshot => transcript_selection_tests::mouse_wheel_does_not_build_transcript_selection_snapshot);
delegate_test!(transcript_selection_render_reuses_cached_snapshot => transcript_selection_tests::transcript_selection_render_reuses_cached_snapshot);
delegate_test!(transcript_selection_render_stays_aligned_after_large_reasoning_block => transcript_selection_tests::transcript_selection_render_stays_aligned_after_large_reasoning_block);
delegate_test!(transcript_render_key_is_cached_across_selection_drag_path => transcript_selection_tests::transcript_render_key_is_cached_across_selection_drag_path);
delegate_test!(transcript_render_key_reuses_cache_until_marked_dirty => transcript_selection_tests::transcript_render_key_reuses_cache_until_marked_dirty);

delegate_test!(historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit => lifecycle_shell_part2_test::historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit);

delegate_test!(historical_terminal_events_stay_in_session_shell_after_live_finish => lifecycle_shell_part2_test::historical_terminal_events_stay_in_session_shell_after_live_finish);

delegate_test!(continued_quiescent_bootstrap_stays_in_session_shell_without_handoff => lifecycle_shell_part2_test::continued_quiescent_bootstrap_stays_in_session_shell_without_handoff);

delegate_test!(startup_ctrl_w_empty_composer_requests_new_worktree_session => lifecycle_shell_part2_test::startup_ctrl_w_empty_composer_requests_new_worktree_session);
delegate_test!(startup_ctrl_w_with_draft_still_deletes_word => lifecycle_shell_part2_test::startup_ctrl_w_with_draft_still_deletes_word);
delegate_test!(palette_new_worktree_requests_new_worktree_session => lifecycle_shell_part2_test::palette_new_worktree_requests_new_worktree_session);
delegate_test!(startup_prompt_enter_echoes_prompt_and_selects_new_session => lifecycle_shell_part2_test::startup_prompt_enter_echoes_prompt_and_selects_new_session);

delegate_test!(slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit => lifecycle_shell_part2_test::slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit);

#[cfg(test)]
#[path = "tests/activity_lifecycle_tests.rs"]
mod activity_lifecycle_tests;

delegate_test!(provider_reasoning_delta_populates_thinking_stream_without_overwriting_answer_text => activity_lifecycle_tests::provider_reasoning_delta_populates_thinking_stream_without_overwriting_answer_text);

delegate_test!(provider_request_finished_total_tokens_populates_active_context_usage => activity_lifecycle_tests::provider_request_finished_total_tokens_populates_active_context_usage);
delegate_test!(provider_request_finished_prompt_tokens_prefer_active_context => activity_lifecycle_tests::provider_request_finished_prompt_tokens_prefer_active_context);

delegate_test!(provider_request_finished_without_usage_leaves_active_context_usage_none => activity_lifecycle_tests::provider_request_finished_without_usage_leaves_active_context_usage_none);

delegate_test!(provider_request_finished_keeps_activity_streaming_until_turn_task_completes => activity_lifecycle_tests::provider_request_finished_keeps_activity_streaming_until_turn_task_completes);

delegate_test!(cache_read_write_tokens_render_as_separate_status_labels => activity_lifecycle_tests::cache_read_write_tokens_render_as_separate_status_labels);

delegate_test!(task_cancelled_marks_matching_activity_as_error => activity_lifecycle_tests::task_cancelled_marks_matching_activity_as_error);

delegate_test!(provider_error_categories_surface_in_tui_activity_and_runtime_state => activity_lifecycle_tests::provider_error_categories_surface_in_tui_activity_and_runtime_state);

delegate_test!(child_tool_task_completed_does_not_finish_parent_turn_activity => activity_lifecycle_tests::child_tool_task_completed_does_not_finish_parent_turn_activity);

delegate_test!(child_tool_task_cancelled_does_not_mark_parent_turn_activity_error => activity_lifecycle_tests::child_tool_task_cancelled_does_not_mark_parent_turn_activity_error);

delegate_test!(terminal_only_turn_completion_scope_marks_activity_done_without_task_row => activity_lifecycle_tests::terminal_only_turn_completion_scope_marks_activity_done_without_task_row);

delegate_test!(terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row => activity_lifecycle_tests::terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row);

delegate_test!(terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state => activity_lifecycle_tests::terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state);

delegate_test!(replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row => activity_lifecycle_tests::terminal_replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row);

delegate_test!(replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row => activity_lifecycle_tests::terminal_replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row);

delegate_test!(replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state => activity_lifecycle_tests::terminal_replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state);

#[cfg(test)]
#[path = "tests/prompt_input_tests.rs"]
mod prompt_input_tests;

#[path = "tests/plan_view_tests.rs"]
mod plan_view_tests;
#[cfg(test)]
#[path = "tests/prompt_stash_tests.rs"]
mod prompt_stash_tests;
#[path = "tests/settings_editor_tests.rs"]
mod settings_editor_tests;

delegate_test!(ctrl_j_inserts_newline_without_submitting => prompt_input_tests::ctrl_j_inserts_newline_without_submitting);
delegate_test!(paste_multiline_text_inserts_newlines_without_submitting => prompt_input_tests::paste_multiline_text_inserts_newlines_without_submitting);
delegate_test!(multiline_history_keys_move_cursor_before_recalling_history => prompt_input_tests::multiline_history_keys_move_cursor_before_recalling_history);
delegate_test!(prompt_history_persists_and_restores_draft_after_recall => prompt_input_tests::prompt_history_persists_and_restores_draft_after_recall);
delegate_test!(startup_auto_submit_persists_prompt_history_once => prompt_input_tests::startup_auto_submit_persists_prompt_history_once);
delegate_test!(live_bootstrap_auto_submit_echoes_and_emits_first_prompt => prompt_input_tests::live_bootstrap_auto_submit_echoes_and_emits_first_prompt);
delegate_test!(startup_auto_submit_owns_status_over_empty_session_seed => prompt_input_tests::startup_auto_submit_owns_status_over_empty_session_seed);
delegate_test!(first_esc_on_nonempty_idle_prompt_shows_press_again_hint_without_clearing => prompt_input_tests::first_esc_on_nonempty_idle_prompt_shows_press_again_hint_without_clearing);
delegate_test!(second_esc_within_800ms_clears_prompt_and_saves_history => prompt_input_tests::second_esc_within_800ms_clears_prompt_and_saves_history);
delegate_test!(second_esc_after_800ms_restarts_clear_gesture_without_clearing => prompt_input_tests::second_esc_after_800ms_restarts_clear_gesture_without_clearing);
delegate_test!(replacing_prompt_after_first_esc_disarms_clear_confirmation => prompt_input_tests::replacing_prompt_after_first_esc_disarms_clear_confirmation);
delegate_test!(backspace_after_first_esc_disarms_clear_confirmation => prompt_input_tests::backspace_after_first_esc_disarms_clear_confirmation);
delegate_test!(delete_after_first_esc_disarms_clear_confirmation => prompt_input_tests::delete_after_first_esc_disarms_clear_confirmation);
delegate_test!(esc_while_turn_running_does_not_cancel_on_single_press => prompt_input_tests::esc_while_turn_running_does_not_cancel_on_single_press);
delegate_test!(double_esc_while_turn_running_does_not_emit_interrupt => prompt_input_tests::double_esc_while_turn_running_does_not_emit_interrupt);
delegate_test!(ctrl_c_clears_draft_then_cancels_running_turn => prompt_input_tests::ctrl_c_clears_draft_then_cancels_running_turn);
delegate_test!(submit_prompt_while_turn_streams_echoes_as_queued_and_emits_intent => prompt_input_tests::submit_prompt_while_turn_streams_echoes_as_queued_and_emits_intent);
delegate_test!(empty_enter_promotes_queued_prompt_during_sendable_wait => prompt_input_tests::empty_enter_promotes_queued_prompt_during_sendable_wait);
delegate_test!(empty_enter_does_not_promote_during_nonblocking_background_poll => prompt_input_tests::empty_enter_does_not_promote_during_nonblocking_background_poll);
delegate_test!(foreground_child_status_control_demotes_the_active_handle => live_turn_status_tests::foreground_child_status_control_demotes_the_active_handle);

delegate_test!(prompt_stash_push_clears_composer_and_persists_entry => prompt_stash_tests::prompt_stash_push_clears_composer_and_persists_entry);
delegate_test!(prompt_stash_pop_restores_text_cursor_and_selection => prompt_stash_tests::prompt_stash_pop_restores_text_cursor_and_selection);
delegate_test!(prompt_stash_pop_with_empty_stash_is_noop => prompt_stash_tests::prompt_stash_pop_with_empty_stash_is_noop);
delegate_test!(prompt_stash_push_with_empty_composer_is_noop => prompt_stash_tests::prompt_stash_push_with_empty_composer_is_noop);
delegate_test!(prompt_stash_list_dialog_opens_and_closes => prompt_stash_tests::prompt_stash_list_dialog_opens_and_closes);
delegate_test!(prompt_stash_list_dialog_renders_entries => prompt_stash_tests::prompt_stash_list_dialog_renders_entries);
delegate_test!(prompt_stash_list_delete_removes_selected_entry => prompt_stash_tests::prompt_stash_list_delete_removes_selected_entry);
delegate_test!(prompt_stash_list_restore_loads_selected_entry_to_composer => prompt_stash_tests::prompt_stash_list_restore_loads_selected_entry_to_composer);
delegate_test!(settings_editor_opens_and_lists_registry_rows => settings_editor_tests::settings_editor_opens_and_lists_registry_rows);
delegate_test!(settings_editor_navigates_and_closes_on_esc => settings_editor_tests::settings_editor_navigates_and_closes_on_esc);
delegate_test!(settings_slash_command_opens_settings_editor => settings_editor_tests::settings_slash_command_opens_settings_editor);
delegate_test!(settings_editor_toggles_hashline_edit_persists_and_reloads => settings_editor_tests::settings_editor_toggles_hashline_edit_persists_and_reloads);
delegate_test!(settings_editor_toggles_compaction_enabled_persists_and_reloads => settings_editor_tests::settings_editor_toggles_compaction_enabled_persists_and_reloads);
delegate_test!(settings_editor_fails_closed_for_secret_setting => settings_editor_tests::settings_editor_fails_closed_for_secret_setting);
delegate_test!(settings_editor_toggles_compaction_auto_retry_overflow_persists_and_reloads => settings_editor_tests::settings_editor_toggles_compaction_auto_retry_overflow_persists_and_reloads);
delegate_test!(settings_editor_summary_counts_bound_writable_paths => settings_editor_tests::settings_editor_summary_counts_bound_writable_paths);
delegate_test!(settings_editor_toggles_deterministic_enabled_persists_and_reloads => settings_editor_tests::settings_editor_toggles_deterministic_enabled_persists_and_reloads);
delegate_test!(settings_editor_toggles_compaction_structured_summary_contract_persists_and_reloads => settings_editor_tests::settings_editor_toggles_compaction_structured_summary_contract_persists_and_reloads);
delegate_test!(settings_editor_toggles_compaction_estimated_token_triggers_persists_and_reloads => settings_editor_tests::settings_editor_toggles_compaction_estimated_token_triggers_persists_and_reloads);
delegate_test!(settings_editor_e2e_open_edit_persist_and_read_effective => settings_editor_tests::settings_editor_e2e_open_edit_persist_and_read_effective);
delegate_test!(pending_settings_project_config_is_applied_on_new_live => settings_editor_tests::pending_settings_project_config_is_applied_on_new_live);
delegate_test!(plan_view_opens_from_action => plan_view_tests::plan_view_opens_from_action);
delegate_test!(plan_view_closes_on_esc => plan_view_tests::plan_view_closes_on_esc);
delegate_test!(context_view_plan_palette_dispatch_opens_plan_view => plan_view_tests::context_view_plan_palette_dispatch_opens_plan_view);
delegate_test!(session_feedback_maps_to_help_action => plan_view_tests::session_feedback_maps_to_help_action);
delegate_test!(plan_view_enter_opens_existing_plan_preview => plan_view_tests::plan_view_enter_opens_existing_plan_preview);
delegate_test!(plan_view_empty_state_enter_toasts_guidance => plan_view_tests::plan_view_empty_state_enter_toasts_guidance);
delegate_test!(plan_view_y_key_reports_clipboard_failure_without_dropping_path_banner => plan_view_tests::plan_view_y_key_reports_clipboard_failure_without_dropping_path_banner);
delegate_test!(plan_view_summary_counts_existing_and_preview => plan_view_tests::plan_view_summary_counts_existing_and_preview);
delegate_test!(plan_view_c_key_reports_clipboard_failure_for_body => plan_view_tests::plan_view_c_key_reports_clipboard_failure_for_body);
delegate_test!(plan_view_c_key_copies_plan_body => plan_view_tests::plan_view_c_key_copies_plan_body);
delegate_test!(plan_view_d_key_deletes_selected_plan => plan_view_tests::plan_view_d_key_deletes_selected_plan);
delegate_test!(plan_view_d_key_toasts_when_no_plans => plan_view_tests::plan_view_d_key_toasts_when_no_plans);
delegate_test!(plan_view_rows_and_summary_surface_byte_len => plan_view_tests::plan_view_rows_and_summary_surface_byte_len);
delegate_test!(plan_view_multi_plan_open_select_activate_product_path => plan_view_tests::plan_view_multi_plan_open_select_activate_product_path);
delegate_test!(prompt_stash_persists_across_session_restart => prompt_stash_tests::prompt_stash_persists_across_session_restart);
delegate_test!(queued_prompt_count_tracks_queued_activities => prompt_stash_tests::queued_prompt_count_tracks_queued_activities);
delegate_test!(queued_prompt_indicator_renders_when_count_positive => prompt_stash_tests::queued_prompt_indicator_renders_when_count_positive);

#[cfg(test)]
#[path = "tests/composer_editing_tests.rs"]
mod composer_editing_tests;

#[cfg(test)]
#[path = "tests/p0_04_composer_tests.rs"]
mod p0_04_composer_tests;

delegate_test!(move_word_left_skips_separators_then_word => composer_editing_tests::move_word_left_skips_separators_then_word);
delegate_test!(move_word_right_skips_word_then_separators => composer_editing_tests::move_word_right_skips_word_then_separators);
delegate_test!(move_word_left_at_start_stays_at_zero => composer_editing_tests::move_word_left_at_start_stays_at_zero);
delegate_test!(move_word_right_at_end_stays_at_end => composer_editing_tests::move_word_right_at_end_stays_at_end);
delegate_test!(move_word_left_handles_leading_separators => composer_editing_tests::move_word_left_handles_leading_separators);
delegate_test!(delete_word_backward_removes_word_and_pushes_undo => composer_editing_tests::delete_word_backward_removes_word_and_pushes_undo);
delegate_test!(delete_word_forward_removes_word_and_pushes_undo => composer_editing_tests::delete_word_forward_removes_word_and_pushes_undo);
delegate_test!(redo_re_applies_after_undo => composer_editing_tests::redo_re_applies_after_undo);
delegate_test!(undo_restores_selection_anchor => composer_editing_tests::undo_restores_selection_anchor);
delegate_test!(select_char_left_extends_selection => composer_editing_tests::select_char_left_extends_selection);
delegate_test!(select_word_right_extends_selection => composer_editing_tests::select_word_right_extends_selection);
delegate_test!(select_all_selects_entire_buffer => composer_editing_tests::select_all_selects_entire_buffer);
delegate_test!(select_line_selects_current_line => composer_editing_tests::select_line_selects_current_line);
delegate_test!(move_line_start_clears_selection => composer_editing_tests::move_line_start_clears_selection);
delegate_test!(move_line_end_clears_selection => composer_editing_tests::move_line_end_clears_selection);
delegate_test!(move_buffer_start_clears_selection => composer_editing_tests::move_buffer_start_clears_selection);
delegate_test!(move_buffer_end_clears_selection => composer_editing_tests::move_buffer_end_clears_selection);
delegate_test!(delete_line_removes_entire_line_including_newline => composer_editing_tests::delete_line_removes_entire_line_including_newline);
delegate_test!(kill_to_line_start_deletes_from_cursor_to_line_start => composer_editing_tests::kill_to_line_start_deletes_from_cursor_to_line_start);
delegate_test!(kill_to_line_end_deletes_from_cursor_to_line_end => composer_editing_tests::kill_to_line_end_deletes_from_cursor_to_line_end);
delegate_test!(typing_after_select_replaces_selection => composer_editing_tests::typing_after_select_replaces_selection);
delegate_test!(backspace_with_selection_deletes_selection => composer_editing_tests::backspace_with_selection_deletes_selection);
delegate_test!(undo_stack_caps_at_max_entries => composer_editing_tests::undo_stack_caps_at_max_entries);
delegate_test!(history_navigation_preserves_draft_via_undo => composer_editing_tests::history_navigation_preserves_draft_via_undo);
delegate_test!(cursor_left_clears_selection => composer_editing_tests::cursor_left_clears_selection);
delegate_test!(word_boundary_detects_punctuation_as_separator => composer_editing_tests::word_boundary_detects_punctuation_as_separator);

#[cfg(test)]
#[path = "tests/file_mention_tests.rs"]
mod file_mention_tests;

delegate_test!(typing_at_opens_file_mention_menu_with_directories => file_mention_tests::typing_at_opens_file_mention_menu_with_directories);
delegate_test!(file_mention_tab_expands_directory_without_closing_menu => file_mention_tests::file_mention_tab_expands_directory_without_closing_menu);
delegate_test!(file_mention_enter_inserts_selected_file_with_space => file_mention_tests::file_mention_enter_inserts_selected_file_with_space);
delegate_test!(file_mentions_use_injected_scanner_workspace_and_clock => file_mention_tests::file_mentions_use_injected_scanner_workspace_and_clock);
delegate_test!(submitting_selected_file_mention_emits_structured_file_part => file_mention_tests::submitting_selected_file_mention_emits_structured_file_part);
delegate_test!(file_mention_picker_excludes_primary_profiles_from_launch_metadata => file_mention_tests::file_mention_picker_excludes_primary_profiles_from_launch_metadata);
delegate_test!(file_mention_picker_selects_subagent_parts_from_launch_metadata => file_mention_tests::file_mention_picker_selects_subagent_parts_from_launch_metadata);
delegate_test!(file_mention_picker_selects_mcp_resource_parts_from_launch_metadata => file_mention_tests::file_mention_picker_selects_mcp_resource_parts_from_launch_metadata);
delegate_test!(file_mention_tag_is_removed_when_user_edits_inside_it => file_mention_tests::file_mention_tag_is_removed_when_user_edits_inside_it);

delegate_test!(queued_turn_schedule_keeps_activity_queued_until_provider_starts => activity_lifecycle_tests::terminal_queued_turn_schedule_keeps_activity_queued_until_provider_starts);

#[cfg(test)]
#[path = "tests/session_navigation_tests.rs"]
mod session_navigation_tests;

delegate_test!(parent_transcript_hides_child_prompt_before_task_tool_finishes => session_navigation_tests::parent_child_parent_transcript_hides_child_prompt_before_task_tool_finishes);

delegate_test!(replay_mode_focus_cycle_skips_prompt_and_blocks_draft_edits => session_navigation_tests::replay_mode_focus_cycle_skips_prompt_and_blocks_draft_edits);

delegate_test!(child_session_navigation_keybinds_follow_default_contract => session_navigation_tests::child_session_navigation_keybinds_follow_default_contract);

delegate_test!(replay_child_navigation_does_not_emit_live_intents => session_navigation_tests::replay_child_navigation_does_not_emit_live_intents);

delegate_test!(replay_handoff_parent_navigation_replays_non_resumable_parent_session => session_navigation_tests::replay_handoff_parent_navigation_replays_non_resumable_parent_session);

delegate_test!(task_child_navigation_opens_inline_subagent_view_without_child_run_dir => session_navigation_tests::task_child_navigation_opens_inline_subagent_view_without_child_run_dir);

delegate_test!(parent_child_navigation_ignores_nested_subagents_hidden_from_parent_transcript => session_navigation_tests::parent_child_navigation_ignores_nested_subagents_hidden_from_parent_transcript);

delegate_test!(live_inline_child_navigation_restores_live_parent_mode => session_navigation_tests::live_inline_child_navigation_restores_live_parent_mode);

delegate_test!(live_parent_events_update_parent_snapshot_while_inline_child_is_selected => session_navigation_tests::live_parent_events_update_parent_snapshot_while_inline_child_is_selected);

#[cfg(test)]
#[path = "tests/slash_menu_tests.rs"]
mod slash_menu_tests;

delegate_test!(slash_menu_closes_after_whitespace => slash_menu_tests::slash_menu_closes_after_whitespace);
delegate_test!(slash_menu_resets_selection_when_filter_changes => slash_menu_tests::slash_menu_resets_selection_when_filter_changes);
delegate_test!(slash_menu_ignores_url_and_escaped_tokens => slash_menu_tests::slash_menu_ignores_url_and_escaped_tokens);
delegate_test!(slash_menu_handles_unicode_query_deterministically => slash_menu_tests::slash_menu_handles_unicode_query_deterministically);
delegate_test!(slash_menu_matches_descriptions_and_boosts_prefixes => slash_menu_tests::slash_menu_matches_descriptions_and_boosts_prefixes);
delegate_test!(slash_alias_executes_matching_command_without_menu => slash_menu_tests::slash_alias_executes_matching_command_without_menu);
delegate_test!(slash_help_opens_help_surface_and_preserves_draft => slash_menu_tests::slash_help_opens_help_surface_and_preserves_draft);
delegate_test!(slash_escape_clears_token_or_restores_prior_draft => slash_menu_tests::slash_escape_clears_token_or_restores_prior_draft);
delegate_test!(slash_exit_matches_quit_requested_behavior => slash_menu_tests::slash_exit_matches_quit_requested_behavior);
delegate_test!(resume_history_surface_uses_meaningful_session_title => slash_menu_tests::resume_history_surface_uses_meaningful_session_title);
delegate_test!(live_session_picker_continue_quits_tui_and_emits_intent => slash_menu_tests::live_session_picker_continue_quits_tui_and_emits_intent);
delegate_test!(live_session_picker_replay_quits_tui_and_emits_intent => slash_menu_tests::live_session_picker_replay_quits_tui_and_emits_intent);
delegate_test!(slash_menu_supports_mouse_selection => slash_menu_tests::slash_menu_supports_mouse_selection);
delegate_test!(slash_menu_exposes_model_switcher_when_models_are_configured => slash_menu_tests::slash_menu_exposes_model_switcher_when_models_are_configured);
delegate_test!(rename_slash_command_availability_matches_mode => slash_menu_tests::rename_slash_command_availability_matches_mode);
delegate_test!(rename_slash_command_emits_update_session_title_intent => slash_menu_tests::rename_slash_command_emits_update_session_title_intent);
delegate_test!(slash_tab_accepts_command_without_executing => slash_menu_tests::slash_tab_accepts_command_without_executing);
delegate_test!(slash_mid_text_required_argument_executes_once_and_restores_draft => slash_menu_tests::slash_mid_text_required_argument_executes_once_and_restores_draft);
delegate_test!(slash_tab_replaces_the_full_command_token_from_mid_token_cursor => slash_menu_tests::slash_tab_replaces_the_full_command_token_from_mid_token_cursor);
delegate_test!(slash_enter_uses_required_arguments_after_the_cursor => slash_menu_tests::slash_enter_uses_required_arguments_after_the_cursor);
delegate_test!(rename_slash_empty_title_emits_error_toast => slash_menu_tests::rename_slash_empty_title_emits_error_toast);

delegate_test!(startup_mode_uses_pending_launch_metadata => lifecycle_shell_part2_test::startup_mode_uses_pending_launch_metadata);

delegate_test!(lifecycle_shell_state_transitions => lifecycle_shell_part2_test::lifecycle_shell_state_transitions);

delegate_test!(default_shell_registry_exposes_home_and_session_shell_only => lifecycle_shell_part2_test::default_shell_registry_exposes_home_and_session_shell_only);
delegate_test!(seed_operator_host_probes_sets_binary_update_and_jujutsu => lifecycle_shell_tests::seed_operator_host_probes_sets_binary_update_and_jujutsu);
delegate_test!(seed_operator_host_probes_binds_crash_scan_and_foreign_discover => lifecycle_shell_tests::seed_operator_host_probes_binds_crash_scan_and_foreign_discover);

delegate_test!(post_run_handoff_ignores_completed_turns_without_terminal_event => lifecycle_shell_part2_test::post_run_handoff_ignores_completed_turns_without_terminal_event);

delegate_test!(tool_task_completion_does_not_copy_tool_output_into_activity_transcript => activity_lifecycle_tests::terminal_tool_task_completion_does_not_copy_tool_output_into_activity_transcript);

delegate_test!(replay_mode_never_reports_lifecycle_shell_actions => lifecycle_shell_part2_test::replay_mode_never_reports_lifecycle_shell_actions);
