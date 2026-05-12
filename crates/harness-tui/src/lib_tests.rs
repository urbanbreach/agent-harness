use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::*;
use crossterm::event::KeyCode;
use harness_core::event::EventEnvelopeV1;

macro_rules! delegate_test {
    ($name:ident => $target:path) => {
        #[cfg(test)]
        #[test]
        fn $name() {
            $target();
        }
    };
}

delegate_test!(replay_mode_snapshot_renders_two_pane_layout => tests::module_replay_mode_snapshot_renders_two_pane_layout);
delegate_test!(transcript_edit_snapshot_renders_inline_diff => tests::module_transcript_edit_snapshot_renders_inline_diff);
delegate_test!(inline_diff_does_not_leave_large_gap_before_active_footer => tests::module_inline_diff_does_not_leave_large_gap_before_active_footer);
delegate_test!(transcript_edit_tool_wide_diff_uses_syntax_highlighting_and_split_palettes => tests::module_transcript_edit_tool_wide_diff_uses_syntax_highlighting_and_split_palettes);
delegate_test!(fenced_code_highlighting_uses_syntect_styles_for_known_languages => tests::module_fenced_code_highlighting_uses_syntect_styles_for_known_languages);
delegate_test!(fenced_code_highlighting_falls_back_to_plain_text_when_unknown => tests::module_fenced_code_highlighting_falls_back_to_plain_text_when_unknown);
delegate_test!(transcript_section_model_preserves_activity_order => ui::exact_test_transcript_section_model_preserves_activity_order);
delegate_test!(transcript_section_model_keeps_nested_tool_and_error_blocks => ui::exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks);
delegate_test!(transcript_reasoning_precedes_answer_and_tool_rows => ui::exact_test_transcript_reasoning_precedes_answer_and_tool_rows);
delegate_test!(latest_assistant_footer_stays_after_trailing_tool_rows => ui::exact_test_latest_assistant_footer_stays_after_trailing_tool_rows);
delegate_test!(transcript_tool_rows_follow_chronological_turn_order => ui::exact_test_transcript_tool_rows_follow_chronological_turn_order);
delegate_test!(transcript_applied_edit_missing_diff_surfaces_fallback => ui::exact_test_transcript_applied_edit_missing_diff_surfaces_fallback);
delegate_test!(transcript_edit_tool_matches_inline_diff_shape => ui::exact_test_transcript_edit_tool_matches_inline_diff_shape);
delegate_test!(transcript_native_edit_renders_inline_diff_from_artifact => ui::exact_test_transcript_native_edit_renders_inline_diff_from_artifact);
delegate_test!(transcript_harness_tool_progress_indicators => ui::exact_test_transcript_harness_tool_progress_indicators);
delegate_test!(transcript_apply_patch_multifile_uses_output_edit_paths => ui::exact_test_transcript_apply_patch_multifile_uses_output_edit_paths);
delegate_test!(subagent_footer_matches_harness_layout => ui::exact_test_subagent_footer_matches_harness_layout);
delegate_test!(subagent_replay_suppresses_parent_replay_dock => ui::exact_test_subagent_replay_suppresses_parent_replay_dock);
delegate_test!(transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs => ui::exact_test_transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs);
delegate_test!(transcript_inline_diff_stays_compact_between_tool_rows => ui::exact_test_transcript_inline_diff_stays_compact_between_tool_rows);
delegate_test!(transcript_proposed_edit_renders_header => ui::exact_test_transcript_proposed_edit_renders_header);
delegate_test!(transcript_rejected_edit_surfaces_reason_inline => ui::exact_test_transcript_rejected_edit_surfaces_reason_inline);
delegate_test!(transcript_follow_mode_uses_measured_surface_heights => ui::exact_test_transcript_follow_mode_uses_measured_surface_heights);
delegate_test!(transcript_scroll_offset_preserves_large_overflow => ui::exact_test_transcript_scroll_offset_preserves_large_overflow);
delegate_test!(visible_surface_lines_support_large_offsets => ui::exact_test_visible_surface_lines_support_large_offsets);
delegate_test!(native_tool_transcript_rows_show_disclosure_timestamps_and_task_metadata => ui::exact_test_native_tool_transcript_rows_show_disclosure_timestamps_and_task_metadata);
delegate_test!(mcp_tool_transcript_rows_use_effective_identity_without_generic_fallback => ui::exact_test_mcp_tool_transcript_rows_use_effective_identity_without_generic_fallback);
delegate_test!(generic_tool_successful_output_prefers_inline_background_rows => ui::exact_test_generic_tool_successful_output_prefers_inline_background_rows);
delegate_test!(lsp_tool_successful_output_stays_hidden_until_generic_output_enabled => ui::exact_test_lsp_tool_successful_output_stays_hidden_until_generic_output_enabled);
delegate_test!(todo_write_rows_render_open_checklist => ui::exact_test_todo_write_rows_render_open_checklist);
delegate_test!(transcript_task_rows_show_child_status_duration_and_counts => ui::exact_test_transcript_task_rows_show_child_status_duration_and_counts);
delegate_test!(block_tool_cards_skip_empty_subtitle_rows => ui::exact_test_block_tool_cards_skip_empty_subtitle_rows);
delegate_test!(inline_tool_rows_wrap_long_subtitles_cleanly => ui::exact_test_inline_tool_rows_wrap_long_subtitles_cleanly);
delegate_test!(transcript_pending_permission_stays_after_last_activity => ui::exact_test_transcript_pending_permission_stays_after_last_activity);

#[cfg(test)]
#[test]
fn transcript_turn_sections_render_open_rail_surfaces() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.activities = std::collections::VecDeque::from(vec![transcript_turn_group_test_activity(
        "req_turn_groups",
        app::ActivityStatus::Done,
        Some("Group these turns"),
        "Grouped response",
    )]);
    app.selected_activity_index = 0;
    app.follow_mode = false;
    app.transcript_scroll = usize::MAX;

    let rendered = render_live_lines(&app, 80, 24);
    let buffer = render_live_cells(&app, 80, 24);
    let theme = Theme::default();
    let lines = rendered.lines().collect::<Vec<_>>();
    let user_body = find_line_containing(&lines, "Group these turns")
        .unwrap_or_else(|| panic!("user body line\n{rendered}"));
    let assistant_body = find_line_containing_from(&lines, user_body + 1, "Grouped response")
        .unwrap_or_else(|| panic!("assistant body line\n{rendered}"));
    let assistant_footer = find_line_containing_from(&lines, assistant_body + 1, "Assistant")
        .unwrap_or_else(|| panic!("assistant footer\n{rendered}"));

    assert!(
        user_body < assistant_body,
        "assistant turn should remain ordered after the user turn content\n{rendered}"
    );
    assert!(assistant_body < assistant_footer);

    let user_body_rail = first_non_whitespace_column(lines[user_body]);
    let assistant_body_rail = first_non_whitespace_column(lines[assistant_body]);
    let user_body_column = first_alphanumeric_column(lines[user_body]);
    let assistant_body_column = first_alphanumeric_column(lines[assistant_body]);

    assert!(
        assistant_body_rail > user_body_rail,
        "assistant prose should sit on an inset canvas instead of reusing the user prompt rail\n{rendered}"
    );
    assert!(
        user_body_column.abs_diff(assistant_body_column) <= 1,
        "top-level turn bodies should stay nearly aligned even after prompt padding changes\n{rendered}"
    );
    assert_eq!(
        user_body_column.saturating_sub(user_body_rail),
        3,
        "user message text should keep the shell's single rail plus two-column left padding\n{rendered}"
    );
    assert!(
        user_body > 0
            && lines[user_body - 1].contains('┃')
            && !lines[user_body - 1].contains("You"),
        "user message should use the shell top padding without a synthetic header label\n{rendered}"
    );
    let (user_body_row, user_body_fgs, user_body_bgs) =
        row_at(&buffer, 80, user_body).expect("user body palette row");
    let (assistant_footer_row, assistant_footer_fgs, assistant_footer_bgs) =
        row_at(&buffer, 80, assistant_footer).expect("assistant footer palette row");
    let user_rail_column = user_body_row.find('┃').expect("user rail");
    assert_eq!(user_body_fgs[user_rail_column], theme.agent_accent("build"));

    let mut plan_app = app::AppState::new_live(None, false, None);
    plan_app.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "plan",
        "default:gpt-5.4-mini",
    ));
    plan_app.activities =
        std::collections::VecDeque::from(vec![transcript_turn_group_test_activity(
            "req_plan_turn_groups",
            app::ActivityStatus::Done,
            Some("Plan this work"),
            "Planned response",
        )]);
    plan_app.selected_activity_index = 0;
    plan_app.follow_mode = false;
    plan_app.transcript_scroll = usize::MAX;

    let plan_rendered = render_live_lines(&plan_app, 80, 24);
    let plan_lines = plan_rendered.lines().collect::<Vec<_>>();
    let plan_user_body = find_line_containing(&plan_lines, "Plan this work")
        .unwrap_or_else(|| panic!("plan user body line\n{plan_rendered}"));
    let (plan_user_body_row, plan_user_body_fgs, _) =
        row_at(&render_live_cells(&plan_app, 80, 24), 80, plan_user_body)
            .expect("plan user body palette row");
    let plan_user_rail_column = plan_user_body_row.find('┃').expect("plan user rail");
    assert_eq!(
        plan_user_body_fgs[plan_user_rail_column],
        theme.agent_accent("plan")
    );
    assert!(!assistant_footer_row.contains('┃'));
    assert_eq!(
        assistant_footer_fgs[first_alphanumeric_column(lines[assistant_footer])],
        theme.text.primary
    );
    assert!(user_body_bgs[user_body_column..user_body_column + 4]
        .iter()
        .all(|color| *color == theme.surface.panel));
    assert!(
        assistant_footer_bgs[assistant_body_column..assistant_body_column + 9]
            .iter()
            .all(|color| *color == theme.surface.shell)
    );
    assert!(
        assistant_body - user_body <= 3,
        "turn stacking should stay compact\n{rendered}"
    );
    assert!(!rendered.contains('╭') && !rendered.contains('╰') && !rendered.contains('│'));

    let mut follow_app = app::AppState::new_live(None, false, None);
    follow_app.activities = std::collections::VecDeque::from(
        (0..8)
            .map(|index| {
                transcript_turn_group_test_activity(
                    &format!("request-{index}"),
                    app::ActivityStatus::Done,
                    Some(&format!("question {index}")),
                    &format!("reply {index}"),
                )
            })
            .collect::<Vec<_>>(),
    );
    follow_app.selected_activity_index = 7;
    follow_app.follow_mode = true;

    let followed = render_live_lines(&follow_app, 60, 18);
    assert!(
        followed.contains("question 7") && followed.contains("reply 7"),
        "follow mode should keep the newest grouped turn visible\n{followed}"
    );
    assert!(
        !followed.contains("question 0"),
        "follow mode should scroll past the earliest grouped turn\n{followed}"
    );

    follow_app.follow_mode = false;
    follow_app.transcript_scroll = usize::MAX;

    let scrolled_back = render_live_lines(&follow_app, 60, 18);
    assert!(
        scrolled_back.contains("question 0") && scrolled_back.contains("reply 0"),
        "scroll-back should still surface the earliest grouped turn\n{scrolled_back}"
    );
    assert!(
        !scrolled_back.contains("question 7"),
        "scroll-back should stop following the newest grouped turn\n{scrolled_back}"
    );
}

#[cfg(test)]
#[test]
fn transcript_turn_sections_keep_nested_tool_details() {
    let mut app = app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    app.active_tab = app::Tab::Run;
    let mut activity = transcript_turn_group_test_activity(
        "req_nested_tool_details",
        app::ActivityStatus::Error,
        Some("Inspect nested details"),
        "Assistant body",
    );
    activity.thinking_text = "tool planning".to_string();
    activity.error_message = Some("tool call failed".to_string());
    activity.tool_calls.push(app::ToolCallEntry {
        tool_call_id: "call-1".to_string(),
        tool_id: "shell.run".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"cmd":"false"}"#.to_string(),
        args_digest: "digest-1".to_string(),
        lifecycle_state: None,
        status: app::ToolCallDisplayStatus::Failed,
        output_summary: Some("command failed".to_string()),
        output_digest: Some("digest-out".to_string()),
        output_json: None,
        truncated_output: Some("command failed".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;
    app.transcript_scroll = usize::MAX;

    let rendered = render_live_lines(&app, 100, 24);
    let buffer = render_live_cells(&app, 100, 24);
    let theme = Theme::default();
    let lines = rendered.lines().collect::<Vec<_>>();
    let reasoning_row = find_line_containing(&lines, "tool planning")
        .unwrap_or_else(|| panic!("reasoning row\n{rendered}"));
    let body_row = find_line_containing(&lines, "Assistant body")
        .unwrap_or_else(|| panic!("assistant body row\n{rendered}"));
    let tool_row = find_line_containing_all_from(&lines, body_row + 1, &["false"])
        .unwrap_or_else(|| panic!("tool row\n{rendered}"));
    let error_row = find_line_containing_from(&lines, tool_row + 1, "tool call failed")
        .unwrap_or_else(|| panic!("tool error row\n{rendered}"));
    let assistant_footer = find_line_containing_from(&lines, error_row + 1, "Assistant")
        .unwrap_or_else(|| panic!("assistant footer\n{rendered}"));

    assert!(reasoning_row < body_row);
    assert!(body_row >= reasoning_row + 2);
    assert!(body_row < tool_row);
    assert!(tool_row < error_row);
    assert!(error_row < assistant_footer);

    let assistant_body_column = first_alphanumeric_column(lines[body_row]);
    let assistant_body_rail = first_non_whitespace_column(lines[body_row]);
    let assistant_footer_column = first_alphanumeric_column(lines[assistant_footer]);
    let (reasoning_row_text, reasoning_row_fgs, _) =
        row_at(&buffer, 100, reasoning_row).expect("reasoning palette row");
    let reasoning_rail_column = reasoning_row_text.find('┃').expect("reasoning rail");
    let thinking_body_start = reasoning_row_text[..reasoning_row_text
        .find("tool planning")
        .expect("thinking body start")]
        .chars()
        .count();

    assert!(reasoning_row_text.contains("tool planning"));
    assert!(
        first_alphanumeric_column(lines[reasoning_row]) == assistant_body_column,
        "thinking label should align with the assistant body column while keeping its own rail\n{rendered}"
    );
    assert_eq!(
        reasoning_row_fgs[reasoning_rail_column], theme.border.subtle,
        "thinking rail should use the subtle border color\n{rendered}"
    );
    assert!(
        reasoning_row_fgs
            [thinking_body_start..thinking_body_start + "tool planning".chars().count()]
            .iter()
            .all(|color| *color == theme.text.secondary),
        "thinking body should stay muted like the shell\n{rendered}"
    );
    let nested_detail_columns = [tool_row, error_row]
        .into_iter()
        .map(|row| first_alphanumeric_column(lines[row]))
        .collect::<Vec<_>>();

    assert!(assistant_footer_column >= assistant_body_rail);
    assert!(
        nested_detail_columns
            .iter()
            .all(|column| *column > assistant_body_column),
        "nested tool details and error rows should remain deeper than the assistant body rail\n{rendered}"
    );
}

delegate_test!(operator_rail_section_model_builds_pinned_summary => ui::exact_test_operator_rail_section_model_builds_pinned_summary);
delegate_test!(compaction_applied_updates_active_context_usage_estimate => ui::exact_test_compaction_applied_updates_active_context_usage_estimate);
delegate_test!(operator_rail_sanitizes_control_chars_in_sidebar_strings => ui::exact_test_operator_rail_sanitizes_control_chars_in_sidebar_strings);
delegate_test!(operator_rail_section_model_hides_empty_sources_but_preserves_order => ui::exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order);
delegate_test!(operator_rail_section_model_counts_generic_mcp_activity => ui::exact_test_operator_rail_section_model_counts_generic_mcp_activity);
delegate_test!(operator_rail_section_model_separates_mcp_from_native_tool_activity => ui::exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity);
delegate_test!(operator_rail_section_model_uses_runtime_mcp_activity_without_config => ui::exact_test_operator_rail_section_model_uses_runtime_mcp_activity_without_config);
delegate_test!(operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp => ui::exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp);
delegate_test!(operator_rail_matches_sidebar_text_styles => ui::exact_test_operator_rail_matches_sidebar_text_styles);
delegate_test!(operator_rail_uses_generated_session_title => ui::exact_test_operator_rail_uses_generated_session_title);
delegate_test!(operator_rail_renders_todo_items_from_tool_state => ui::exact_test_operator_rail_renders_todo_items_from_tool_state);
delegate_test!(operator_rail_renders_todo_items_from_artifact_state => ui::exact_test_operator_rail_renders_todo_items_from_artifact_state);
delegate_test!(operator_rail_renders_subagent_rows_from_orchestration_state => ui::exact_test_operator_rail_renders_subagent_rows_from_orchestration_state);
delegate_test!(operator_rail_keeps_subagents_visible_in_replay => ui::exact_test_operator_rail_keeps_subagents_visible_in_replay);
delegate_test!(operator_rail_keeps_completed_todo_state_visible => ui::exact_test_operator_rail_keeps_completed_todo_state_visible);
delegate_test!(operator_rail_collapses_todo_section_body => ui::exact_test_operator_rail_collapses_todo_section_body);
delegate_test!(operator_rail_collapses_modified_files_section_body => ui::exact_test_operator_rail_collapses_modified_files_section_body);
delegate_test!(operator_sidebar_hit_target_maps_section_headers => ui::exact_test_operator_sidebar_hit_target_maps_section_headers);

#[cfg(test)]
#[test]
fn operator_sidebar_pins_summary_and_hides_empty_sections() {
    ui::exact_test_operator_rail_low_activity_presentation_prefers_primary_stack();
    ui::exact_test_operator_rail_section_model_builds_pinned_summary();
    ui::exact_test_operator_rail_sanitizes_control_chars_in_sidebar_strings();
    ui::exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order();
    ui::exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity();
    ui::exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp();
    ui::exact_test_operator_rail_section_model_surfaces_pending_permissions_first();
}

#[cfg(test)]
#[test]
fn operator_sidebar_compact_empty_mode_preserves_anchor_copy_with_fixed_width() {
    harness_core::config::set_registered_integrations_config(
        harness_core::config::IntegrationsConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let live_empty = operator_sidebar_empty_live_app();
    let replay_empty = app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    let live_populated = operator_sidebar_todo_live_app();
    let replay_populated = operator_sidebar_todo_replay_app();

    let live_empty_plan =
        layout::FrameLayoutPlan::for_app(&live_empty, ratatui::layout::Rect::new(0, 0, 160, 30));
    let replay_empty_plan =
        layout::FrameLayoutPlan::for_app(&replay_empty, ratatui::layout::Rect::new(0, 0, 100, 30));
    let live_populated_plan = layout::FrameLayoutPlan::for_app(
        &live_populated,
        ratatui::layout::Rect::new(0, 0, 160, 30),
    );
    let replay_populated_plan = layout::FrameLayoutPlan::for_app(
        &replay_populated,
        ratatui::layout::Rect::new(0, 0, 100, 30),
    );

    let live_empty_sidebar = live_empty_plan
        .operator_sidebar
        .expect("live compact sidebar");
    let replay_empty_sidebar = replay_empty_plan
        .operator_sidebar
        .expect("replay compact sidebar");

    assert_eq!(
        live_empty_sidebar.width,
        live_populated_plan
            .operator_sidebar
            .expect("live populated sidebar")
            .width
    );
    assert_eq!(
        replay_empty_sidebar.width,
        replay_populated_plan
            .operator_sidebar
            .expect("replay populated sidebar")
            .width
    );
    for (label, app) in [("live", &live_empty), ("replay", &replay_empty)] {
        let sidebar = operator_sidebar_text(app);
        let has_mcp_state = sidebar.contains("No MCP integrations configured")
            || sidebar.contains("No MCP servers configured")
            || sidebar.contains("websearch Disconnected");
        let has_lsp_state =
            sidebar.contains("No active LSP servers") || sidebar.contains("LSP disabled");

        assert!(
            sidebar.contains("▼ MCP")
                && has_mcp_state
                && sidebar.contains("▼ LSP")
                && has_lsp_state
                && sidebar.contains("▶ Modified Files")
                && !sidebar.contains("No modified files"),
            "{label} compact rail should preserve anchor copy"
        );
    }
}

#[cfg(test)]
#[test]
fn operator_sidebar_width_stays_fixed_when_todo_or_modified_files_exist() {
    let live_empty_width = layout::FrameLayoutPlan::for_app(
        &operator_sidebar_empty_live_app(),
        ratatui::layout::Rect::new(0, 0, 160, 30),
    )
    .operator_sidebar
    .expect("live compact sidebar")
    .width;
    let replay_empty_width = layout::FrameLayoutPlan::for_app(
        &app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new()),
        ratatui::layout::Rect::new(0, 0, 100, 30),
    )
    .operator_sidebar
    .expect("replay compact sidebar")
    .width;

    assert_operator_sidebar_expanded(
        &operator_sidebar_todo_live_app(),
        "▶ Modified Files",
        "Explain the refactor",
        live_empty_width,
    );
    assert_operator_sidebar_expanded(
        &operator_sidebar_modified_files_live_app(),
        "▼ Modified Files",
        "src/ui_secondary.rs",
        live_empty_width,
    );
    assert_operator_sidebar_expanded(
        &operator_sidebar_todo_replay_app(),
        "▶ Modified Files",
        "Explain the refactor",
        replay_empty_width,
    );
    assert_operator_sidebar_expanded(
        &operator_sidebar_modified_files_replay_app(),
        "▼ Modified Files",
        "src/ui_secondary.rs",
        replay_empty_width,
    );
}

delegate_test!(persistent_operator_sidebar_uses_panel_gutter_instead_of_border_line => ui::exact_test_persistent_operator_sidebar_uses_panel_gutter);
delegate_test!(control_dock_view_model_handles_live_runtime_variants => view_model::exact_test_control_dock_view_model_handles_live_runtime_variants);
delegate_test!(control_dock_view_model_preserves_replay_read_only_variant => view_model::exact_test_control_dock_view_model_preserves_replay_read_only_variant);
delegate_test!(tool_runtime_state_uses_effective_tool_identity => view_model::exact_test_tool_runtime_state_uses_effective_tool_identity);
delegate_test!(startup_shell_keeps_no_default_tab_chrome_after_runtime_context_addition => ui::exact_test_startup_shell_keeps_no_default_tab_chrome_after_runtime_context_addition);
delegate_test!(replay_prompt_pane_is_visibly_read_only => ui::exact_test_replay_prompt_pane_is_visibly_read_only);
delegate_test!(live_control_dock_renders_shared_surface => ui::exact_test_live_control_dock_renders_shared_surface);
delegate_test!(live_control_dock_collapses_disclosure_before_status => ui::exact_test_live_control_dock_collapses_disclosure_before_status);
delegate_test!(live_composer_metadata_omits_success_without_variant => ui::exact_test_live_composer_metadata_omits_success_without_variant);
delegate_test!(live_composer_reserves_right_gap => ui::exact_test_live_composer_reserves_right_gap);
delegate_test!(live_composer_disclosure_summarizes_compaction_metrics => ui::exact_test_live_composer_disclosure_summarizes_compaction_metrics);
delegate_test!(startup_disclosure_matches_harness_hint_row => ui::exact_test_startup_disclosure_matches_harness_hint_row);
delegate_test!(composer_viewport_wraps_by_display_width => ui::exact_test_composer_viewport_wraps_by_display_width);
delegate_test!(composer_viewport_wraps_at_word_boundaries => ui::exact_test_composer_viewport_wraps_at_word_boundaries);
delegate_test!(tool_status_summary_uses_effective_tool_identity => ui::exact_test_tool_status_summary_uses_effective_tool_identity);
delegate_test!(wheel_target_hits_transcript_when_hovered => ui::exact_test_wheel_target_hits_transcript_when_hovered);
delegate_test!(wheel_target_hits_inspector_inside_live_overlay => ui::exact_test_wheel_target_hits_inspector_inside_live_overlay);
delegate_test!(wheel_target_excludes_activity_portion_of_live_overlay => ui::exact_test_wheel_target_excludes_activity_portion_of_live_overlay);
delegate_test!(compact_operator_rail_does_not_capture_wheel => ui::exact_test_compact_operator_rail_does_not_capture_wheel);
delegate_test!(compact_operator_rail_skips_focus_cycle => app::exact_test_compact_operator_rail_skips_focus_cycle);
delegate_test!(diff_renderer_uses_stacked_layout_in_narrow_geometries => tests::module_diff_renderer_uses_stacked_layout_in_narrow_geometries);
delegate_test!(wide_diff_renderer_pairs_before_and_after_columns => tests::module_wide_diff_renderer_pairs_before_and_after_columns);
delegate_test!(startup_slash_commands_execute_without_menu => app::exact_test_startup_slash_commands_execute_without_menu);
delegate_test!(slash_new_preserves_draft_and_returns_home => app::exact_test_slash_new_preserves_draft_and_returns_home);
delegate_test!(replay_mode_disables_slash_workflow => app::exact_test_replay_mode_disables_slash_workflow);
delegate_test!(slash_replay_opens_history_and_restores_draft => app::exact_test_slash_replay_opens_history_and_restores_draft);
delegate_test!(slash_resume_opens_history_and_restores_draft => app::exact_test_slash_resume_opens_history_and_restores_draft);
delegate_test!(slash_events_opens_review_surface => app::exact_test_slash_events_opens_review_surface);
delegate_test!(slash_status_opens_status_dialog_and_restores_draft => app::exact_test_slash_status_opens_status_dialog_and_restores_draft);
delegate_test!(status_dialog_mcp_rows_match_harness_states => ui::exact_test_status_dialog_mcp_rows_match_harness_states);
delegate_test!(status_dialog_render_snapshot_covers_harness_sections => ui::exact_test_status_dialog_render_snapshot_covers_harness_sections);
delegate_test!(slash_shell_closes_review_surface => app::exact_test_slash_shell_closes_review_surface);
delegate_test!(slash_follow_toggles_follow_mode => app::exact_test_slash_follow_toggles_follow_mode);
delegate_test!(live_slash_compact_appears_when_supported => app::exact_test_live_slash_compact_appears_when_supported);
delegate_test!(live_slash_compact_emits_ui_intent => app::exact_test_live_slash_compact_emits_ui_intent);
delegate_test!(live_without_compact_support_hides_slash_compact => app::exact_test_live_without_compact_support_hides_slash_compact);
delegate_test!(slash_menu_lists_lineage_commands => app::exact_test_slash_menu_lists_lineage_commands);
delegate_test!(slash_lineage_write_commands_blocked_in_replay => app::exact_test_slash_lineage_write_commands_blocked_in_replay);
delegate_test!(slash_lineage_write_commands_blocked_when_live_unstable => app::exact_test_slash_lineage_write_commands_blocked_when_live_unstable);
delegate_test!(slash_lineage_descriptions_use_harness_branding => app::exact_test_slash_lineage_descriptions_use_harness_branding);

#[cfg(test)]
#[test]
fn replay_mode_never_reports_lifecycle_shell_actions() {
    let replay = app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            Some("req_replay_terminal"),
            harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(
        replay.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(!replay.lifecycle_shell_actions_visible());
}

#[cfg(test)]
#[test]
fn permission_modal_preempts_palette_and_slash() {
    let mut palette_app = app::AppState::new_live(None, false, None);
    palette_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    palette_app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    assert!(palette_app.palette_visible);

    palette_app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_palette_and_slash",
        "tool_call_preempt_palette_and_slash",
    ));
    palette_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    let palette_render = render_live_lines(&palette_app, 100, 24);
    assert!(palette_render.contains("Permission required"));
    assert!(!palette_render.contains("Commands"));
    assert!(!palette_app.palette_visible);
    assert_eq!(
        palette_app.overlay_stack().ordered(),
        &[overlay::OverlayKind::PermissionModal]
    );

    let mut slash_app = app::AppState::new_live(None, false, None);
    slash_app.handle_key(key(crossterm::event::KeyCode::Char('/')));
    assert!(slash_app.slash_visible);

    slash_app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_slash",
        "tool_call_preempt_slash",
    ));
    slash_app.handle_key(key(crossterm::event::KeyCode::Char('/')));

    let slash_render = render_live_lines(&slash_app, 100, 24);
    assert!(slash_render.contains("Permission required"));
    assert!(!slash_render.contains("Slash commands"));
    assert_eq!(slash_app.prompt_buffer, "/");
    assert!(!slash_app.slash_visible);
    assert_eq!(
        slash_app.overlay_stack().ordered(),
        &[overlay::OverlayKind::PermissionModal]
    );
}

#[cfg(test)]
#[test]
fn completed_sessions_show_inline_completion_state_instead_of_handoff_card() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.ingest_event(envelope(
        1,
        Some("req_completed_inline"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 160, 48);

    assert!(app.completed_session_shell_active());
    assert!(!app.post_run_handoff_visible());
    assert!(rendered.contains("Tab focus"));
    assert!(rendered.contains("Ctrl+p commands"));
    assert!(rendered.contains("q quit"));
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
}

#[cfg(test)]
#[test]
fn live_shell_uses_single_chrome_path() {
    let ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 24, None, None, "Ctrl+p commands");

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_single_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    assert_live_shell_document_composer_contract(&completed, 100, 24, None, None, "Tab focus");

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    assert_live_shell_document_composer_contract(&degraded, 100, 24, None, None, "Degraded");
}

#[cfg(test)]
#[test]
fn live_shell_status_strip_has_single_priority_order() {
    let mut orchestration = orchestration_status_strip_fixture();
    orchestration.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );
    let mut theme = Theme::default();
    theme.live_shell.primary.details_sidebar_width = 12;
    theme.live_shell.primary.content_margin_x = 2;
    orchestration.set_theme_for_test(theme);

    let orchestration_render = render_live_lines(&orchestration, 140, 40);

    assert!(
        orchestration_render.contains("Current runtime:")
            || orchestration_render.contains("Launch:")
    );

    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );
    app.set_theme_for_test(theme);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let rendered = render_live_lines(&app, 140, 40);

    assert!(rendered.contains("Ctrl+p commands"));
    assert!(!rendered.contains("Enter send"));
    assert!(!rendered.contains("tool finished"));
    assert!(!rendered.contains("turn 1"));
    assert!(!rendered.contains("ready for next turn"));
}

#[cfg(test)]
#[test]
fn live_shell_footer_is_shortcuts_only() {
    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("continued"),
    );

    let primary_render = render_live_lines(&live, 100, 30);
    assert!(!primary_render.contains("q quit"));
    assert!(primary_render.contains("Ctrl+p commands"));

    let reduced_render = render_live_lines(&live, 80, 24);
    assert!(!reduced_render.contains("q quit"));
    assert!(reduced_render.contains("Ctrl+p commands"));

    let minimal_render = render_live_lines(&live, 60, 18);
    assert!(!minimal_render.contains("q quit"));
    assert!(minimal_render.contains("Ctrl+p commands"));

    let replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    let replay_render = render_live_lines(&replay, 100, 24);
    let replay_lines = replay_render.lines().collect::<Vec<_>>();
    let replay_footer_row = find_last_line_containing(&replay_lines, "q quit")
        .map(|row| replay_lines[row].trim_end().to_string())
        .expect("replay footer row");
    assert_markers_in_order(
        &replay_footer_row,
        &["? shortcuts", "tab focus", "r reload", "q quit"],
    );
    assert!(!replay_footer_row.contains("Replay"));
    assert!(!replay_footer_row.contains("run_fixture"));
    assert!(!replay_footer_row.contains("/tmp/replay-session"));
    assert!(!replay_footer_row.contains("/status"));
}

#[cfg(test)]
#[test]
fn primary_and_wide_live_shells_hide_metadata_header() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );
    for event in session_view_events() {
        app.ingest_event(event);
    }

    for (width, height) in [(100, 30), (160, 48)] {
        let plan = FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, width, height));
        assert_eq!(
            plan.header.height, 0,
            "live shell header should stay hidden at {width}x{height}"
        );
        assert!(plan.live_anchor.is_none());

        let rendered = render_live_lines(&app, width, height);
        assert!(
            !rendered.contains("Composer ·"),
            "wide live shells should not reintroduce composer label chrome\n{rendered}"
        );
        assert!(
            !rendered
                .lines()
                .next()
                .unwrap_or_default()
                .contains("run run_fixture"),
            "wide live shells should not surface the old top identity bar\n{rendered}"
        );
    }
}

#[cfg(test)]
#[test]
fn completed_shell_bottom_rows_do_not_duplicate_command_help_footers() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.active_review_surface = Some(app::ReviewSurface::Events);
    app.focus = app::Focus::Prompt;
    app.prompt_buffer = "keep this draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        Some("req_completed_decrowded_footer"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let footer_row = find_last_line_containing(&lines, "Tab focus").expect("completed footer row");

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("Tab focus"))
            .count(),
        1,
        "completed shell should keep a single footer hint row\n{rendered}"
    );
    assert!(lines[footer_row].contains("Ctrl+p commands"));
    assert!(lines[footer_row].contains("q quit"));
}

#[cfg(test)]
#[test]
fn live_state_matrix_preserves_shell_structure() {
    let mut ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 24, None, None, "Ctrl+p commands");

    for ch in "draft next turn".chars() {
        ready.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_live_shell_document_composer_contract(
        &ready,
        100,
        24,
        Some("draft next turn"),
        None,
        "Ctrl+p commands",
    );

    let mut multiline = app::AppState::new_live(None, false, None);
    for ch in "draft".chars() {
        multiline.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    multiline.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for ch in "second line".chars() {
        multiline.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_live_shell_document_composer_contract(
        &multiline,
        100,
        24,
        Some("draft"),
        None,
        "Ctrl+p commands",
    );

    let mut streaming = app::AppState::new_live(None, false, None);
    streaming.ingest_event(envelope(
        1,
        Some("req_streaming_matrix"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_streaming_matrix".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "streaming".to_string(),
                request_digest: "digest-streaming".to_string(),
                metadata: None,
            },
        ),
    ));
    streaming.ingest_event(envelope(
        2,
        Some("req_streaming_matrix"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_streaming_matrix".to_string(),
                delta: "partial output".to_string(),
            },
        ),
    ));
    assert_live_shell_document_composer_contract(
        &streaming,
        100,
        24,
        None,
        None,
        "Ctrl+p commands",
    );

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    assert_live_shell_document_composer_contract(&degraded, 100, 24, None, None, "Degraded");

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    assert_live_shell_document_composer_contract(
        &disconnected,
        100,
        24,
        None,
        None,
        "Disconnected",
    );

    let mut failure = app::AppState::new_live(None, false, None);
    failure.set_status_banner(Some("runtime error while updating session".to_string()));
    assert_live_shell_document_composer_contract(&failure, 100, 24, None, None, "Failure");

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_matrix"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    assert_live_shell_document_composer_contract(&completed, 100, 24, None, None, "Tab focus");
}

#[cfg(test)]
#[test]
fn legacy_live_redesign_gate_is_removed() {
    let app_src = include_str!("app.rs");
    let chrome_src = include_str!("ui_chrome.rs");
    let transcript_src = include_str!("ui_transcript.rs");

    assert!(!app_src.contains("transcript_first_shell_redesign_active"));
    assert!(!chrome_src.contains("transcript_first_shell_redesign_active"));
    assert!(!transcript_src.contains("transcript_first_shell_redesign_active"));
    assert!(!chrome_src.contains("append_orchestration_status_legacy"));
}

#[cfg(test)]
#[test]
fn replay_read_only_copy_matches_operator_shell_contract() {
    let app =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());

    let rendered = render_live_lines(&app, 100, 24);

    assert!(rendered.contains("Replay · read-only"));
    assert!(rendered.contains("Replay is read-only"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▶ Modified Files"));
    assert!(rendered.contains("r reload"));
    assert!(rendered.contains("q quit"));
    assert!(!rendered.contains("Tab nav"));
    assert!(
        !rendered.contains("Inspect the transcript, event log, or diff, then press r to reload.")
    );
}

#[cfg(test)]
#[test]
fn replay_shell_is_read_only_without_tab_bar() {
    replay_shell_uses_read_only_operator_layout();
}

#[cfg(test)]
#[test]
fn command_palette_groups_commands_for_shell() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Commands"));
    assert!(rendered.contains("Continue session"));

    let mut live_app = app::AppState::new_live(None, false, None);
    live_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "toggle".chars() {
        live_app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    let filtered = render_live_lines(&live_app, 120, 30);
    assert!(filtered.contains("Commands"));
    assert!(filtered.contains("Toggle follow"));

    let mut system_app = app::AppState::new_startup(Vec::new(), None);
    system_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "quit".chars() {
        system_app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    let system_render = render_live_lines(&system_app, 120, 30);
    assert!(system_render.contains("Commands"));
    assert!(system_render.contains("Quit"));
}

#[cfg(test)]
#[test]
fn session_switcher_groups_entries_by_recency() {
    let entries = vec![
        startup_session_entry_with_details(
            "run_older",
            "/tmp/sessions/run_older",
            "older-run",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-02-14T08:30:00Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        ),
        startup_session_entry_with_details(
            "run_yesterday",
            "/tmp/sessions/run_yesterday",
            "yesterday-run",
            Some(harness_core::proj::RunStatus::Finished),
            Some(&test_timestamp_days_ago(1, "21:15")),
            "ops",
            "anthropic/claude-3.7",
            true,
            None,
        ),
        startup_session_entry_with_details(
            "run_today",
            "/tmp/sessions/run_today",
            "today-run",
            Some(harness_core::proj::RunStatus::Running),
            Some(&test_timestamp_days_ago(0, "09:45")),
            "worker",
            "mock/model-1",
            true,
            None,
        ),
    ];
    let mut app = app::AppState::new_startup(entries, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_today", "run_yesterday", "run_older"]
    );

    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("today-run"));
    assert!(rendered.contains("yesterday-run"));
    assert!(rendered.contains("older-run"));
}

#[cfg(test)]
#[test]
fn session_history_overlay_sorts_results_deterministically() {
    session_switcher_groups_entries_by_recency();
}

#[cfg(test)]
#[test]
fn footer_shortcuts_collapse_without_overlap() {
    lifecycle_shell_narrow_layout_renders_primary_cta();
}

#[cfg(test)]
#[test]
fn slash_commands_only_track_leading_slash_input() {
    let mut plain = app::AppState::new_live(None, false, None);
    plain.handle_key(key(crossterm::event::KeyCode::Char('h')));
    assert!(!plain.slash_visible);

    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(key(crossterm::event::KeyCode::Char('/')));
    assert!(app.slash_visible);
    assert_eq!(
        app.overlay_stack().top(),
        Some(overlay::OverlayKind::SlashCommands)
    );

    app.handle_key(key(crossterm::event::KeyCode::Char('h')));
    assert!(app.slash_visible);

    let mut non_leading = app::AppState::new_live(None, false, None);
    for ch in "hi/there".chars() {
        non_leading.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(!non_leading.slash_visible);
}

#[cfg(test)]
#[test]
fn startup_home_screen_renders_compose_first_shell() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4")
            .with_available_models(vec![
                app::ModelOption::from_model_ref("deep", "proxy:gpt-5.4"),
                app::ModelOption::from_model_ref("planner", "proxy:gpt-5.4-mini"),
            ])
            .with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 160, 48);
    assert!(rendered.contains("╻ ╻  ┏━┓  ┏━┓  ┏┓╻"));
    assert!(!rendered.contains("Launch: deep · gpt-5.4"));
    assert!(!rendered.contains("Provider proxy"));
    assert!(rendered.contains("Deep gpt-5.4 proxy · Demo"));
    assert!(rendered.contains("ctrl+p commands"));
    assert!(!rendered.contains("Enter select"));
    assert!(rendered.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(rendered.contains("commands"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions: New session · Continue session · Replay session"));
}

#[cfg(test)]
#[test]
fn startup_home_screen_uses_minimal_compat_shell() {
    let app = app::AppState::new_startup(Vec::new(), None);

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("╻ ╻  ┏━┓  ┏━┓  ┏┓╻") || rendered.contains("Harness"));
    assert!(rendered.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("New session"));
    assert!(!rendered.contains("Continue session"));
    assert!(!rendered.contains("Replay session"));
}

#[cfg(test)]
#[test]
fn startup_composer_keeps_inset_input_then_metadata_row_order() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    for (width, height) in [(100, 30), (80, 24)] {
        let rendered = render_live_lines(&app, width, height);
        let lines = rendered.lines().collect::<Vec<_>>();
        let composer_input_row = find_line_containing(
            &lines,
            "Ask anything... \"What is the tech stack of this project?\"",
        )
        .unwrap_or_else(|| panic!("startup composer input row at {width}x{height}\n{rendered}"));
        let composer_first_row = composer_input_row.saturating_sub(1);
        let metadata_gap_row = composer_input_row.saturating_add(1);
        let metadata_row = find_line_containing(&lines, "Deep gpt-5.4 proxy · Demo")
            .unwrap_or_else(|| panic!("startup metadata row at {width}x{height}\n{rendered}"));
        let composer_last_row = metadata_row.saturating_add(1);

        assert_eq!(
            composer_input_row,
            composer_first_row + 1,
            "startup composer should keep a blank inset row before input at {width}x{height}\n{rendered}"
        );
        assert_eq!(
            metadata_row,
            composer_input_row + 2,
            "startup metadata should keep the shell's blank spacer between the input and metadata rows at {width}x{height}\n{rendered}"
        );
        assert_eq!(
            composer_last_row,
            metadata_row + 1,
            "startup composer should end with the cap row immediately after metadata at {width}x{height}\n{rendered}"
        );
        assert!(
            !lines[composer_first_row].chars().any(char::is_alphanumeric),
            "startup inset row should stay visually blank at {width}x{height}\n{rendered}"
        );
        assert!(
            !lines[metadata_gap_row].chars().any(char::is_alphanumeric),
            "startup metadata spacer row should stay visually blank at {width}x{height}\n{rendered}"
        );
    }
}

#[cfg(test)]
#[test]
fn startup_composer_width_stays_capped_for_shell() {
    let app = app::AppState::new_startup(Vec::new(), None);

    for (width, height) in [(80, 24), (100, 30), (160, 48)] {
        let plan = FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, width, height));
        let dock = plan.dock.expect("startup dock layout");

        assert_eq!(
            dock.shell.width, 75,
            "startup composer should keep the shell width cap at {width}x{height}"
        );
        assert_eq!(dock.composer.width, 75);
    }
}

#[cfg(test)]
#[test]
fn dense_live_composer_uses_full_height_without_metadata_row() {
    let mut ready = app::AppState::new_live(None, false, None);
    ready.prompt_buffer = "draft".to_string();
    ready.prompt_cursor = ready.prompt_buffer.chars().count();
    let rendered = render_live_lines(&ready, 60, 18);
    let lines = rendered.lines().collect::<Vec<_>>();
    let (composer_first_row, composer_input_row, composer_last_row) =
        live_shell_composer_input_span(&lines);

    assert!(lines[composer_input_row].contains("draft"));
    assert_eq!(composer_input_row, composer_first_row + 1);
    assert!(composer_last_row > composer_input_row);
    assert!(
        find_line_containing_in_range(
            &lines,
            composer_input_row + 1,
            composer_last_row + 1,
            "Current runtime:"
        )
        .is_none(),
        "dense live composer should remove the status row under the draft\n{rendered}"
    );
}

#[cfg(test)]
#[test]
fn dense_live_compaction_feedback_uses_toast_when_metadata_row_is_absent() {
    let mut ready = app::AppState::new_live(None, false, None);
    ready.prompt_buffer = "draft".to_string();
    ready.prompt_cursor = ready.prompt_buffer.chars().count();
    ready.set_toast_for_test(
        "manual compaction skipped: need at least two completed turns",
        app::ToastVariant::Info,
    );

    let rendered = render_live_lines(&ready, 60, 18);
    let lines = rendered.lines().collect::<Vec<_>>();
    let (_, composer_input_row, composer_last_row) = live_shell_composer_input_span(&lines);

    assert!(
        find_line_containing_in_range(
            &lines,
            composer_input_row + 1,
            composer_last_row + 1,
            "Current runtime:"
        )
        .is_none(),
        "dense live composer should still omit the metadata row\n{rendered}"
    );
    assert!(
        rendered.contains("manual compaction skipped"),
        "manual compaction feedback should stay visible via toast when the footer metadata row is absent\n{rendered}"
    );
}

#[cfg(test)]
#[test]
fn live_composer_disclosure_keeps_compact_summary_and_commands() {
    let mut ready = app::AppState::new_live(None, false, None);
    let mut events = session_view_events();
    events.pop();
    for event in events {
        ready.ingest_event(event);
    }
    let rendered = render_live_lines(&ready, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let disclosure_row = find_line_containing(&lines, "Ctrl+p commands")
        .unwrap_or_else(|| panic!("live composer disclosure row\n{rendered}"));

    assert!(lines[disclosure_row].contains("Ctrl+p commands"));
    assert!(!lines[disclosure_row].contains("Enter send"));
    assert!(!lines[disclosure_row].contains("tool finished"));
    assert!(!lines[disclosure_row].contains("turn 1"));
    assert!(!lines[disclosure_row].contains("ready for next turn"));
    assert!(!rendered.contains("Current runtime:"));
}

#[cfg(test)]
#[test]
fn slash_overlay_uses_reference_navigation_keys() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(key(crossterm::event::KeyCode::Char('/')));
    assert_eq!(
        app.overlay_stack().top(),
        Some(overlay::OverlayKind::SlashCommands)
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.slash_selected,
        app.slash_filtered.len().saturating_sub(1)
    );

    app.handle_key(key(crossterm::event::KeyCode::Down));
    assert_eq!(app.slash_selected, 0);

    app.handle_key(key(crossterm::event::KeyCode::Up));
    assert_eq!(
        app.slash_selected,
        app.slash_filtered.len().saturating_sub(1)
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('n'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(app.slash_selected, 0);

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(app.prompt_buffer, "");
    assert!(!app.slash_visible);
}

#[cfg(test)]
#[test]
fn slash_overlay_uses_input_width_aligned_rows_and_accent_selection() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Char('/')));

    let rendered = render_live_lines(&app, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let row = find_line_containing_all(&lines, &["/events", "Open the event log review"])
        .unwrap_or_else(|| panic!("slash /events row\n{rendered}"));
    let events_description = lines[row]
        .find("Open the event log review")
        .expect("events description column");
    let new_row = find_line_containing_all(&lines, &["/new", "Return to the home shell"])
        .unwrap_or_else(|| panic!("slash /new row\n{rendered}"));
    let new_description = lines[new_row]
        .find("Return to the home shell")
        .expect("new description column");

    assert_eq!(events_description, new_description);
    assert!(!lines[row].contains('┃'));
    assert!(!rendered.contains('╭') && !rendered.contains('╰') && !rendered.contains('│'));

    let buffer = render_live_cells(&app, 100, 24);
    let selected_command = format!(
        "/{}",
        app.slash_filtered.first().expect("selected slash command")
    );
    let (selected_row, selected_fgs, selected_bgs) =
        row_text_and_palette(&buffer, 100, &selected_command).expect("selected slash row palette");
    let command_start = selected_row
        .find(&selected_command)
        .expect("selected command start");
    let description_start = selected_row
        .find(crate::app::slash_command_description(
            selected_command.trim_start_matches('/'),
        ))
        .expect("selected description start");
    let theme = Theme::default();

    assert_eq!(selected_bgs[command_start], theme.text.accent);
    assert_eq!(selected_bgs[description_start], theme.text.accent);
    assert_eq!(selected_fgs[command_start], theme.text.inverse);
    assert_eq!(selected_fgs[description_start], theme.text.inverse);
}

#[cfg(test)]
fn exact_test_key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

#[cfg(test)]
fn exact_test_key_with_modifiers(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
}

#[cfg(test)]
fn exact_test_session_entry(run_id: &str, run_dir: &str) -> app::SessionHistoryEntry {
    app::SessionHistoryEntry {
        run_dir: PathBuf::from(run_dir),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: run_id.to_string(),
            run_name: Some("Resume target".to_string()),
            status: Some(harness_core::proj::RunStatus::Finished),
            last_updated_at: Some("2026-03-10T10:00:00Z".to_string()),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some("deep".to_string()),
            provider_model: Some("default/gpt-5.4-mini".to_string()),
            mode_source: harness_core::proj::SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    }
}

#[cfg(test)]
#[test]
fn new_session_preserves_unsent_draft_across_home_navigation() {
    app::set_pending_live_prompt_draft(Some("draft from home".to_string()));

    let mut startup = app::AppState::new_startup(Vec::new(), None);
    assert_eq!(startup.prompt_buffer, "draft from home");

    startup.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "new".chars() {
        startup.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    startup.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    assert!(startup.should_quit);

    let live = app::AppState::new_live(None, false, None);
    assert_eq!(live.prompt_buffer, "draft from home");
    assert_eq!(live.prompt_cursor, "draft from home".chars().count());
}

#[cfg(test)]
#[test]
fn command_driven_session_switch_emits_correct_ui_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        Some(sink),
    );

    app.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));

    assert!(matches!(
        intents.lock().expect("lock intents").last(),
        Some(UiIntent::ContinueSession { run_id, run_dir })
            if run_id == "run_resume" && run_dir.as_path() == Path::new("/tmp/sessions/run_resume")
    ));
}

#[cfg(test)]
#[test]
fn live_shell_geometry_contract_is_rule_based() {
    let theme = Theme::default();
    let session_contract = |width, height| {
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        layout::session_geometry_contract(area, theme.live_shell_layout(width, height))
    };

    assert_eq!(session_contract(95, 40), session_contract(96, 40));
    assert_eq!(session_contract(101, 30), session_contract(100, 30));
    assert_eq!(session_contract(81, 25), session_contract(80, 25));

    assert_eq!(
        session_contract(95, 40).sidebar_mode,
        layout::SessionSidebarMode::Overlay { width: 42 }
    );
    assert_eq!(
        session_contract(120, 30).sidebar_mode,
        layout::SessionSidebarMode::Overlay { width: 42 }
    );
}

#[cfg(test)]
#[test]
fn live_shell_threshold_edges_are_stable() {
    let theme = Theme::default();
    let session_contract = |width, height| {
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        layout::session_geometry_contract(area, theme.live_shell_layout(width, height))
    };

    let expectations = [
        (
            89,
            40,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            90,
            35,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            90,
            36,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            99,
            29,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            99,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            100,
            29,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            100,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
    ];

    for (width, height, header_mode, footer_mode, sidebar_mode) in expectations {
        let contract = session_contract(width, height);
        assert_eq!(
            contract.header_mode, header_mode,
            "unexpected header mode for {width}x{height}"
        );
        assert_eq!(
            contract.footer_mode, footer_mode,
            "unexpected footer mode for {width}x{height}"
        );
        assert_eq!(
            contract.sidebar_mode, sidebar_mode,
            "unexpected sidebar mode for {width}x{height}"
        );
        assert_eq!(contract.palette_overlay_max_width, None);
        assert_eq!(contract.slash_overlay_max_width, None);
    }
}

#[cfg(test)]
#[test]
fn dense_minimum_shell_hides_sidebar_and_caps_overlays() {
    let theme = Theme::default();
    let area = ratatui::layout::Rect::new(0, 0, 60, 18);
    let contract = layout::session_geometry_contract(area, theme.live_shell_layout(60, 18));

    assert_eq!(contract.header_mode, layout::SessionHeaderMode::Hidden);
    assert_eq!(contract.footer_mode, layout::SessionFooterMode::Minimal);
    assert_eq!(contract.sidebar_mode, layout::SessionSidebarMode::Hidden);
    assert_eq!(contract.palette_overlay_max_width, Some(46));
    assert_eq!(contract.slash_overlay_max_width, None);

    let non_dense = layout::session_geometry_contract(
        ratatui::layout::Rect::new(0, 0, 61, 19),
        theme.live_shell_layout(61, 19),
    );
    assert_ne!(non_dense.sidebar_mode, layout::SessionSidebarMode::Hidden);
    assert_eq!(non_dense.palette_overlay_max_width, None);
    assert_eq!(non_dense.slash_overlay_max_width, None);

    let mut dense = app::AppState::new_live(None, false, None);
    dense.live_details_drawer_open = true;
    let dense_plan = layout::FrameLayoutPlan::for_app(&dense, area);
    assert!(dense_plan.operator_sidebar.is_none());
    assert!(dense_plan.details_overlay.is_none());

    let mut palette = app::AppState::new_live(None, false, None);
    palette.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_plan = layout::FrameLayoutPlan::for_app(&palette, area);
    assert_eq!(
        palette_plan.palette_overlay.map(|overlay| overlay.width),
        Some(58)
    );
}

#[cfg(test)]
#[test]
fn slash_overlay_matches_composer_text_input_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Char('/')));

    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);
    let composer = plan.dock.expect("live dock layout").composer;
    let overlay = plan.slash_overlay.expect("slash overlay");
    let content = layout::slash_command_overlay_content_area(overlay);
    let theme = Theme::default();
    let body_width = composer.width.saturating_sub(1);
    let input_padding = theme
        .live_shell
        .rhythm
        .composer_padding_x
        .min(body_width.saturating_sub(1));
    let input_x = composer.x.saturating_add(1).saturating_add(input_padding);
    let input_width = body_width.saturating_sub(input_padding.saturating_mul(2));

    assert_eq!(overlay.x, input_x);
    assert_eq!(overlay.width, input_width);
    assert_eq!(overlay.y.saturating_add(overlay.height), composer.y);
    assert_eq!(overlay.height, app.slash_filtered.len() as u16);
    assert!(overlay.height <= 10);
    assert_eq!(content.x, overlay.x);
    assert_eq!(content.width, overlay.width);
    assert_eq!(content.y, overlay.y);
    assert_eq!(content.height, overlay.height);
}

#[cfg(test)]
fn assert_live_shell_headerless_contract(app: &app::AppState, width: u16, height: u16) {
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    let plan = layout::FrameLayoutPlan::for_app(app, area);
    let rendered = render_live_lines(app, width, height);
    let transcript = plan
        .transcript
        .expect("headerless layout should preserve transcript content");

    assert_eq!(
        plan.session_contract.header_mode,
        layout::SessionHeaderMode::Hidden
    );
    assert_eq!(
        plan.header.height, 0,
        "root header must stay hidden\n{rendered}"
    );
    assert!(
        plan.live_anchor.is_none(),
        "live anchor should stay removed\n{rendered}"
    );
    assert_eq!(transcript.y, plan.shell.y);
    if let Some(sidebar) = plan.operator_sidebar {
        assert_eq!(sidebar.y, plan.shell.y);
    }
}

#[cfg(test)]
#[test]
fn live_shell_hidden_header_modes_remove_in_shell_anchor() {
    let mut split_live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        split_live.ingest_event(event);
    }
    assert_live_shell_headerless_contract(&split_live, 96, 40);

    let mut primary_details = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        primary_details.ingest_event(event);
    }
    primary_details.live_details_drawer_open = true;
    assert_live_shell_headerless_contract(&primary_details, 100, 30);

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    for event in session_view_events() {
        completed.ingest_event(event);
    }
    completed.ingest_event(envelope(
        11,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));
    assert_live_shell_headerless_contract(&completed, 100, 30);

    let mut recovery = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    recovery.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "tool execution failed".to_string(),
        }),
    ));
    recovery.set_status_banner(Some("runtime error while updating session".to_string()));
    assert_live_shell_headerless_contract(&recovery, 100, 30);
}

#[cfg(test)]
#[test]
fn live_shell_minimum_modes_stay_headerless() {
    for (width, height) in [(80, 24), (60, 18)] {
        let mut app = app::AppState::new_live(None, false, None);
        for event in session_view_events() {
            app.ingest_event(event);
        }

        let area = ratatui::layout::Rect::new(0, 0, width, height);
        let plan = layout::FrameLayoutPlan::for_app(&app, area);
        let rendered = render_live_lines(&app, width, height);
        let lines = rendered.lines().collect::<Vec<_>>();
        assert_eq!(
            plan.session_contract.header_mode,
            layout::SessionHeaderMode::Hidden
        );
        assert_eq!(
            plan.header.height, 0,
            "minimum layouts should remove the root header\n{rendered}"
        );
        assert!(
            plan.live_anchor.is_none(),
            "minimum layouts must not add an in-shell anchor\n{rendered}"
        );
        assert_eq!(count_lines_containing(&lines, "run run_fixture"), 0);
    }
}

#[cfg(test)]
#[test]
fn live_shell_redesign_guardrails_preserve_primary_contract() {
    let scope_for = |surface| {
        FULL_SURFACE_SCOPE_MATRIX
            .iter()
            .copied()
            .find(|scope| scope.surface == surface)
            .unwrap_or_else(|| panic!("missing redesign guardrail scope for {surface:?}"))
    };

    for surface in [
        ParitySurface::LiveEmpty,
        ParitySurface::LiveRun,
        ParitySurface::CompletedPostRun,
        ParitySurface::ReplayShell,
    ] {
        let scope = scope_for(surface);
        assert_eq!(
            scope.hierarchy,
            ShellHierarchyContract::TranscriptFirstSession,
            "{surface:?} redesign guardrail must preserve transcript-first hierarchy"
        );
        assert!(
            !scope.default_tab_chrome,
            "{surface:?} redesign guardrail must preserve no default tab chrome"
        );
        assert!(
            !scope.debug_inspector_in_primary_path,
            "{surface:?} redesign guardrail must keep debug inspector out of the primary path"
        );
    }

    assert_eq!(
        scope_for(ParitySurface::ReplayShell).composer,
        ComposerContract::ReplayReadOnlyProgressiveDisclosure,
        "replay read-only redesign guardrail must stay explicit"
    );

    let theme = Theme::default();
    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }

    for (label, width, height, expected_header, expected_footer, expected_sidebar) in [
        (
            "dense 60x18 breakpoint support",
            60,
            18,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Minimal,
            layout::SessionSidebarMode::Hidden,
        ),
        (
            "minimum 80x24 breakpoint support",
            80,
            24,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Reduced,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            "split 96x40 breakpoint support",
            96,
            40,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            "primary 100x30 breakpoint support",
            100,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Overlay { width: 42 },
        ),
        (
            "wide 160x30 breakpoint support",
            160,
            30,
            layout::SessionHeaderMode::Hidden,
            layout::SessionFooterMode::Standard,
            layout::SessionSidebarMode::Persistent { width: 42 },
        ),
    ] {
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        let contract =
            layout::session_geometry_contract(area, theme.live_shell_layout(width, height));
        let plan = layout::FrameLayoutPlan::for_app(&live, area);
        let rendered = render_live_lines(&live, width, height);

        assert_eq!(
            contract.header_mode, expected_header,
            "{label}: redesign guardrail must preserve breakpoint support without reintroducing header chrome"
        );
        assert_eq!(
            contract.footer_mode, expected_footer,
            "{label}: redesign guardrail must preserve footer breakpoint support"
        );
        assert_eq!(
            contract.sidebar_mode, expected_sidebar,
            "{label}: redesign guardrail must preserve transcript-first/operator-sidebar breakpoint support"
        );
        assert!(
            plan.transcript.is_some(),
            "{label}: transcript-first redesign guardrail must keep a transcript surface"
        );
        assert!(
            !rendered.contains("Tabs"),
            "{label}: redesign guardrail must preserve no default tab chrome\n{rendered}"
        );
        assert!(
            plan.details_overlay.is_none(),
            "{label}: redesign guardrail should not route the primary path through overlay chrome"
        );

        match expected_sidebar {
            layout::SessionSidebarMode::Persistent { .. } => {
                let transcript = plan
                    .transcript
                    .expect("persistent breakpoint should preserve transcript frame");
                let sidebar = plan
                    .operator_sidebar
                    .expect("persistent breakpoint should preserve operator sidebar");
                assert!(
                    transcript.x < sidebar.x,
                    "{label}: transcript-first layout must keep transcript left of the operator sidebar"
                );
                assert!(
                    transcript.width > sidebar.width,
                    "{label}: transcript-first layout must keep transcript primary over the operator sidebar"
                );
            }
            layout::SessionSidebarMode::Overlay { .. } | layout::SessionSidebarMode::Hidden => {
                assert!(
                    plan.operator_sidebar.is_none(),
                    "{label}: compact redesign guardrail must avoid default persistent sidebar chrome"
                );
            }
        }
    }

    let mut replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    replay.focus = app::Focus::Prompt;
    for ch in "blocked in replay".chars() {
        replay.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    replay.handle_key(key(crossterm::event::KeyCode::Enter));

    let replay_plan =
        layout::FrameLayoutPlan::for_app(&replay, ratatui::layout::Rect::new(0, 0, 100, 30));
    let replay_render = render_live_lines(&replay, 100, 30);
    assert!(
        replay_plan.transcript.is_some(),
        "replay read-only redesign guardrail must preserve transcript-first shell structure"
    );
    assert!(
        replay_plan.operator_sidebar.is_some(),
        "replay read-only redesign guardrail must preserve the operator sidebar when primary geometry allows"
    );
    assert!(
        replay.prompt_buffer.is_empty(),
        "replay read-only redesign guardrail must drop typed draft text after submit attempts"
    );
    let replay_lines = replay_render.lines().collect::<Vec<_>>();
    let replay_header_row = find_line_containing_all(&replay_lines, &["Replay", "read-only"])
        .unwrap_or_else(|| {
            panic!("replay read-only redesign guardrail must preserve replay identity\n{replay_render}")
        });
    let replay_disabled_row = find_line_containing_all_from(
        &replay_lines,
        replay_header_row + 1,
        &["▎", "Replay is read-only."],
    )
    .filter(|row| !replay_lines[*row].contains("run "))
    .unwrap_or_else(|| {
        panic!("replay read-only redesign guardrail must preserve a disabled composer row\n{replay_render}")
    });
    let replay_shortcuts_row = find_line_containing_from(&replay_lines, replay_disabled_row + 1, "shortcuts")
        .unwrap_or_else(|| {
            panic!("replay read-only redesign guardrail must preserve shortcut affordances\n{replay_render}")
        });

    assert!(
        replay_header_row < replay_disabled_row,
        "replay identity should stay above the disabled composer guidance\n{replay_render}"
    );
    assert!(
        replay_disabled_row < replay_shortcuts_row,
        "shortcut guidance should remain below the disabled composer guidance\n{replay_render}"
    );
    assert!(
        !replay_render.contains("blocked in replay"),
        "replay read-only redesign guardrail must not surface submitted draft text\n{replay_render}"
    );
    assert!(
        !replay_render.contains("Tabs"),
        "replay read-only redesign guardrail must preserve no default tab chrome\n{replay_render}"
    );
}

delegate_test!(overlays_share_elevated_card_language => module_overlays_share_elevated_card_language);
delegate_test!(quiet_overlay_helper_rows_use_semantic_chrome_palette => module_quiet_overlay_helper_rows_use_semantic_chrome_palette);
delegate_test!(permission_modal_remains_visually_dominant_and_fail_closed => module_permission_modal_remains_visually_dominant_and_fail_closed);

delegate_test!(overlay_stack_orders_permission_above_commands_and_slash => app::AppState::exact_test_overlay_stack_orders_permission_above_commands_and_slash);

delegate_test!(live_shell_redesign_preserves_replay_overlay_and_permission_parity => module_live_shell_redesign_preserves_replay_overlay_and_permission_parity);

#[cfg(test)]
#[test]
fn harness_dark_theme_is_default() {
    let default = Theme::default();
    let harness_dark = Theme::harness_dark();

    assert_eq!(default.surface, harness_dark.surface);
    assert_eq!(default.border, harness_dark.border);
    assert_eq!(default.text, harness_dark.text);
    assert_eq!(default.status, harness_dark.status);
}

#[cfg(test)]
#[test]
fn theme_tokens_cover_live_shell_states() {
    let default = Theme::default();
    let tokens = default.token_families();

    assert_eq!(default.live_shell.glyphs.streaming, "◐");
    assert_eq!(default.live_shell.glyphs.done, "●");
    assert_eq!(default.live_shell.glyphs.error, "✗");
    assert_eq!(default.live_shell.glyphs.pending_permission, "◷");
    assert_eq!(default.live_shell.glyphs.queued, "◴");
    assert_eq!(default.live_shell.glyphs.running, "◐");
    assert_eq!(default.live_shell.glyphs.succeeded, "●");
    assert_eq!(default.live_shell.glyphs.failed, "✗");
    assert_eq!(tokens.live_shell.glyphs.ascii.status.streaming, "o");
    assert_eq!(
        tokens.live_shell.glyphs.ascii.status.pending_permission,
        "?"
    );
    assert_eq!(tokens.live_shell.glyphs.ascii.status.failed, "x");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.user_marker, ">");
    assert_eq!(tokens.live_shell.glyphs.ascii.transcript.card_top, "+-");

    assert_eq!(default.live_shell.heights.header, 1);
    assert_eq!(default.live_shell.heights.tabs, 3);
    assert_eq!(default.live_shell.heights.status, 1);
    assert_eq!(default.live_shell.heights.footer, 1);
    assert_eq!(default.live_shell.heights.prompt_block(), 5);
    assert_eq!(default.live_shell.rhythm.transcript_gutter_x, 2);
    assert_eq!(default.live_shell.rhythm.status_separator, 2);
    assert_eq!(default.live_shell.minimum.centered_content_width, 76);
    assert_eq!(default.live_shell.minimum.content_margin_x, 1);
    assert_eq!(default.live_shell.primary.centered_content_width, 90);
    assert_eq!(default.live_shell.primary.content_margin_x, 0);
    assert_eq!(tokens.palette.surfaces, default.surface);
    assert_eq!(tokens.palette.borders, default.border);
    assert_eq!(
        tokens.live_shell.geometry.breakpoints,
        crate::theme::ShellBreakpoints::DEFAULT
    );
    assert_eq!(
        tokens.live_shell.geometry.minimum,
        default.live_shell.minimum
    );
    assert_eq!(
        tokens.live_shell.spacing.heights,
        default.live_shell.heights
    );
    assert_eq!(tokens.live_shell.spacing.rhythm, default.live_shell.rhythm);
    assert_eq!(tokens.live_shell.copy.startup, default.live_shell.startup);
    assert_eq!(
        tokens.live_shell.copy.empty_state,
        default.live_shell.empty_state
    );
    assert_eq!(
        default.live_shell.primary.target,
        ShellGeometryTarget::Primary
    );
    assert_eq!(
        default.live_shell.minimum.target,
        ShellGeometryTarget::Minimum
    );
}

#[cfg(test)]
#[test]
fn harness_dark_theme_has_exact_palette() {
    let theme = Theme::harness_dark();

    assert_eq!(
        theme.surface.canvas,
        ratatui::style::Color::Rgb(0x0A, 0x0A, 0x0A)
    );
    assert_eq!(
        theme.surface.shell,
        ratatui::style::Color::Rgb(0x0A, 0x0A, 0x0A)
    );
    assert_eq!(
        theme.surface.panel,
        ratatui::style::Color::Rgb(0x14, 0x14, 0x14)
    );
    assert_eq!(
        theme.surface.panel_elevated,
        ratatui::style::Color::Rgb(0x1E, 0x1E, 0x1E)
    );
    assert_eq!(
        theme.surface.overlay,
        ratatui::style::Color::Rgb(0x14, 0x14, 0x14)
    );
    assert_eq!(
        theme.border.subtle,
        ratatui::style::Color::Rgb(0x3C, 0x3C, 0x3C)
    );
    assert_eq!(
        theme.border.strong,
        ratatui::style::Color::Rgb(0x48, 0x48, 0x48)
    );
    assert_eq!(
        theme.border.focus,
        ratatui::style::Color::Rgb(0x60, 0x60, 0x60)
    );
    assert_eq!(
        theme.text.primary,
        ratatui::style::Color::Rgb(0xEE, 0xEE, 0xEE)
    );
    assert_eq!(
        theme.text.secondary,
        ratatui::style::Color::Rgb(0x80, 0x80, 0x80)
    );
    assert_eq!(
        theme.text.tertiary,
        ratatui::style::Color::Rgb(0x80, 0x80, 0x80)
    );
    assert_eq!(
        theme.text.accent,
        ratatui::style::Color::Rgb(0xF5, 0xA7, 0x42)
    );
    assert_eq!(
        theme.text.inverse,
        ratatui::style::Color::Rgb(0x0A, 0x0A, 0x0A)
    );
    assert_eq!(
        theme.status.success,
        ratatui::style::Color::Rgb(0x7F, 0xD8, 0x8F)
    );
    assert_eq!(
        theme.status.warning,
        ratatui::style::Color::Rgb(0xF5, 0xA7, 0x42)
    );
    assert_eq!(
        theme.status.error,
        ratatui::style::Color::Rgb(0xE0, 0x6C, 0x75)
    );
    assert_eq!(
        theme.status.info,
        ratatui::style::Color::Rgb(0x56, 0xB6, 0xC2)
    );
    assert_eq!(
        theme.status.disabled,
        ratatui::style::Color::Rgb(0x80, 0x80, 0x80)
    );
    assert_eq!(
        theme.agents.build,
        ratatui::style::Color::Rgb(0x5C, 0x9C, 0xF5)
    );
    assert_eq!(
        theme.agents.plan,
        ratatui::style::Color::Rgb(0x9D, 0x7C, 0xD8)
    );
}

#[cfg(test)]
#[test]
fn command_palette_state_filters_existing_commands() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('n')));

    assert!(app.palette_visible);
    assert_eq!(app.palette_input, "n");
    assert_eq!(app.palette_cursor, 1);
    assert_eq!(
        app.palette_filtered,
        vec!["new_session".to_string(), "agent_cycle".to_string()]
    );
    assert!(app.palette_filtered.iter().all(|command| {
        Action::palette_commands()
            .iter()
            .any(|(existing, _)| existing == command)
    }));
}

#[cfg(test)]
#[test]
fn hovered_wheel_target_uses_layout_plan() {
    let area = ratatui::layout::Rect::new(0, 0, 140, 40);

    let mut default_app = app::AppState::new_live(None, false, None);
    default_app.live_details_drawer_open = true;
    default_app.ingest_event(operator_sidebar_edit_only_event(1));

    let mut themed_app = app::AppState::new_live(None, false, None);
    themed_app.live_details_drawer_open = true;
    themed_app.ingest_event(operator_sidebar_edit_only_event(1));
    let mut custom_theme = Theme::default();
    custom_theme.live_shell.primary.centered_content_width = 72;
    custom_theme.live_shell.primary.content_margin_x = 10;
    custom_theme.live_shell.primary.activity_drawer_width = 18;
    custom_theme.live_shell.primary.details_sidebar_width = 36;
    themed_app.set_theme_for_test(custom_theme);

    let default_plan = layout::FrameLayoutPlan::for_app(&default_app, area);
    let default_transcript = default_plan.transcript.expect("default transcript area");
    let themed_plan = layout::FrameLayoutPlan::for_app(&themed_app, area);
    let themed_sidebar = themed_plan
        .operator_sidebar
        .expect("themed operator sidebar");
    let default_target = ui::hovered_wheel_target(
        &default_app,
        area,
        default_transcript.x.saturating_add(2),
        default_transcript.y.saturating_add(1),
    );
    let themed_target = ui::hovered_wheel_target(
        &themed_app,
        area,
        themed_sidebar.x.saturating_add(1),
        themed_sidebar.y.saturating_add(1),
    );

    assert_ne!(default_plan.operator_sidebar, themed_plan.operator_sidebar);
    assert_eq!(default_target, Some(ui::WheelTarget::Transcript));
    assert_eq!(themed_target, Some(ui::WheelTarget::Inspector));
}

#[cfg(test)]
#[test]
fn layout_plan_minimum_geometry_matches_shell_contract() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 80, 24));
    let dock = plan.dock.expect("minimum dock layout");

    assert_eq!(plan.root, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(plan.header, ratatui::layout::Rect::new(0, 0, 80, 0));
    assert_eq!(plan.content, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(plan.shell, ratatui::layout::Rect::new(2, 0, 76, 24));
    assert_eq!(plan.footer, ratatui::layout::Rect::new(0, 24, 80, 0));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(2, 0, 76, 18))
    );
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(2, 18, 76, 5))
    );
    assert_eq!(dock.shell, ratatui::layout::Rect::new(2, 18, 76, 6));
    assert_eq!(dock.status, plan.status);
    assert_eq!(dock.composer, plan.composer.expect("minimum composer"));
    assert_eq!(
        dock.disclosure,
        Some(ratatui::layout::Rect::new(2, 23, 76, 1))
    );
    assert_eq!(plan.disclosure, dock.disclosure);
}

#[cfg(test)]
#[test]
fn layout_plan_primary_geometry_matches_shell_contract() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    for event in session_view_events() {
        app.ingest_event(event);
    }
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));
    let dock = plan.dock.expect("primary dock layout");

    assert_eq!(plan.root, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(plan.header, ratatui::layout::Rect::new(0, 0, 100, 0));
    assert_eq!(plan.content, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(plan.shell, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(plan.footer, ratatui::layout::Rect::new(0, 30, 100, 0));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 100, 24))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(0, 24, 100, 5))
    );
    assert_eq!(dock.shell, ratatui::layout::Rect::new(0, 24, 100, 6));
    assert_eq!(dock.status, plan.status);
    assert_eq!(dock.composer, plan.composer.expect("primary composer"));
    assert_eq!(
        dock.disclosure,
        Some(ratatui::layout::Rect::new(0, 29, 100, 1))
    );
    assert_eq!(plan.disclosure, dock.disclosure);
}

#[cfg(test)]
#[test]
fn layout_plan_primary_empty_operator_rail_keeps_fixed_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));

    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 100, 24))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(plan.details_overlay, None);
    assert_eq!(plan.wheel_hit_areas.transcript, plan.transcript);
    assert_eq!(plan.wheel_hit_areas.overlay, None);
    assert_eq!(plan.wheel_hit_areas.inspector, None);
}

#[cfg(test)]
#[test]
fn layout_plan_split_empty_operator_rail_keeps_fixed_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 96, 40));

    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 96, 34))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(plan.details_overlay, None);
    assert_eq!(plan.wheel_hit_areas.transcript, plan.transcript);
    assert_eq!(plan.wheel_hit_areas.overlay, None);
    assert_eq!(plan.wheel_hit_areas.inspector, None);
}

#[cfg(test)]
#[test]
fn wide_primary_live_layout_uses_available_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;

    let area = ratatui::layout::Rect::new(0, 0, 160, 40);
    let theme = app.theme();
    let shell_layout = theme.live_shell_layout(area.width, area.height);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);

    assert_eq!(shell_layout.target, ShellGeometryTarget::Primary);
    assert_eq!(plan.shell.x, 0);
    assert_eq!(plan.shell.width, area.width);
    assert!(plan.shell.width > shell_layout.centered_content_width);
}

#[cfg(test)]
#[test]
fn split_window_live_layout_uses_available_width() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;

    let area = ratatui::layout::Rect::new(0, 0, 96, 40);
    let theme = app.theme();
    let shell_layout = theme.live_shell_layout(area.width, area.height);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);

    assert_eq!(shell_layout.target, ShellGeometryTarget::Split);
    assert_eq!(plan.shell.x, 0);
    assert_eq!(plan.shell.width, area.width);
    assert!(plan.shell.width > shell_layout.centered_content_width);
}

#[cfg(test)]
#[test]
fn live_layout_breakpoints_choose_shell_variant() {
    let theme = Theme::default();

    let minimum = theme.live_shell_layout(80, 24);
    assert_eq!(minimum.target, ShellGeometryTarget::Minimum);
    assert_eq!(minimum.activity_drawer_width, 20);
    assert_eq!(minimum.inspector_drawer_width, 20);
    assert_eq!(minimum.details_sidebar_width, 42);
    assert_eq!(minimum.transcript_min_width, 28);
    assert_eq!(minimum.centered_content_width, 76);

    let split = theme.live_shell_layout(96, 40);
    assert_eq!(split.target, ShellGeometryTarget::Split);
    assert_eq!(split.activity_drawer_width, 18);
    assert_eq!(split.inspector_drawer_width, 24);
    assert_eq!(split.details_sidebar_width, 42);
    assert_eq!(split.transcript_min_width, 32);
    assert_eq!(split.centered_content_width, 86);

    let primary = theme.live_shell_layout(100, 30);
    assert_eq!(primary.target, ShellGeometryTarget::Primary);
    assert_eq!(primary.activity_drawer_width, 24);
    assert_eq!(primary.inspector_drawer_width, 28);
    assert_eq!(primary.details_sidebar_width, 42);
    assert_eq!(primary.transcript_min_width, 40);
    assert_eq!(primary.centered_content_width, 90);

    assert_eq!(
        theme.live_shell.target(89, 40),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(
        theme.live_shell.target(90, 35),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(theme.live_shell.target(90, 36), ShellGeometryTarget::Split);
    assert_eq!(
        theme.live_shell.target(99, 30),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(theme.live_shell.target(99, 40), ShellGeometryTarget::Split);
    assert_eq!(
        theme.live_shell.target(100, 29),
        ShellGeometryTarget::Minimum
    );
    assert_eq!(
        theme.live_shell.target(100, 30),
        ShellGeometryTarget::Primary
    );
}

#[cfg(test)]
#[test]
fn layout_breakpoints_match_shell_parity_contract() {
    let mut wide = app::AppState::new_live(None, false, None);
    wide.active_tab = app::Tab::Run;
    for event in session_view_events() {
        wide.ingest_event(event);
    }
    let wide_plan =
        layout::FrameLayoutPlan::for_app(&wide, ratatui::layout::Rect::new(0, 0, 160, 48));
    assert_eq!(wide_plan.header.height, 0);
    assert_eq!(
        wide_plan.operator_sidebar,
        Some(ratatui::layout::Rect::new(118, 0, 42, 48))
    );

    let mut primary = app::AppState::new_live(None, false, None);
    primary.active_tab = app::Tab::Run;
    for event in session_view_events() {
        primary.ingest_event(event);
    }
    let primary_plan =
        layout::FrameLayoutPlan::for_app(&primary, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(primary_plan.header.height, 0);
    assert_eq!(primary_plan.operator_sidebar, None);

    let mut split = app::AppState::new_live(None, false, None);
    split.active_tab = app::Tab::Run;
    for event in session_view_events() {
        split.ingest_event(event);
    }
    let split_plan =
        layout::FrameLayoutPlan::for_app(&split, ratatui::layout::Rect::new(0, 0, 96, 40));
    assert_eq!(split_plan.header.height, 0);
    assert_eq!(split_plan.operator_sidebar, None);

    let mut overlay = app::AppState::new_live(None, false, None);
    overlay.live_details_drawer_open = true;
    let overlay_plan =
        layout::FrameLayoutPlan::for_app(&overlay, ratatui::layout::Rect::new(0, 0, 80, 48));
    assert_eq!(overlay_plan.header.height, 0);
    assert!(overlay_plan.operator_sidebar.is_none());
    assert_eq!(
        overlay_plan.details_overlay,
        Some(ratatui::layout::Rect::new(36, 0, 42, 42))
    );

    let mut compact = app::AppState::new_live(None, false, None);
    compact.live_details_drawer_open = true;
    let compact_plan =
        layout::FrameLayoutPlan::for_app(&compact, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(compact_plan.header.height, 0);
    assert!(compact_plan.operator_sidebar.is_none());
    assert_eq!(
        compact_plan.details_overlay,
        Some(ratatui::layout::Rect::new(36, 0, 42, 18))
    );

    let mut dense = app::AppState::new_live(None, false, None);
    dense.live_details_drawer_open = true;
    let dense_plan =
        layout::FrameLayoutPlan::for_app(&dense, ratatui::layout::Rect::new(0, 0, 60, 18));
    assert_eq!(dense_plan.header.height, 0);
    assert!(dense_plan.operator_sidebar.is_none());
    assert!(dense_plan.details_overlay.is_none());
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParitySurface {
    StartupHome,
    LiveEmpty,
    LiveRun,
    CompletedPostRun,
    ReplayShell,
    OperatorSidebar,
    ReviewSurfaces,
    PermissionModal,
    CommandPalette,
    SlashOverlay,
    RuntimeStateOverlay,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellHierarchyContract {
    ComposeFirstHome,
    TranscriptFirstSession,
    OperatorSidebarSecondary,
    ReviewSecondary,
    InterruptiveOverlay,
    CommandOverlay,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromeContract {
    FocusedStartupCard,
    QuietSessionShell,
    SecondaryPane,
    ReviewShell,
    ElevatedModal,
    ElevatedCommandOverlay,
    ElevatedRuntimeOverlay,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerContract {
    StartupPrimaryCallToAction,
    LiveProgressiveDisclosure,
    DisabledLiveProgressiveDisclosure,
    ReplayReadOnlyProgressiveDisclosure,
    NotApplicable,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarContract {
    PersistentWhenGeometryAllows,
    SecondaryOnly,
    SuppressedByOverlay,
    NotApplicable,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceScopeContract {
    surface: ParitySurface,
    hierarchy: ShellHierarchyContract,
    chrome: ChromeContract,
    composer: ComposerContract,
    sidebar: SidebarContract,
    default_tab_chrome: bool,
    debug_inspector_in_primary_path: bool,
}

#[cfg(test)]
const FULL_SURFACE_SCOPE_MATRIX: [SurfaceScopeContract; 11] = [
    SurfaceScopeContract {
        surface: ParitySurface::StartupHome,
        hierarchy: ShellHierarchyContract::ComposeFirstHome,
        chrome: ChromeContract::FocusedStartupCard,
        composer: ComposerContract::StartupPrimaryCallToAction,
        sidebar: SidebarContract::NotApplicable,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::LiveEmpty,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::LiveProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::LiveRun,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::LiveProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::CompletedPostRun,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::DisabledLiveProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::ReplayShell,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::ReplayReadOnlyProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::OperatorSidebar,
        hierarchy: ShellHierarchyContract::OperatorSidebarSecondary,
        chrome: ChromeContract::SecondaryPane,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SecondaryOnly,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::ReviewSurfaces,
        hierarchy: ShellHierarchyContract::ReviewSecondary,
        chrome: ChromeContract::ReviewShell,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::NotApplicable,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::PermissionModal,
        hierarchy: ShellHierarchyContract::InterruptiveOverlay,
        chrome: ChromeContract::ElevatedModal,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::CommandPalette,
        hierarchy: ShellHierarchyContract::CommandOverlay,
        chrome: ChromeContract::ElevatedCommandOverlay,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::SlashOverlay,
        hierarchy: ShellHierarchyContract::CommandOverlay,
        chrome: ChromeContract::ElevatedCommandOverlay,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::RuntimeStateOverlay,
        hierarchy: ShellHierarchyContract::InterruptiveOverlay,
        chrome: ChromeContract::ElevatedRuntimeOverlay,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
];

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveMetadataHeadlineContract {
    Prohibited,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveMetadataPlacementContract {
    StatusOrFooterOnly,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HintDisclosureContract {
    ProgressiveBySpace,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerRowContract {
    NotPinnedToThreeRows,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveShellNoiseBudgetContract {
    dedicated_live_metadata_headline: LiveMetadataHeadlineContract,
    live_metadata_placement: LiveMetadataPlacementContract,
    hint_disclosure: HintDisclosureContract,
    composer_rows: ComposerRowContract,
    stable_shell_contexts: [ParitySurface; 9],
}

#[cfg(test)]
const LIVE_SHELL_NOISE_BUDGET: LiveShellNoiseBudgetContract = LiveShellNoiseBudgetContract {
    dedicated_live_metadata_headline: LiveMetadataHeadlineContract::Prohibited,
    live_metadata_placement: LiveMetadataPlacementContract::StatusOrFooterOnly,
    hint_disclosure: HintDisclosureContract::ProgressiveBySpace,
    composer_rows: ComposerRowContract::NotPinnedToThreeRows,
    stable_shell_contexts: [
        ParitySurface::StartupHome,
        ParitySurface::LiveEmpty,
        ParitySurface::LiveRun,
        ParitySurface::CompletedPostRun,
        ParitySurface::ReplayShell,
        ParitySurface::PermissionModal,
        ParitySurface::CommandPalette,
        ParitySurface::SlashOverlay,
        ParitySurface::RuntimeStateOverlay,
    ],
};

#[cfg(test)]
#[test]
fn full_surface_scope_matrix_is_defined() {
    assert_eq!(
        FULL_SURFACE_SCOPE_MATRIX,
        [
            SurfaceScopeContract {
                surface: ParitySurface::StartupHome,
                hierarchy: ShellHierarchyContract::ComposeFirstHome,
                chrome: ChromeContract::FocusedStartupCard,
                composer: ComposerContract::StartupPrimaryCallToAction,
                sidebar: SidebarContract::NotApplicable,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::LiveEmpty,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::LiveProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::LiveRun,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::LiveProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::CompletedPostRun,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::DisabledLiveProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::ReplayShell,
                hierarchy: ShellHierarchyContract::TranscriptFirstSession,
                chrome: ChromeContract::QuietSessionShell,
                composer: ComposerContract::ReplayReadOnlyProgressiveDisclosure,
                sidebar: SidebarContract::PersistentWhenGeometryAllows,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::OperatorSidebar,
                hierarchy: ShellHierarchyContract::OperatorSidebarSecondary,
                chrome: ChromeContract::SecondaryPane,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SecondaryOnly,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::ReviewSurfaces,
                hierarchy: ShellHierarchyContract::ReviewSecondary,
                chrome: ChromeContract::ReviewShell,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::NotApplicable,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::PermissionModal,
                hierarchy: ShellHierarchyContract::InterruptiveOverlay,
                chrome: ChromeContract::ElevatedModal,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::CommandPalette,
                hierarchy: ShellHierarchyContract::CommandOverlay,
                chrome: ChromeContract::ElevatedCommandOverlay,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::SlashOverlay,
                hierarchy: ShellHierarchyContract::CommandOverlay,
                chrome: ChromeContract::ElevatedCommandOverlay,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
            SurfaceScopeContract {
                surface: ParitySurface::RuntimeStateOverlay,
                hierarchy: ShellHierarchyContract::InterruptiveOverlay,
                chrome: ChromeContract::ElevatedRuntimeOverlay,
                composer: ComposerContract::NotApplicable,
                sidebar: SidebarContract::SuppressedByOverlay,
                default_tab_chrome: false,
                debug_inspector_in_primary_path: false,
            },
        ]
    );
}

#[cfg(test)]
#[test]
fn live_shell_noise_budget_contract_is_defined() {
    assert_eq!(
        LIVE_SHELL_NOISE_BUDGET,
        LiveShellNoiseBudgetContract {
            dedicated_live_metadata_headline: LiveMetadataHeadlineContract::Prohibited,
            live_metadata_placement: LiveMetadataPlacementContract::StatusOrFooterOnly,
            hint_disclosure: HintDisclosureContract::ProgressiveBySpace,
            composer_rows: ComposerRowContract::NotPinnedToThreeRows,
            stable_shell_contexts: [
                ParitySurface::StartupHome,
                ParitySurface::LiveEmpty,
                ParitySurface::LiveRun,
                ParitySurface::CompletedPostRun,
                ParitySurface::ReplayShell,
                ParitySurface::PermissionModal,
                ParitySurface::CommandPalette,
                ParitySurface::SlashOverlay,
                ParitySurface::RuntimeStateOverlay,
            ],
        }
    );
}

#[cfg(test)]
#[test]
fn legacy_three_row_composer_contract_removed() {
    assert_eq!(
        LIVE_SHELL_NOISE_BUDGET.composer_rows,
        ComposerRowContract::NotPinnedToThreeRows
    );

    let quiet_shell = [
        "Assistant · model-1",
        "┃",
        "┃",
        "┃  default · local/-",
        "Success  ·  run finished · session shell preserved  0  Ctrl+p commands  ·  ? help  ·  q quit",
    ];

    assert_live_shell_composer_progressive_disclosure(&quiet_shell, None, "Ctrl+p commands");
    assert!(find_line_containing(&quiet_shell, "Composer").is_none());
}

#[cfg(test)]
#[test]
fn live_shell_composer_contract_matches_shell_parity() {
    let ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 30, None, None, "Ctrl+p commands");

    let mut multiline = app::AppState::new_live(None, false, None);
    multiline.prompt_buffer = (1..=8)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    multiline.prompt_cursor = multiline.prompt_buffer.chars().count();

    let rendered = render_live_lines(&multiline, 100, 30);
    let lines = rendered.lines().collect::<Vec<_>>();
    let (_, first_input_row, last_shell_row) = live_shell_composer_input_span(&lines);

    assert!(find_line_containing_in_range(&lines, 0, last_shell_row + 1, "Composer ·").is_none());
    assert_eq!(
        lines[first_input_row..=last_shell_row]
            .iter()
            .filter(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("▎  line ")
                    || trimmed.starts_with("┃  line ")
                    || trimmed.starts_with("╹  line ")
            })
            .count(),
        6,
        "multiline composer should stay capped\n{rendered}"
    );
    assert!(
        lines[first_input_row].contains("line 3"),
        "cursor-following composer should keep the latest visible window in view\n{rendered}"
    );
    assert!(rendered.contains("line 8"));
}

#[cfg(test)]
#[test]
fn live_shell_composer_progressive_disclosure_by_width() {
    let ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 90, 36, None, None, "Ctrl+p commands");

    assert_live_shell_document_composer_contract(&ready, 80, 24, None, None, "Ctrl+p commands");

    assert_live_shell_document_composer_contract(&ready, 60, 18, None, None, "Ctrl+p commands");
}

#[cfg(test)]
#[test]
fn live_run_shell_places_under_input_controls_above_the_status_strip() {
    let mut app = app::AppState::new_live(None, false, None);
    let mut events = session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }

    assert_live_shell_document_composer_contract(&app, 100, 30, None, None, "Ctrl+p commands");
    assert_live_shell_document_composer_contract(&app, 80, 24, None, None, "Ctrl+p commands");

    let dense = render_live_lines(&app, 60, 18);
    assert!(!dense.contains("↑/↓ history"));
    assert!(!dense.contains("Enter send"));
}

#[cfg(test)]
#[test]
fn live_shell_composer_disabled_states_share_same_structure() {
    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    assert_live_shell_document_composer_contract(&degraded, 100, 30, None, None, "Degraded");

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    assert_live_shell_document_composer_contract(
        &disconnected,
        100,
        30,
        None,
        None,
        "Disconnected",
    );

    let mut failure = app::AppState::new_live(None, false, None);
    failure.set_status_banner(Some("runtime error while updating session".to_string()));
    assert_live_shell_document_composer_contract(&failure, 100, 30, None, None, "Failure");

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_task_6"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    assert_live_shell_document_composer_contract(&completed, 100, 30, None, None, "Tab focus");
}

#[cfg(test)]
#[test]
fn compact_geometry_uses_overlay_sidebar_and_minimal_footer() {
    let mut compact = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        compact.ingest_event(event);
    }
    compact.live_details_drawer_open = true;
    let compact_plan =
        layout::FrameLayoutPlan::for_app(&compact, ratatui::layout::Rect::new(0, 0, 80, 24));
    assert!(compact_plan.operator_sidebar.is_none());
    assert!(compact_plan.details_overlay.is_some());

    let compact_render = render_live_lines(&compact, 80, 24);
    assert!(!compact_render.contains("run run_fixture ·"));
    assert!(!compact_render.contains("e events"));

    let mut dense = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        dense.ingest_event(event);
    }
    dense.live_details_drawer_open = true;
    let dense_plan =
        layout::FrameLayoutPlan::for_app(&dense, ratatui::layout::Rect::new(0, 0, 60, 18));
    assert!(dense_plan.details_overlay.is_none());

    let dense_render = render_live_lines(&dense, 60, 18);
    assert!(!dense_render.contains("run run_fixture"));
    assert!(!dense_render.contains("i details"));
    assert!(!dense_render.contains("No MCP integrations configured"));
}

#[cfg(test)]
#[test]
fn focus_order_cycles_transcript_sidebar_composer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.active_tab = app::Tab::Run;
    app.live_details_drawer_open = true;

    app.handle_key(focus_cycle_key());
    assert_eq!(app.focus, app::Focus::List);
    assert!(app.details_drawer_open());

    app.handle_key(focus_cycle_key());
    assert_eq!(app.focus, app::Focus::Prompt);
    assert!(!app.details_drawer_open());

    app.handle_key(focus_cycle_key());
    assert_eq!(app.focus, app::Focus::Details);
    assert!(!app.details_drawer_open());
}

#[cfg(test)]
#[test]
fn hovered_wheel_target_uses_sidebar_overlay_hit_areas() {
    let mut app = app::AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let plan = layout::FrameLayoutPlan::for_app(&app, area);
    let overlay = plan.details_overlay.expect("overlay sidebar area");
    let transcript = plan.transcript.expect("transcript area");

    let overlay_column = overlay.x.saturating_add(1);
    let overlay_row = overlay.y.saturating_add(1);
    let transcript_column = transcript.x.saturating_add(1);
    let transcript_row = transcript.y.saturating_add(1);

    assert_eq!(plan.wheel_hit_areas.overlay, Some(overlay));
    assert_eq!(
        ui::hovered_wheel_target(&app, area, overlay_column, overlay_row),
        Some(ui::WheelTarget::Inspector)
    );
    assert_eq!(
        ui::hovered_wheel_target(&app, area, transcript_column, transcript_row),
        Some(ui::WheelTarget::Transcript)
    );
}

#[cfg(test)]
#[test]
fn session_view_tracks_request_turn_and_tool_state() {
    let events = session_view_events();

    let mut live = app::AppState::new_live(None, false, None);
    for event in events.clone() {
        live.ingest_event(event);
    }
    assert_session_view_state(&live);

    let replay = app::AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), events);
    assert_session_view_state(&replay);
}

#[cfg(test)]
#[test]
fn session_view_ignores_duplicate_seq_without_losing_ui_state() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));
    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(app.active_permission().is_some());
    assert!(app.permission_submission_pending("perm_1"));

    app.focus = app::Focus::Prompt;
    app.prompt_buffer = "draft".to_string();
    app.prompt_cursor = "draft".chars().count();

    app.ingest_event(envelope(
        1,
        Some("req_duplicate"),
        harness_core::event::EventV1::RunStarted(harness_core::event::RunStartedEvent {
            run_name: "duplicate-seq".to_string(),
            workspace_root: "/tmp".to_string(),
        }),
    ));

    assert_eq!(app.events.len(), 1);
    assert!(app.active_permission().is_some());
    assert!(app.permission_submission_pending("perm_1"));
    assert_eq!(app.prompt_buffer, "draft");
    assert_eq!(app.prompt_cursor, "draft".chars().count());
}

#[cfg(test)]
#[test]
fn orchestration_projection_resolves_owner_labels() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));

    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        None,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_supervisor".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:supervisor".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        None,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_system".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("tool:shell.run".to_string()),
        }),
    ));

    let summary = app.orchestration_summary();
    assert_eq!(
        summary,
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 2,
            running: 1,
            stale: 0,
        }
    );

    let rows = app.orchestration_visible_rows();
    assert_eq!(
        rows.iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_supervisor", "task_system", "task_worker"]
    );

    let worker = rows
        .iter()
        .find(|row| row.task_id == "task_worker")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(worker),
        crate::app::OrchestrationOwnerLabels {
            label: "agent_worker".to_string(),
            profile: "researcher".to_string(),
        }
    );

    let supervisor = rows
        .iter()
        .find(|row| row.task_id == "task_supervisor")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(supervisor),
        crate::app::OrchestrationOwnerLabels {
            label: "supervisor".to_string(),
            profile: "n/a".to_string(),
        }
    );

    let system = rows
        .iter()
        .find(|row| row.task_id == "task_system")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(system),
        crate::app::OrchestrationOwnerLabels {
            label: "system".to_string(),
            profile: "n/a".to_string(),
        }
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_ignores_duplicate_seq_events() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_dup".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_dup".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    assert_eq!(app.orchestration_visible_rows().len(), 1);
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("stale for 3001 ms")
    );

    app.ingest_event(envelope_with_actor(
        1,
        None,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "rewritten".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_dup".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_dup".to_string(),
            stale_for_ms: 9999,
        }),
    ));

    assert_eq!(app.events.len(), 3);
    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, crate::app::OrchestrationTaskState::Stale);
    assert_eq!(rows[0].queue_key.as_deref(), Some("agent:running"));
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("stale for 3001 ms")
    );
    assert_eq!(
        app.orchestration_owner_labels(&rows[0]),
        crate::app::OrchestrationOwnerLabels {
            label: "agent_worker".to_string(),
            profile: "researcher".to_string(),
        }
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_tracks_queued_started_completed_counts() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));

    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_primary".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:primary".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 0,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_worker_primary",
            Some("agent:queued:primary"),
            None,
            crate::app::OrchestrationTaskState::Queued,
        )
    );

    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_primary".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:primary".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 1,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_worker_primary",
            Some("agent:running:primary"),
            None,
            crate::app::OrchestrationTaskState::Running,
        )
    );

    app.ingest_event(envelope_with_actor(
        4,
        Some("req_worker_secondary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_secondary".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:secondary".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 1,
            stale: 0,
        },
        "active_agents must count unique worker owners only"
    );
    assert_eq!(
        app.orchestration_visible_rows()
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_worker_primary", "task_worker_secondary"]
    );

    app.ingest_event(envelope_with_actor(
        5,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_worker_primary".to_string(),
            result_summary: "primary completed".to_string(),
            result_digest: "digest-primary".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 0,
            stale: 0,
        }
    );

    app.ingest_event(envelope_with_actor(
        6,
        Some("req_worker_secondary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_worker_secondary".to_string(),
            result_summary: "secondary completed".to_string(),
            result_digest: "digest-secondary".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 0,
            stale: 0,
        }
    );
    assert_eq!(
        app.orchestration_visible_rows()
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_worker_secondary", "task_worker_primary"]
    );

    app.ingest_event(envelope_with_actor(
        7,
        None,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_supervisor_only".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:supervisor".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 1,
            stale: 0,
        },
        "non-worker rows must not contribute to active_agents"
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_tracks_stale_then_late_result() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_stale".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:stale".to_string()),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 1,
            stale: 0,
        }
    );

    app.ingest_event(envelope_with_actor(
        3,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_stale".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_stale",
            Some("agent:running:stale"),
            Some("stale for 3001 ms"),
            crate::app::OrchestrationTaskState::Stale,
        )
    );

    app.ingest_event(envelope_with_actor(
        4,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskResultLate(harness_core::event::TaskResultLateEvent {
            task_id: "task_stale".to_string(),
            result_digest: "digest-late".to_string(),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 0,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(
        rows.len(),
        1,
        "late result must update the stale row in place"
    );
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_stale",
            Some("agent:running:stale"),
            Some("late result after stale cancellation"),
            crate::app::OrchestrationTaskState::LateResult,
        )
    );
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("late result after stale cancellation")
    );
}

#[cfg(test)]
#[test]
fn orchestration_projection_retains_only_recent_terminal_rows() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_live_stale".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:live".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_live_stale".to_string(),
            stale_for_ms: 4242,
        }),
    ));
    app.ingest_event(envelope(
        3,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_live_queued".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:live".to_string()),
        }),
    ));

    app.ingest_event(envelope(
        4,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_1".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q1".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        5,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_1".to_string(),
            result_summary: "terminal 1 completed".to_string(),
            result_digest: "digest-terminal-1".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        6,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_2".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q2".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        7,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_terminal_2".to_string(),
            reason: "cancelled 2".to_string(),
            task_scope: None,
        }),
    ));
    app.ingest_event(envelope(
        8,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_3".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q3".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        9,
        None,
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_terminal_3".to_string(),
            stale_for_ms: 9003,
        }),
    ));
    app.ingest_event(envelope(
        10,
        None,
        harness_core::event::EventV1::TaskResultLate(harness_core::event::TaskResultLateEvent {
            task_id: "task_terminal_3".to_string(),
            result_digest: "digest-terminal-3".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        11,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_4".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q4".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        12,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_4".to_string(),
            result_summary: "terminal 4 completed".to_string(),
            result_digest: "digest-terminal-4".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_5".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q5".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        14,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_terminal_5".to_string(),
            reason: "cancelled 5".to_string(),
            task_scope: None,
        }),
    ));
    app.ingest_event(envelope(
        15,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_6".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q6".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        16,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_6".to_string(),
            result_summary: "terminal 6 completed".to_string(),
            result_digest: "digest-terminal-6".to_string(),
            metadata: None,
        }),
    ));

    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 7);
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.task_id.as_str(),
                row.queue_key.as_deref(),
                row.warning.as_deref(),
                row.state,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "task_live_stale",
                Some("agent:running:live"),
                Some("stale for 4242 ms"),
                crate::app::OrchestrationTaskState::Stale,
            ),
            (
                "task_live_queued",
                Some("agent:queued:live"),
                None,
                crate::app::OrchestrationTaskState::Queued,
            ),
            (
                "task_terminal_6",
                Some("terminal:q6"),
                None,
                crate::app::OrchestrationTaskState::Completed,
            ),
            (
                "task_terminal_5",
                Some("terminal:q5"),
                Some("cancelled 5"),
                crate::app::OrchestrationTaskState::Cancelled,
            ),
            (
                "task_terminal_4",
                Some("terminal:q4"),
                None,
                crate::app::OrchestrationTaskState::Completed,
            ),
            (
                "task_terminal_3",
                Some("terminal:q3"),
                Some("late result after stale cancellation"),
                crate::app::OrchestrationTaskState::LateResult,
            ),
            (
                "task_terminal_2",
                Some("terminal:q2"),
                Some("cancelled 2"),
                crate::app::OrchestrationTaskState::Cancelled,
            ),
        ]
    );
    assert!(
        !rows.iter().any(|row| row.task_id == "task_terminal_1"),
        "terminal retention must drop the oldest terminal row once six exist"
    );
}

#[cfg(test)]
pub(crate) fn session_view_events() -> Vec<harness_core::event::EventEnvelopeV1> {
    vec![
        envelope(
            1,
            Some("req_001"),
            harness_core::event::EventV1::UserMessageSubmitted(
                harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_001".to_string(),
                    text: "Explain the refactor".to_string(),
                },
            ),
        ),
        envelope(
            2,
            Some("req_001"),
            harness_core::event::EventV1::ProviderRequestStarted(
                harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_001".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5-codex".to_string(),
                    prompt_summary: "Explain the refactor".to_string(),
                    request_digest: "digest-req-001".to_string(),
                    metadata: None,
                },
            ),
        ),
        envelope(
            3,
            Some("req_001"),
            harness_core::event::EventV1::ProviderStreamDelta(
                harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_001".to_string(),
                    delta: "Working through the steps.".to_string(),
                },
            ),
        ),
        envelope(
            4,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallRequested(
                harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                    tool_id: "fs.read".to_string(),
                    args_summary: r#"{"path":"src/app.rs"}"#.to_string(),
                    args_digest: "digest-tool-args".to_string(),
                    metadata: None,
                },
            ),
        ),
        permission_requested_event(5, "perm_1", "tool_call_1"),
        permission_resolved_event(6, "perm_1", harness_core::perm::PermissionDecision::Allow),
        envelope(
            7,
            Some("req_001"),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "tool_call_1".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("tool:fs.read".to_string()),
            }),
        ),
        envelope(
            8,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallStarted(
                harness_core::event::ToolCallStartedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                },
            ),
        ),
        envelope(
            9,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallFinished(
                harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("tool output".to_string()),
                    output_digest: Some("digest-tool-output".to_string()),
                    output_json: None,
                    metadata: None,
                },
            ),
        ),
        envelope(
            10,
            Some("req_001"),
            harness_core::event::EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: "req_001".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-final".to_string()),
                    usage: None,
                    metadata: None,
                },
            ),
        ),
    ]
}

#[cfg(test)]
fn orchestration_details_drawer_events(extra_terminal_rows: usize) -> Vec<EventEnvelopeV1> {
    let mut events = session_view_events();
    events.extend([
        envelope(
            11,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "w1".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            12,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "w2".to_string(),
                profile: "scout".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            13,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w1".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_stale".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some("scan".to_string()),
            }),
        ),
        envelope_with_actor(
            14,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w1".to_string()),
            ),
            harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
                task_id: "task_stale".to_string(),
                stale_for_ms: 3001,
            }),
        ),
        envelope_with_actor(
            15,
            Some("req_001"),
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_run".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: None,
            }),
        ),
        envelope_with_actor(
            16,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::System,
                Some("coordinator".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_queue".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("tool:read".to_string()),
            }),
        ),
        envelope_with_actor(
            17,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_done".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some("tool:done".to_string()),
            }),
        ),
        envelope_with_actor(
            18,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
                task_id: "task_done".to_string(),
                result_summary: "done".to_string(),
                result_digest: "digest-task-done".to_string(),
                metadata: None,
            }),
        ),
    ]);

    let mut seq = 19;
    for index in 0..extra_terminal_rows {
        let task_id = format!("task_tail_{index}");
        events.push(envelope_with_actor(
            seq,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: task_id.clone(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some(format!("tail:{index}")),
            }),
        ));
        seq += 1;
        events.push(envelope_with_actor(
            seq,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
                task_id,
                result_summary: format!("tail {index} done"),
                result_digest: format!("digest-tail-{index}"),
                metadata: None,
            }),
        ));
        seq += 1;
    }

    events
}

#[cfg(test)]
fn orchestration_details_drawer_app(extra_terminal_rows: usize) -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    for event in orchestration_details_drawer_events(extra_terminal_rows) {
        app.ingest_event(event);
    }
    app.handle_key(focus_cycle_key());
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));
    app
}

#[cfg(test)]
fn assert_session_view_state(app: &app::AppState) {
    assert_eq!(app.activities.len(), 1);

    let activity = app.activities.front().expect("activity exists");
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.provider_id, "openai");
    assert_eq!(activity.model_id, "gpt-5-codex");
    assert_eq!(activity.status, app::ActivityStatus::Done);
    assert_eq!(activity.thinking_text, "Working through the steps.");
    assert_eq!(activity.transcript_text, "");
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("Explain the refactor")
    );

    assert_eq!(activity.tool_calls.len(), 1);
    let tool_call = activity.tool_calls.first().expect("tool call exists");
    assert_eq!(tool_call.tool_call_id, "tool_call_1");
    assert_eq!(tool_call.tool_id, "fs.read");
    assert_eq!(tool_call.status, app::ToolCallDisplayStatus::Succeeded);
    assert_eq!(tool_call.output_summary.as_deref(), Some("tool output"));
    assert_eq!(tool_call.truncated_output.as_deref(), Some("tool output"));

    assert!(app.active_permission().is_none());
}

#[cfg(test)]
fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

#[cfg(test)]
fn focus_cycle_key() -> crossterm::event::KeyEvent {
    key_with_modifiers(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::CONTROL,
    )
}

#[cfg(test)]
pub(super) fn key_with_modifiers(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
}

#[cfg(test)]
pub(super) fn render_live_buffer(app: &app::AppState, width: u16, height: u16) -> String {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .expect("draw frame");
    format!("{:?}", terminal.backend().buffer())
}

#[cfg(test)]
pub(super) fn render_live_cells(
    app: &app::AppState,
    width: u16,
    height: u16,
) -> ratatui::buffer::Buffer {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .expect("draw frame");
    terminal.backend().buffer().clone()
}

#[cfg(test)]
pub(super) fn row_text_and_palette(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    needle: &str,
) -> Option<(
    String,
    Vec<ratatui::style::Color>,
    Vec<ratatui::style::Color>,
)> {
    buffer.content.chunks(width as usize).find_map(|row| {
        let text = row.iter().map(|cell| cell.symbol()).collect::<String>();
        text.contains(needle).then(|| {
            (
                text,
                row.iter().map(|cell| cell.fg).collect::<Vec<_>>(),
                row.iter().map(|cell| cell.bg).collect::<Vec<_>>(),
            )
        })
    })
}

#[cfg(test)]
fn row_at(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    row_index: usize,
) -> Option<(
    String,
    Vec<ratatui::style::Color>,
    Vec<ratatui::style::Color>,
)> {
    buffer
        .content
        .chunks(width as usize)
        .nth(row_index)
        .map(|row| {
            (
                row.iter().map(|cell| cell.symbol()).collect::<String>(),
                row.iter().map(|cell| cell.fg).collect::<Vec<_>>(),
                row.iter().map(|cell| cell.bg).collect::<Vec<_>>(),
            )
        })
}

#[cfg(test)]
fn assert_selected_overlay_row_uses_highlight(
    app: &app::AppState,
    width: u16,
    height: u16,
    needle: &str,
    expected_bg: ratatui::style::Color,
) {
    let buffer = render_live_cells(app, width, height);
    let (row, fgs, bgs) = row_text_and_palette(&buffer, width, needle)
        .unwrap_or_else(|| panic!("missing selected overlay row {needle:?}"));
    let start_byte = row
        .find(needle)
        .expect("row contains selected overlay needle");
    let start = row[..start_byte].chars().count();
    let end = start + needle.chars().count();

    assert!(
        fgs[start..end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Rgb(0x0A, 0x0A, 0x0A)),
        "selected overlay row should use inverse foreground for {needle:?}\n{row}"
    );
    assert!(
        bgs[start..end].iter().all(|color| *color == expected_bg),
        "selected overlay row should use the expected background for {needle:?}\n{row}"
    );
}

#[cfg(test)]
fn assert_row_segment_palette(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    needle: &str,
    expected_fg: ratatui::style::Color,
    expected_bg: ratatui::style::Color,
) {
    let (row, fgs, bgs) = row_text_and_palette(buffer, width, needle)
        .unwrap_or_else(|| panic!("missing row for {needle:?}"));
    let start_byte = row.find(needle).expect("row contains helper substring");
    let start = row[..start_byte].chars().count();
    let end = start + needle.chars().count();

    assert!(
        fgs[start..end].iter().all(|color| *color == expected_fg),
        "row should use the expected helper foreground for {needle:?}\n{row}"
    );
    assert!(
        bgs[start..end].iter().all(|color| *color == expected_bg),
        "row should use the expected helper background for {needle:?}\n{row}"
    );
}

#[cfg(test)]
fn assert_row_segment_background(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    needle: &str,
    expected_bg: ratatui::style::Color,
) {
    let (row, _, bgs) = row_text_and_palette(buffer, width, needle)
        .unwrap_or_else(|| panic!("missing row for {needle:?}"));
    let start_byte = row.find(needle).expect("row contains helper substring");
    let start = row[..start_byte].chars().count();
    let end = start + needle.chars().count();

    assert!(
        bgs[start..end].iter().all(|color| *color == expected_bg),
        "row should use the expected helper background for {needle:?}\n{row}"
    );
}

#[cfg(test)]
fn assert_alphanumeric_row_palette(
    buffer: &ratatui::buffer::Buffer,
    width: u16,
    row_index: usize,
    expected_fg: ratatui::style::Color,
    expected_bg: ratatui::style::Color,
    label: &str,
) {
    let (row, fgs, bgs) = row_at(buffer, width, row_index)
        .unwrap_or_else(|| panic!("missing row {row_index} for {label}"));
    let semantic_columns = row
        .chars()
        .enumerate()
        .filter_map(|(index, ch)| ch.is_alphanumeric().then_some(index))
        .collect::<Vec<_>>();

    assert!(
        !semantic_columns.is_empty(),
        "{label} row should contain semantic content\n{row}"
    );
    assert!(
        semantic_columns
            .iter()
            .all(|index| fgs[*index] == expected_fg),
        "{label} row should use the expected foreground palette\n{row}"
    );
    assert!(
        semantic_columns
            .iter()
            .all(|index| bgs[*index] == expected_bg),
        "{label} row should use the expected background palette\n{row}"
    );
}

#[cfg(test)]
pub(super) fn transcript_code_block_app(language: &str) -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_code_block";

    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "show code".to_string(),
                request_digest: "digest-code".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: format!(
                    "Here is a sample:\n```{language}\nfn main() {{\n    let answer = 42;\n}}\n```"
                ),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-code-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    app
}

#[cfg(test)]
pub(super) fn transcript_diff_block_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_diff_block";

    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "show diff".to_string(),
                request_digest: "digest-diff-block".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "```diff\n--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n```".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-diff-block-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    app
}

#[cfg(test)]
fn run_palette_command(app: &mut app::AppState, query: &str) {
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for c in query.chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
}

#[cfg(test)]
fn rich_transcript_fixture_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_rich_shell";

    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Restyle the transcript shell".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Restyle the transcript shell".to_string(),
                request_digest: "digest-rich-shell-request".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Drafting a document-like plan".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_rich_read".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs","start_line":1,"limit":24}"#.to_string(),
                args_digest: "digest-rich-shell-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_rich_read".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        6,
        Some(request_id),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_rich_read".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("24 lines read from src/ui.rs".to_string()),
                output_digest: Some("digest-rich-shell-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        7,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Found the transcript renderer and the composer chrome.".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        8,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-rich-shell-finished".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    app
}

#[cfg(test)]
fn multi_turn_transcript_fixture_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);

    for (seq, request_id, user_text, reply_text) in [
        (
            1_u64,
            "req_turn_one",
            "Summarize the current shell",
            "The shell is transcript-first and calm.",
        ),
        (
            10_u64,
            "req_turn_two",
            "Tighten the transcript spacing",
            "Spacing is collapsed without losing turn boundaries.",
        ),
    ] {
        app.ingest_event(envelope(
            seq,
            Some(request_id),
            harness_core::event::EventV1::UserMessageSubmitted(
                harness_core::event::UserMessageSubmittedEvent {
                    request_id: request_id.to_string(),
                    text: user_text.to_string(),
                },
            ),
        ));
        app.ingest_event(envelope(
            seq + 1,
            Some(request_id),
            harness_core::event::EventV1::ProviderRequestStarted(
                harness_core::event::ProviderRequestStartedEvent {
                    request_id: request_id.to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: user_text.to_string(),
                    request_digest: format!("digest-{request_id}"),
                    metadata: None,
                },
            ),
        ));
        app.ingest_event(envelope(
            seq + 2,
            Some(request_id),
            harness_core::event::EventV1::ProviderStreamDelta(
                harness_core::event::ProviderStreamDeltaEvent {
                    request_id: request_id.to_string(),
                    delta: reply_text.to_string(),
                },
            ),
        ));
        app.ingest_event(envelope(
            seq + 3,
            Some(request_id),
            harness_core::event::EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: request_id.to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some(format!("digest-finished-{request_id}")),
                    usage: None,
                    metadata: None,
                },
            ),
        ));
    }

    app
}

#[cfg(test)]
fn render_live_screen(app: &app::AppState, width: u16, height: u16) -> String {
    let debug = render_live_buffer(app, width, height);
    let mut in_content = false;
    let mut rows = Vec::new();

    for line in debug.lines() {
        if line.trim() == "content: [" {
            in_content = true;
            continue;
        }
        if in_content && line.trim() == "]," {
            break;
        }
        if !in_content {
            continue;
        }

        let trimmed = line.trim();
        if let Some(content) = trimmed
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix("\","))
        {
            rows.push(content.to_string());
        }
    }

    rows.join("\n")
}

#[cfg(test)]
#[test]
fn permission_modal_snapshot_renders_request() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission required",
            "Allow once",
            "Allow always",
            "enter",
            "⇆",
        ],
    );
}

#[cfg(test)]
fn module_overlays_share_elevated_card_language() {
    let width = 120;
    let height = 30;
    let mut palette = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        None,
    );
    palette.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_render = render_live_lines(&palette, width, height);
    assert!(palette_render.contains("Commands"));
    assert_selected_overlay_row_uses_highlight(
        &palette,
        width,
        height,
        "New session",
        ratatui::style::Color::Rgb(0xF5, 0xA7, 0x42),
    );

    let mut sessions = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        None,
    );
    sessions.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    let sessions_render = render_live_lines(&sessions, width, height);
    assert!(sessions_render.contains("Continue session"));
    assert_selected_overlay_row_uses_highlight(
        &sessions,
        width,
        height,
        "Resume target",
        ratatui::style::Color::Rgb(0xF5, 0xA7, 0x42),
    );
}

#[cfg(test)]
fn module_quiet_overlay_helper_rows_use_semantic_chrome_palette() {
    let width = 120;
    let height = 30;

    let mut palette = app::AppState::new_startup(Vec::new(), None);
    palette.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_buffer = render_live_cells(&palette, width, height);
    assert_row_segment_palette(
        &palette_buffer,
        width,
        "Commands",
        ratatui::style::Color::Rgb(0xEE, 0xEE, 0xEE),
        ratatui::style::Color::Rgb(0x14, 0x14, 0x14),
    );

    let mut sessions = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        None,
    );
    sessions.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    let sessions_buffer = render_live_cells(&sessions, width, height);
    assert_row_segment_palette(
        &sessions_buffer,
        width,
        "Continue session",
        ratatui::style::Color::Rgb(0xEE, 0xEE, 0xEE),
        ratatui::style::Color::Rgb(0x14, 0x14, 0x14),
    );
}

#[cfg(test)]
fn module_live_shell_redesign_preserves_replay_overlay_and_permission_parity() {
    startup_and_live_empty_share_spacing_contract();
    compact_geometry_uses_overlay_sidebar_and_minimal_footer();
    hovered_wheel_target_uses_sidebar_overlay_hit_areas();
    module_permission_modal_remains_visually_dominant_and_fail_closed();

    let theme = Theme::default();

    let mut replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    replay.transcript_scroll = usize::MAX;
    let replay_plan = FrameLayoutPlan::for_app(&replay, ratatui::layout::Rect::new(0, 0, 100, 30));
    let replay_render = render_live_lines(&replay, 100, 30);
    let replay_buffer = render_live_cells(&replay, 100, 30);
    let replay_lines = replay_render.lines().collect::<Vec<_>>();
    assert!(replay_plan.live_anchor.is_none());
    assert!(replay_plan.operator_sidebar.is_some());
    let replay_header_row = find_line_containing_all(&replay_lines, &["Replay", "read-only"])
        .unwrap_or_else(|| {
            panic!("replay header should preserve replay identity\n{replay_render}")
        });
    let replay_disabled_row = find_line_containing_all_from(
        &replay_lines,
        replay_header_row + 1,
        &["▎", "Replay is read-only."],
    )
    .filter(|row| !replay_lines[*row].contains("run "))
    .unwrap_or_else(|| {
        panic!("replay shell should preserve a disabled composer row\n{replay_render}")
    });
    let replay_shortcuts_row =
        find_line_containing_from(&replay_lines, replay_disabled_row + 1, "shortcuts")
            .unwrap_or_else(|| {
                panic!("replay shell should preserve shortcut guidance\n{replay_render}")
            });
    let user_row = find_line_containing(&replay_lines, "Explain the refactor")
        .unwrap_or_else(|| panic!("replay shell should preserve the user turn\n{replay_render}"));
    let thinking_row =
        find_line_containing_all_from(&replay_lines, user_row + 1, &["Working through the steps."])
            .unwrap_or_else(|| {
                panic!("replay shell should preserve visible thinking text\n{replay_render}")
            });

    assert!(replay_header_row < replay_disabled_row && replay_disabled_row < replay_shortcuts_row);
    assert!(
        user_row < thinking_row,
        "replay transcript should preserve turn order\n{replay_render}"
    );
    assert_alphanumeric_row_palette(
        &replay_buffer,
        100,
        replay_disabled_row,
        theme.status.disabled,
        theme.surface.shell,
        "replay disabled composer",
    );
    assert_row_segment_palette(
        &replay_buffer,
        100,
        "? shortcuts",
        theme.text.secondary,
        theme.surface.shell,
    );

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    let degraded_buffer = render_live_cells(&degraded, 80, 24);
    assert_row_segment_background(
        &degraded_buffer,
        80,
        "Recovery in progress",
        theme.surface.overlay,
    );

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    let disconnected_buffer = render_live_cells(&disconnected, 80, 24);
    assert_row_segment_background(
        &disconnected_buffer,
        80,
        "Connection lost",
        theme.surface.overlay,
    );

    let mut failure = app::AppState::new_live(None, false, None);
    failure.set_status_banner(Some(
        "runtime error: exit code 1\nstderr permission denied".to_string(),
    ));
    let failure_buffer = render_live_cells(&failure, 80, 24);
    assert_row_segment_background(
        &failure_buffer,
        80,
        "Review required",
        theme.surface.overlay,
    );
}

#[cfg(test)]
fn module_permission_modal_remains_visually_dominant_and_fail_closed() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.ingest_event(permission_requested_event(
        1,
        "perm_dominant_fail_closed",
        "tool_call_dominant_fail_closed",
    ));

    let rendered = render_live_lines(&app, 100, 24);
    let buffer = render_live_cells(&app, 100, 24);
    let theme = Theme::default();
    let (row, _, bgs) = row_text_and_palette(&buffer, 100, "Allow once").expect("allow chip row");
    let start_byte = row.find("Allow once").expect("chip substring");
    let start = row[..start_byte].chars().count();
    let end = start + "Allow once".chars().count();

    assert_eq!(
        app.overlay_stack().ordered(),
        &[overlay::OverlayKind::PermissionModal]
    );
    assert!(!app.palette_visible);
    assert!(rendered.contains("Permission required"));
    assert!(rendered.contains("Allow once"));
    assert!(rendered.contains("Allow always"));
    assert!(rendered.contains("enter"));
    assert!(rendered.contains("⇆"));
    assert!(!rendered.contains("Commands"));
    assert!(
        bgs[start..end]
            .iter()
            .all(|color| *color == theme.status.warning),
        "selected allow chip should stay stronger than quiet command overlays\n{row}"
    );
}

#[cfg(test)]
#[test]
fn overlay_stack_orders_details_palette_permission() {
    let mut app = app::AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[
            overlay::OverlayKind::DetailsDrawer,
            overlay::OverlayKind::CommandPalette,
        ]
    );

    app.ingest_event(permission_requested_event(
        1,
        "perm_stack_order",
        "tool_call_stack_order",
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[
            overlay::OverlayKind::DetailsDrawer,
            overlay::OverlayKind::PermissionModal,
        ]
    );
}

#[cfg(test)]
#[test]
fn permission_modal_preempts_palette() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_palette",
        "tool_call_preempt_palette",
    ));

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(!app.palette_visible);
    assert!(app.palette_input.is_empty());
    assert_eq!(
        app.overlay_stack().top(),
        Some(overlay::OverlayKind::PermissionModal)
    );

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_preempt_palette".to_string(),
            decision: harness_core::perm::PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }]
    );
}

#[cfg(test)]
#[test]
fn focus_returns_after_palette_close() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.prompt_buffer = "keep prompt draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    assert!(app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");
    let open_debug = render_live_screen(&app, 120, 36);
    println!("PALETTE_OPEN\n{open_debug}");

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");
    assert_eq!(app.prompt_cursor, "keep prompt draft".chars().count());
    let closed_debug = render_live_screen(&app, 100, 24);
    println!("PALETTE_CLOSED\n{closed_debug}");
}

#[cfg(test)]
#[test]
fn live_status_strip_distinguishes_terminal_states() {
    let ready = app::AppState::new_live(None, false, None);
    assert_eq!(ready.runtime_state().kind, app::RuntimeStateKind::Ready);

    let mut sending = app::AppState::new_live(None, false, None);
    for c in "hello".chars() {
        sending.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    sending.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(sending.runtime_state().kind, app::RuntimeStateKind::Sending);

    sending.ingest_event(envelope(
        1,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_phase".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "hello".to_string(),
                request_digest: "digest-phase".to_string(),
                metadata: None,
            },
        ),
    ));
    sending.ingest_event(envelope(
        2,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_phase".to_string(),
                delta: "streaming text".to_string(),
            },
        ),
    ));

    assert_eq!(
        sending.runtime_state().kind,
        app::RuntimeStateKind::Streaming
    );

    sending.ingest_event(envelope(
        3,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_phase".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    assert!(!matches!(
        sending.runtime_state().kind,
        app::RuntimeStateKind::Sending | app::RuntimeStateKind::Streaming
    ));

    let mut cancelled = app::AppState::new_live(None, false, None);
    cancelled.ingest_event(envelope(
        1,
        Some("req_cancel"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_cancel".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "cancel".to_string(),
                request_digest: "digest-cancel".to_string(),
                metadata: None,
            },
        ),
    ));
    cancelled.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "req_cancel".to_string(),
            reason: "operator cancelled".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));
    let cancelled_debug = render_live_buffer(&cancelled, 80, 24);
    assert_eq!(
        cancelled.runtime_state().kind,
        app::RuntimeStateKind::Cancelled
    );
    assert!(!cancelled_debug.contains("request_digest="));

    let mut errored = app::AppState::new_live(None, false, None);
    errored.ingest_event(envelope(
        1,
        Some("req_error"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_error".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "fail".to_string(),
                request_digest: "digest-error".to_string(),
                metadata: None,
            },
        ),
    ));
    errored.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "API rate limit exceeded".to_string(),
        }),
    ));
    let error_debug = render_live_buffer(&errored, 80, 24);
    assert!(error_debug.contains("API rate limit exceeded"));

    let mut permission_blocked = app::AppState::new_live(None, false, None);
    permission_blocked.ingest_event(permission_requested_event(1, "perm_blocked", "tool_call_1"));
    let permission_blocked_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_blocked_debug.contains("Permission required"));
    assert!(permission_blocked_debug.contains("Allow once"));
    assert!(permission_blocked_debug.contains("Allow always"));
    assert!(permission_blocked_debug.contains("enter"));
    assert!(permission_blocked_debug.contains("⇆"));

    permission_blocked.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let permission_pending_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_pending_debug.contains("decision sent"));
    assert!(permission_pending_debug.contains("awaiting confirmation"));

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    let degraded_debug = render_live_buffer(&degraded, 80, 24);
    assert!(degraded_debug.contains("Degraded"));
    assert!(degraded_debug.contains("replaying from seq 1"));
    assert!(!degraded_debug.contains("Composer ·"));
    assert!(!degraded_debug.contains("Draft preserved locally"));
    assert!(degraded_debug.contains("Draft locally until recovery completes."));
    assert!(degraded_debug.contains("Recovery in progress"));

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    let disconnected_debug = render_live_buffer(&disconnected, 80, 24);
    assert!(disconnected_debug.contains("Disconnected"));
    assert!(!disconnected_debug.contains("Composer ·"));
    assert!(!disconnected_debug.contains("Draft preserved locally"));
    assert!(disconnected_debug.contains("Reopen the TUI, then continue from the transcript."));
}

#[cfg(test)]
#[test]
fn run_finished_keeps_transcript_and_ready_composer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_done".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "finished".to_string(),
                request_digest: "digest-done".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_done"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_done".to_string(),
                delta: "transcript remains visible".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_done".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-done-out".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    insta::with_settings!({ prepend_module_to_snapshot => false }, {
        insta::with_settings!({ prepend_module_to_snapshot => false }, {
        insta::assert_snapshot!("harness_tui__live_shell_finished_state", render_live_lines(&app, 80, 24));
    });
    });
}

#[cfg(test)]
#[test]
fn streaming_transcript_auto_scrolls_to_latest_wrapped_content() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_scroll"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_scroll".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "scroll test".to_string(),
                request_digest: "digest-scroll".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_scroll"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_scroll".to_string(),
                delta: [
                    "HEADTOKEN",
                    "alpha",
                    "beta",
                    "gamma",
                    "delta",
                    "epsilon",
                    "zeta",
                    "eta",
                    "theta",
                    "iota",
                    "kappa",
                    "lambda",
                    "mu",
                    "nu",
                    "xi",
                    "omicron",
                    "harness",
                    "rho",
                    "sigma",
                    "tau",
                    "upsilon",
                    "phi",
                    "chi",
                    "psi",
                    "TAILTOKEN",
                ]
                .join(" "),
            },
        ),
    ));

    let debug = render_live_buffer(&app, 38, 11);
    assert!(
        debug.contains("TAILTOKEN"),
        "auto-follow should keep the latest wrapped transcript content visible: {debug}"
    );
}

#[cfg(test)]
#[test]
fn transcript_scrollbar_matches_session_shape() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(
        (0..14)
            .map(|index| {
                transcript_turn_group_test_activity(
                    &format!("request-scrollbar-{index}"),
                    app::ActivityStatus::Done,
                    Some(&format!("question {index}")),
                    &format!(
                        "reply {index} keeps wrapping through the transcript viewport so the scrollbar thumb has real room to move"
                    ),
                )
            })
            .collect::<Vec<_>>(),
    );
    app.selected_activity_index = 13;
    app.follow_mode = false;
    app.transcript_scroll = 18;

    insta::with_settings!({ prepend_module_to_snapshot => false }, {
        insta::assert_snapshot!("harness_tui__live_transcript_scrollbar", render_live_lines(&app, 80, 24));
    });
}

#[cfg(test)]
#[test]
fn transcript_page_down_reaches_response_tail_after_scrolling_up() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(vec![transcript_turn_group_test_activity(
        "request-scroll-recovery",
        app::ActivityStatus::Done,
        None,
        &[
            "HEADTOKEN",
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "zeta",
            "eta",
            "theta",
            "iota",
            "kappa",
            "lambda",
            "mu",
            "nu",
            "xi",
            "omicron",
            "harness",
            "rho",
            "sigma",
            "tau",
            "upsilon",
            "phi",
            "chi",
            "psi",
            "omega",
            "TAILTOKEN",
        ]
        .join(" "),
    )]);
    app.selected_activity_index = 0;
    app.focus = app::Focus::Details;

    let _ = render_live_buffer(&app, 38, 11);
    app.handle_key(key(KeyCode::Home));

    let top = render_live_buffer(&app, 38, 11);
    assert!(
        top.contains("HEADTOKEN"),
        "scroll-to-top should reveal the response head: {top}"
    );
    assert!(
        !top.contains("TAILTOKEN"),
        "top view should not already show the tail: {top}"
    );

    for _ in 0..20 {
        app.handle_key(key(KeyCode::PageDown));
        if app.follow_mode {
            break;
        }
    }

    let bottom = render_live_buffer(&app, 38, 11);
    assert!(
        bottom.contains("TAILTOKEN"),
        "paging back down should make the tail reachable again: {bottom}"
    );
}

#[cfg(test)]
#[test]
fn transcript_without_overflow_hides_scrollbar() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(vec![transcript_turn_group_test_activity(
        "request-no-scrollbar",
        app::ActivityStatus::Done,
        Some("short question"),
        "short reply",
    )]);
    app.selected_activity_index = 0;
    app.follow_mode = true;

    let rendered = render_live_lines(&app, 80, 24);
    assert!(
        !rendered.contains('│'),
        "non-overflow transcripts should not reserve the shell scrollbar track\n{rendered}"
    );
}

#[cfg(test)]
#[test]
fn disconnected_stream_disables_composer_with_reopen_guidance() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_disconnect"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_disconnect".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "disconnect".to_string(),
                request_digest: "digest-disconnect".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_disconnect"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_disconnect".to_string(),
                delta: "transcript stays visible".to_string(),
            },
        ),
    ));
    app.set_status_banner(Some("live event stream disconnected".to_string()));
    app.handle_key(key(crossterm::event::KeyCode::Char('x')));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(app.prompt_buffer.is_empty());
    assert!(debug.contains("transcript stays visible"));
    assert!(debug.contains("Disconnected"));
    assert!(!debug.contains("Composer ·"));
    assert!(!debug.contains("Draft preserved locally"));
    assert!(debug.contains("Reopen the TUI, then continue from the transcript."));
}

#[cfg(test)]
#[test]
fn transcript_renders_inline_tool_states_and_prompt_echo() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "Inspect src/ui.rs".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    app.ingest_event(envelope(
        1,
        Some("req_inline"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_inline".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Inspect src/ui.rs".to_string(),
                request_digest: "digest-inline".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_inline"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_inline".to_string(),
                delta: "Drafting a plan".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_inline".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false"}"#.to_string(),
                args_digest: "digest-inline-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_inline".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_inline".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("exit code: 1".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Inspect src/ui.rs"));
    assert!(debug.contains("exit code: 1") || debug.contains("Drafting a plan"));
    assert!(!debug.contains("args {"));
    assert!(!debug.contains(r#"{"cmd":"false"}"#));
}

#[cfg(test)]
#[test]
fn transcript_tool_rows_keep_status_but_not_raw_json_dump() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_tool_compact"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_tool_compact".to_string(),
                text: "Read the file".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_compact".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-tool-compact".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_compact".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#.to_string(),
                args_digest: "digest-tool-compact-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_compact".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_compact".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("12 lines read".to_string()),
                output_digest: Some("digest-tool-compact-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let transcript = render_live_lines(&app, 120, 36);
    assert!(transcript.contains("Read src/lib.rs [offset=42, limit=20]"));
    assert!(!transcript.contains(r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#));
    assert!(!transcript.contains("args {"));
}

#[cfg(test)]
#[test]
fn transcript_shell_renders_bubbleless_document_flow() {
    transcript_shell_remains_scannable_without_bubble_cards();
}

#[cfg(test)]
#[test]
fn transcript_shell_remains_scannable_without_bubble_cards() {
    let app = rich_transcript_fixture_app();

    let rendered = render_live_lines(&app, 120, 30);
    let lines = rendered.lines().collect::<Vec<_>>();
    let prompt_row =
        find_line_containing(&lines, "Restyle the transcript shell").expect("user prompt row");
    let thinking_row =
        find_line_containing_all_from(&lines, prompt_row + 1, &["Drafting a document-like plan"])
            .expect("reasoning row");
    let tool_row = find_line_containing_all_from(
        &lines,
        thinking_row + 1,
        &["Read src/ui.rs", "[offset=1, limit=24]"],
    )
    .expect("tool row");
    let body_row = find_line_containing_from(
        &lines,
        tool_row + 1,
        "Found the transcript renderer and the composer chrome.",
    )
    .expect("assistant body row");

    assert!(prompt_row < body_row);
    assert!(prompt_row < thinking_row);
    assert!(thinking_row < tool_row);
    assert!(tool_row < body_row);
    assert!(
        first_alphanumeric_column(lines[thinking_row]) == first_alphanumeric_column(lines[body_row]),
        "reasoning should align with the assistant body text while keeping its own muted rail\n{rendered}"
    );
    assert!(
        first_alphanumeric_column(lines[tool_row]) > first_alphanumeric_column(lines[body_row]),
        "tool details should remain nested deeper than the assistant body rail\n{rendered}"
    );
    assert!(!rendered.contains("Composer ·"));
    assert!(!rendered.contains("Ask Harness to inspect, edit, or explain…"));
    assert!(!rendered.contains("Current runtime: default · model-1"));
    assert!(!rendered.contains("provider mock"));
    assert!(!rendered.contains("┌"));
    assert!(!rendered.contains("└"));
    assert!(!rendered.contains("(tool fs.read · succeeded)"));
}

#[cfg(test)]
#[test]
fn transcript_status_metadata_is_inline_not_chrome() {
    let app = rich_transcript_fixture_app();

    let rendered = render_live_lines(&app, 120, 30);

    assert!(!rendered.contains("req_rich_shell"));
    assert!(rendered.contains("Assistant · model-1"));
    assert!(rendered.contains("Read src/ui.rs [offset=1, limit=24]"));
    assert!(!rendered.contains("user ("));
    assert!(!rendered.contains("assistant ("));
    assert!(!rendered.contains("(tool fs.read · succeeded)"));
}

#[cfg(test)]
#[test]
fn transcript_turn_spacing_collapses_without_losing_actor_boundaries() {
    let app = multi_turn_transcript_fixture_app();
    let rendered = render_live_lines(&app, 120, 30);
    let lines = rendered.lines().collect::<Vec<_>>();

    let first_reply_row = find_line_containing(&lines, "The shell is transcript-first and calm.")
        .expect("first assistant body row");
    let second_prompt_row = find_line_containing_from(
        &lines,
        first_reply_row + 1,
        "Tighten the transcript spacing",
    )
    .expect("second prompt row");
    let second_assistant_row = find_line_containing_from(
        &lines,
        second_prompt_row + 1,
        "Spacing is collapsed without losing turn boundaries.",
    )
    .expect("second assistant row");

    assert!(
        second_prompt_row > first_reply_row,
        "second turn should follow the first reply\n{rendered}"
    );
    assert!(
        second_assistant_row > second_prompt_row,
        "assistant reply should stay below the second prompt\n{rendered}"
    );
}

#[cfg(test)]
#[test]
fn nested_transcript_rows_preserve_prefix_on_wrapped_continuations() {
    let mut app = rich_transcript_fixture_app();
    app.activities[0].thinking_text = "Drafting a document-like plan with enough extra detail to force a wrapped continuation so the nested rail stays visible on every continued row.".to_string();

    let rendered = render_live_lines(&app, 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let thinking_row = find_line_containing(&lines, "Drafting a document-like plan")
        .expect("wrapped reasoning row");
    let body_row = find_line_containing(
        &lines,
        "Found the transcript renderer and the composer chrome.",
    )
    .expect("assistant body row");
    let continuation_row = (thinking_row + 1..body_row)
        .find(|row| !lines[*row].trim().is_empty())
        .expect("wrapped continuation row");
    let answer_gap_row = (continuation_row + 1..body_row)
        .find(|row| lines[*row].trim().is_empty())
        .expect("blank gap row before assistant body");

    assert!(
        first_alphanumeric_column(lines[thinking_row])
            == first_alphanumeric_column(lines[body_row]),
        "reasoning should keep the same text column while wrapping under its own rail\n{rendered}"
    );
    assert_eq!(
        first_alphanumeric_column(lines[thinking_row]),
        first_alphanumeric_column(lines[continuation_row]),
        "wrapped nested continuation should repeat the nested prefix and rail\n{rendered}"
    );
    assert!(answer_gap_row < body_row);
}

#[cfg(test)]
#[test]
fn thinking_visibility_toggle_hides_and_restores_inline_thinking_rows() {
    let mut app = rich_transcript_fixture_app();

    let initial = render_live_lines(&app, 120, 30);
    assert!(initial.contains("Drafting a document-like plan"));

    run_palette_command(&mut app, "hide thinking");
    let hidden = render_live_lines(&app, 120, 30);
    assert!(!hidden.contains("Drafting a document-like plan"));
    assert!(hidden.contains("Found the transcript renderer and the composer chrome."));

    run_palette_command(&mut app, "show thinking");
    let restored = render_live_lines(&app, 120, 30);
    assert!(restored.contains("Drafting a document-like plan"));
}

#[cfg(test)]
#[test]
fn tool_details_toggle_collapses_successful_tool_payloads() {
    let mut app = rich_transcript_fixture_app();

    let shown = render_live_lines(&app, 120, 30);
    assert!(shown.contains("Read src/ui.rs [offset=1, limit=24]"));

    run_palette_command(&mut app, "hide tool details");
    let hidden = render_live_lines(&app, 120, 30);
    assert!(!hidden.contains("Read src/ui.rs [offset=1, limit=24]"));

    run_palette_command(&mut app, "show tool details");
    let restored = render_live_lines(&app, 120, 30);
    assert!(restored.contains("Read src/ui.rs [offset=1, limit=24]"));
}

#[cfg(test)]
#[test]
fn failed_tool_rows_still_surface_error_summary() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_tool_error"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_error".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Run the command".to_string(),
                request_digest: "digest-tool-error".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_error".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false","cwd":"/tmp/demo"}"#.to_string(),
                args_digest: "digest-tool-error-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_error".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_error".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: permission denied".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let transcript = render_live_lines(&app, 120, 36);
    assert!(transcript.contains("false"));
    assert!(transcript.contains("exit code: 1 stderr: permission denied"));
    assert!(!transcript.contains(r#"{"cmd":"false","cwd":"/tmp/demo"}"#));
    assert!(!transcript.contains("args {"));
}

#[cfg(test)]
#[test]
fn permission_overlay_preserves_draft_and_transcript_context() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay",
        "tool_call_overlay",
    ));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(!debug.contains("Composer · disabled · Permission blocked"));
    assert!(debug.contains("Permission required"));
    assert!(debug.contains("Draft preserved · keep this draft"));
    assert!(!debug.contains("Select an activity to view transcript"));
    assert!(
        debug.matches("Apply hashline edit to demo.txt").count() >= 1,
        "permission summary should remain visible in the modal"
    );
}

#[cfg(test)]
#[test]
fn permission_overlay_ignores_plain_draft_input_once_prompt_is_active() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "keep this dr".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay_buffered_input",
        "tool_call_overlay_buffered_input",
    ));

    for c in "aft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Char('/')));

    assert_eq!(app.prompt_buffer, "keep this dr");
    assert!(app.active_permission().is_some());

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Draft preserved · keep this dr"));
    assert!(!debug.contains("Slash commands"));
}

#[cfg(test)]
#[test]
fn permission_overlay_preserves_existing_draft_without_buffering_new_letters() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "keep t".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay_home_row_input",
        "tool_call_overlay_home_row_input",
    ));

    for c in "zz".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_buffer, "keep t");
    assert_eq!(
        app.permission_modal_selection("perm_overlay_home_row_input"),
        app::permissions::PermissionModalSelection::AllowOnce
    );

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Draft preserved · keep t"));
}

#[cfg(test)]
fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: permission_id.to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some(tool_call_id.to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    )
}

#[cfg(test)]
fn permission_resolved_event(
    seq: u64,
    permission_id: &str,
    decision: harness_core::perm::PermissionDecision,
) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        Some("tool_call_1"),
        harness_core::event::EventV1::PermissionResolved(
            harness_core::event::PermissionResolvedEvent {
                permission_id: permission_id.to_string(),
                decision: match decision {
                    harness_core::perm::PermissionDecision::Allow => {
                        harness_core::event::PermissionDecision::Allow
                    }
                    harness_core::perm::PermissionDecision::Deny => {
                        harness_core::event::PermissionDecision::Deny
                    }
                },
                reason: Some("resolved in test".to_string()),
            },
        ),
    )
}

#[cfg(test)]
fn startup_session_entry(
    run_id: &str,
    run_dir: &str,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    startup_session_entry_with_details(
        run_id,
        run_dir,
        &format!("run-{run_id}"),
        None,
        None,
        "default",
        "openai/gpt-5.4-mini",
        is_resumable,
        resume_disabled_reason,
    )
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "test helper keeps session-history fixture fields explicit at call sites"
)]
fn startup_session_entry_with_details(
    run_id: &str,
    run_dir: &str,
    run_name: &str,
    status: Option<harness_core::proj::RunStatus>,
    last_updated_at: Option<&str>,
    profile_preset: &str,
    provider_model: &str,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    startup_session_entry_with_mode_and_details(
        run_id,
        run_dir,
        run_name,
        status,
        last_updated_at,
        profile_preset,
        provider_model,
        harness_core::proj::SessionModeSource::InteractiveLive,
        is_resumable,
        resume_disabled_reason,
    )
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "test helper keeps session-history fixture fields explicit at call sites"
)]
fn startup_session_entry_with_mode_and_details(
    run_id: &str,
    run_dir: &str,
    run_name: &str,
    status: Option<harness_core::proj::RunStatus>,
    last_updated_at: Option<&str>,
    profile_preset: &str,
    provider_model: &str,
    mode_source: harness_core::proj::SessionModeSource,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    app::SessionHistoryEntry {
        run_dir: PathBuf::from(run_dir),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: run_id.to_string(),
            run_name: Some(run_name.to_string()),
            status,
            last_updated_at: last_updated_at.map(str::to_string),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some(profile_preset.to_string()),
            provider_model: Some(provider_model.to_string()),
            mode_source,
            is_resumable,
            resume_disabled_reason: resume_disabled_reason.map(str::to_string),
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    }
}

#[cfg(test)]
fn test_timestamp_days_ago(days_ago: i64, time_hh_mm: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before unix epoch");
    let today_days = i64::try_from(now.as_secs() / 86_400).expect("unix day count fits in i64");
    let date = test_civil_date_from_days_since_epoch(today_days - days_ago);
    format!("{date}T{time_hh_mm}:00Z")
}

#[cfg(test)]
fn test_civil_date_from_days_since_epoch(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
fn envelope(
    seq: u64,
    correlation_id: Option<&str>,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        correlation_id,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        payload,
    )
}

#[cfg(test)]
fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: harness_core::event::EventActor,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

#[cfg(test)]
fn orchestration_status_strip_fixture() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_alpha".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_beta".to_string(),
            profile: "reviewer".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_orch_queued"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_queued".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:alpha".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        Some("req_orch_running"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_beta".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_running".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:beta".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        5,
        Some("req_orch_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_stale".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:alpha".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        6,
        Some("req_orch_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_stale".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    app
}

#[cfg(test)]
#[test]
fn session_shell_hides_tab_chrome_and_replay_review_is_command_driven() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }

    let live_backend = TestBackend::new(80, 24);
    let mut live_terminal = Terminal::new(live_backend).expect("create live terminal");
    live_terminal
        .draw(|frame| ui::render_app(frame, &live))
        .expect("draw live frame");

    let live_debug = format!("{:?}", live_terminal.backend().buffer());
    assert!(live_debug.contains("┃ "));
    assert!(!live_debug.contains("Composer ·"));
    assert!(!live_debug.contains("Tabs"));
    assert!(!live_debug.contains("Activity ("));
    assert!(!live_debug.contains("Inspector"));

    live.handle_key(focus_cycle_key());
    live.handle_key(key(crossterm::event::KeyCode::Char('i')));
    assert_eq!(live.review_surface(), None);
    assert!(live.details_drawer_open());
    live.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    live.palette_filtered = vec!["open_event_log".to_string()];
    live.palette_selected = 0;
    live.handle_key(key(crossterm::event::KeyCode::Enter));
    assert_eq!(live.review_surface(), Some(app::ReviewSurface::Events));
    assert!(!live.details_drawer_open());
    live.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(live.review_surface(), None);
    assert!(!live.details_drawer_open());

    let replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    let replay_backend = TestBackend::new(80, 24);
    let mut replay_terminal = Terminal::new(replay_backend).expect("create replay terminal");
    replay_terminal
        .draw(|frame| ui::render_app(frame, &replay))
        .expect("draw replay frame");

    let replay_debug = format!("{:?}", replay_terminal.backend().buffer());
    assert!(!replay_debug.contains("Tabs"));
    assert!(replay_debug.contains("Replay · read-only"));

    let mut replay = replay;
    replay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    replay.palette_filtered = vec!["open_event_log".to_string()];
    replay.palette_selected = 0;
    replay.handle_key(key(crossterm::event::KeyCode::Enter));
    let replay_events_debug = render_live_buffer(&replay, 80, 24);
    assert!(!replay_events_debug.contains("Tabs"));
    assert!(replay_events_debug.contains("Selected event"));
}

#[cfg(test)]
#[test]
fn live_mode_accepts_input_without_focus_switch() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_buffer, "hello");
    assert_eq!(app.prompt_cursor, 5);
}

#[cfg(test)]
#[test]
fn command_palette_renders_and_filters() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    assert_eq!(
        app.palette_filtered,
        vec![
            "new_session".to_string(),
            "resume_session".to_string(),
            "replay_session".to_string(),
            "agent_cycle".to_string(),
            "agent_cycle_reverse".to_string(),
            "cycle_variant".to_string(),
            "toggles".to_string(),
            "open_event_log".to_string(),
            "toggle_terminal_panel".to_string(),
            "toggle_follow".to_string(),
            "hide_thinking".to_string(),
            "show_timestamps".to_string(),
            "hide_tool_details".to_string(),
            "show_generic_tool_output".to_string(),
            "stack_transcript_diffs".to_string(),
            "quit".to_string(),
        ]
    );

    let open_debug = render_live_screen(&app, 120, 36);
    assert!(open_debug.contains("Commands"));
    assert!(open_debug.contains("New session"));

    app.handle_key(key(crossterm::event::KeyCode::Char('n')));

    assert_eq!(app.palette_input, "n");
    assert_eq!(app.palette_cursor, 1);
    assert_eq!(
        app.palette_filtered,
        vec!["new_session".to_string(), "agent_cycle".to_string()]
    );

    let filtered_debug = render_live_screen(&app, 120, 36);
    assert!(filtered_debug.contains("Commands"));
    assert!(filtered_debug.contains("Start a fresh live session"));
    assert!(!filtered_debug.contains("Review diff artifact"));
}

#[cfg(test)]
#[test]
fn command_palette_exposes_model_switcher_when_models_are_configured() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini").with_available_models(
            vec![app::ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )],
        ),
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    assert!(app
        .palette_filtered
        .iter()
        .any(|command| command == "switch_model"));
}

#[cfg(test)]
#[test]
fn command_palette_dims_background_instead_of_repainting_it() {
    let width = 120;
    let height = 36;
    let base = app::AppState::new_startup(Vec::new(), None);
    let base_buffer = render_live_cells(&base, width, height);

    let mut palette = app::AppState::new_startup(Vec::new(), None);
    palette.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert!(palette.palette_visible);

    let overlay =
        FrameLayoutPlan::for_app(&palette, ratatui::layout::Rect::new(0, 0, width, height))
            .palette_overlay
            .expect("palette overlay");
    let palette_buffer = render_live_cells(&palette, width, height);
    let (x, y, base_cell, palette_cell) = base_buffer
        .content
        .iter()
        .enumerate()
        .find_map(|(index, base_cell)| {
            let x = u16::try_from(index % usize::from(width)).ok()?;
            let y = u16::try_from(index / usize::from(width)).ok()?;
            let inside_overlay = x >= overlay.x
                && x < overlay.x.saturating_add(overlay.width)
                && y >= overlay.y
                && y < overlay.y.saturating_add(overlay.height);
            if inside_overlay || base_cell.symbol().trim().is_empty() {
                return None;
            }

            let palette_cell = &palette_buffer[(x, y)];
            (palette_cell.symbol() == base_cell.symbol())
                .then(|| (x, y, base_cell.clone(), palette_cell.clone()))
        })
        .unwrap_or_else(|| panic!("missing visible startup cell outside the palette overlay"));

    assert_eq!(palette_cell.symbol(), base_cell.symbol());
    match (base_cell.fg, palette_cell.fg) {
        (
            ratatui::style::Color::Rgb(base_red, base_green, base_blue),
            ratatui::style::Color::Rgb(palette_red, palette_green, palette_blue),
        ) => {
            assert_eq!(palette_red, overlay_scrim_channel(base_red));
            assert_eq!(palette_green, overlay_scrim_channel(base_green));
            assert_eq!(palette_blue, overlay_scrim_channel(base_blue));
        }
        _ => panic!("startup content at ({x}, {y}) should use rgb foreground colors"),
    }
    match (base_cell.bg, palette_cell.bg) {
        (
            ratatui::style::Color::Rgb(base_red, base_green, base_blue),
            ratatui::style::Color::Rgb(palette_red, palette_green, palette_blue),
        ) => {
            assert_eq!(palette_red, overlay_scrim_channel(base_red));
            assert_eq!(palette_green, overlay_scrim_channel(base_green));
            assert_eq!(palette_blue, overlay_scrim_channel(base_blue));
        }
        _ => panic!("startup content at ({x}, {y}) should use rgb background colors"),
    }
}

#[cfg(test)]
fn overlay_scrim_channel(channel: u8) -> u8 {
    let channel = u16::from(channel);
    u8::try_from(channel.saturating_mul(105) / 255).unwrap_or_default()
}

#[cfg(test)]
#[test]
fn command_palette_empty_state_renders() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('z')));

    assert!(app.palette_visible);
    assert!(app.palette_filtered.is_empty());

    let debug = render_live_screen(&app, 100, 24);
    println!("EMPTY\n{debug}");
    assert!(debug.contains("Commands"));
    assert!(debug.contains("No results found"));
}

#[cfg(test)]
#[test]
fn command_palette_filtered_results_preserve_overlay_command_order() {
    let mut app = app::AppState::new_startup(Vec::new(), None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "re".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }

    assert_eq!(
        app.palette_filtered,
        vec!["resume_session".to_string(), "replay_session".to_string(),]
    );
}

#[cfg(test)]
#[test]
fn command_palette_includes_session_history_entry() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    assert!(app.palette_filtered.starts_with(&[
        "new_session".to_string(),
        "resume_session".to_string(),
        "replay_session".to_string(),
    ]));

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("New session"));
    assert!(rendered.contains("Continue session"));
}

#[cfg(test)]
#[test]
fn session_history_picker_renders_resumable_and_replay_rows() {
    let entries = vec![
        startup_session_entry_with_details(
            "run_resume",
            "/tmp/sessions/run_resume",
            "New session - 2026-03-08T12:34:56.000Z",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        ),
        startup_session_entry_with_mode_and_details(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            "beta-prompt",
            Some(harness_core::proj::RunStatus::Failed),
            Some("2026-03-07T03:21:00Z"),
            "ops",
            "anthropic/claude-3.7",
            harness_core::proj::SessionModeSource::Prompt,
            false,
            Some("prompt runs are not resumable"),
        ),
    ];
    let mut app = app::AppState::new_startup(entries, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    let resume_render = render_live_lines(&app, 120, 30);
    assert!(resume_render.contains("Continue session"));
    assert!(resume_render.contains("Search"));
    assert!(resume_render.contains("New session"));
    assert!(!resume_render.contains("New session - 2026-03-08T12:34:56.000Z"));
    assert!(!resume_render.contains("beta-prompt"));
    assert!(resume_render.contains("continue ready"));

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    let replay_render = render_live_lines(&app, 120, 30);
    assert!(replay_render.contains("Replay session"));
    assert!(replay_render.contains("beta-prompt"));
    assert!(replay_render.contains("delete"));
    assert!(replay_render.contains("rename"));
}

#[cfg(test)]
#[test]
fn session_history_filter_uses_case_insensitive_substrings() {
    fn open_continue_picker() -> app::AppState {
        let mut app = app::AppState::new_startup(
            vec![
                startup_session_entry_with_mode_and_details(
                    "RUN-ABC123",
                    "/tmp/sessions/RUN-ABC123",
                    "Alpha Runner",
                    Some(harness_core::proj::RunStatus::Finished),
                    Some("2026-03-08T12:34:56Z"),
                    "DeepOps",
                    "OpenAI/GPT-5.4-Mini",
                    harness_core::proj::SessionModeSource::InteractiveLive,
                    false,
                    Some("run is still active"),
                ),
                startup_session_entry_with_details(
                    "run_other",
                    "/tmp/sessions/run_other",
                    "beta-run",
                    Some(harness_core::proj::RunStatus::Running),
                    Some("2026-03-08T08:00:00Z"),
                    "ops",
                    "anthropic/claude-3.7",
                    true,
                    None,
                ),
            ],
            None,
        );

        app.handle_key(key_with_modifiers(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        for ch in "resume".chars() {
            app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
        }
        app.handle_key(key(crossterm::event::KeyCode::Enter));
        app
    }

    let mut by_run_name = open_continue_picker();
    for ch in "runner".chars() {
        by_run_name.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_run_name.session_history_filtered, vec![0]);

    let mut by_case_insensitive_title = open_continue_picker();
    for ch in "ALPHA".chars() {
        by_case_insensitive_title.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_case_insensitive_title.session_history_filtered, vec![0]);

    let mut by_non_title_metadata = open_continue_picker();
    for ch in "gpt-5".chars() {
        by_non_title_metadata.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(by_non_title_metadata.session_history_filtered.is_empty());

    let mut no_match = open_continue_picker();
    for ch in "missing".chars() {
        no_match.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    no_match.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(no_match.session_history_filtered.is_empty());
    let rendered = render_live_lines(&no_match, 120, 30);
    assert!(rendered.contains("No results found"));
}

#[cfg(test)]
#[test]
fn continue_picker_filters_to_interactive_sessions() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry_with_mode_and_details(
                "run_blocked",
                "/tmp/sessions/run_blocked",
                "blocked-interactive",
                Some(harness_core::proj::RunStatus::Running),
                Some("2026-03-08T09:00:00Z"),
                "ops",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::InteractiveLive,
                false,
                Some("run is still active"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_prompt",
                "/tmp/sessions/run_prompt",
                "prompt-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T08:00:00Z"),
                "ops",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::Prompt,
                false,
                Some("prompt runs are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_ready_live",
                "/tmp/sessions/run_ready_live",
                "ready-live",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T07:00:00Z"),
                "deep",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::InteractiveLive,
                true,
                None,
            ),
            startup_session_entry_with_mode_and_details(
                "run_scenario",
                "/tmp/sessions/run_scenario",
                "scenario-fixture",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T06:00:00Z"),
                "default",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::ScenarioFixture,
                false,
                Some("scenario fixture runs are excluded from resume"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_replay_only",
                "/tmp/sessions/run_replay_only",
                "replay-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T05:00:00Z"),
                "default",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::ReplayOnly,
                false,
                Some("replay-only launches are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_ready_mock",
                "/tmp/sessions/run_ready_mock",
                "ready-mock",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T04:00:00Z"),
                "mock",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::InteractiveMock,
                true,
                None,
            ),
        ],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_ready_live", "run_ready_mock", "run_blocked"]
    );
    assert_eq!(
        app.session_history_entries[*app
            .session_history_filtered
            .last()
            .expect("blocked interactive entry present")]
        .catalog
        .resume_disabled_reason
        .as_deref(),
        Some("run is still active")
    );
    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("run is still active"));
    assert!(!rendered.contains("prompt-only"));
    assert!(!rendered.contains("scenario-fixture"));
    assert!(!rendered.contains("replay-only"));
}

#[cfg(test)]
#[test]
fn replay_picker_keeps_prompt_runs_visible() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry_with_mode_and_details(
                "run_ready_live",
                "/tmp/sessions/run_ready_live",
                "ready-live",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T07:00:00Z"),
                "deep",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::InteractiveLive,
                true,
                None,
            ),
            startup_session_entry_with_mode_and_details(
                "run_prompt",
                "/tmp/sessions/run_prompt",
                "prompt-only",
                Some(harness_core::proj::RunStatus::Failed),
                Some("2026-03-08T06:00:00Z"),
                "ops",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::Prompt,
                false,
                Some("prompt runs are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_scenario",
                "/tmp/sessions/run_scenario",
                "scenario-fixture",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T05:00:00Z"),
                "default",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::ScenarioFixture,
                false,
                Some("scenario fixture runs are excluded from resume"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_replay_only",
                "/tmp/sessions/run_replay_only",
                "replay-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T04:00:00Z"),
                "default",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::ReplayOnly,
                false,
                Some("replay-only launches are not resumable"),
            ),
        ],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_ready_live", "run_prompt"]
    );
    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Replay session"));
    assert!(rendered.contains("prompt-only"));
    assert!(rendered.contains("replay ready"));
    assert!(!rendered.contains("scenario-fixture"));
    assert!(!rendered.contains("replay-only"));
}

#[cfg(test)]
#[test]
fn focus_returns_after_session_history_close() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.prompt_buffer = "keep prompt draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.set_session_history_entries(vec![startup_session_entry_with_details(
        "run_replay",
        "/tmp/sessions/run_replay",
        "replayable-run",
        Some(harness_core::proj::RunStatus::Finished),
        Some("2026-03-08T12:34:56Z"),
        "deep",
        "openai/gpt-5.4-mini",
        true,
        None,
    )]);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.session_history_visible);
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "keep prompt draft");
    assert_eq!(app.prompt_cursor, "keep prompt draft".chars().count());
}

#[cfg(test)]
#[test]
fn command_palette_enter_executes_selected_command() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_review_surface = Some(app::ReviewSurface::Help);
    app.focus = app::Focus::Details;
    app.prompt_buffer = "preserve me".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "run".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.prompt_buffer, "preserve me");
    assert_eq!(app.prompt_cursor, "preserve me".chars().count());
}

#[cfg(test)]
#[test]
fn palette_escape_preserves_prompt_draft() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "keep this prompt".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let prompt_before = app.prompt_buffer.clone();
    let cursor_before = app.prompt_cursor;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));

    assert!(app.palette_visible);
    assert_eq!(app.palette_input, "d");

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.palette_visible);
    assert!(app.palette_input.is_empty());
    assert_eq!(app.palette_cursor, 0);
    assert!(app.palette_filtered.is_empty());
    assert_eq!(app.palette_selected, 0);
    assert_eq!(app.prompt_buffer, prompt_before);
    assert_eq!(app.prompt_cursor, cursor_before);
    assert!(app.prompt_history.is_empty());
    assert_eq!(app.prompt_history_index, None);
}

#[cfg(test)]
#[test]
fn permission_modal_preempts_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    for c in "blocked by permission".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_block_submit",
        "tool_call_block_submit",
    ));

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_block_submit".to_string(),
            decision: harness_core::perm::PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }]
    );
    drop(intents);

    assert_eq!(app.prompt_buffer, "blocked by permission");
    assert_eq!(app.prompt_cursor, "blocked by permission".chars().count());
    assert!(app.prompt_history.is_empty());
    assert!(app.activities.is_empty());
    assert!(app.active_permission().is_some());
}

#[cfg(test)]
#[test]
fn startup_surface_renders_primary_actions() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 100, 24);
    assert_eq!(app.focus, app::Focus::List);
    assert!(rendered.contains("╻ ╻  ┏━┓  ┏━┓  ┏┓╻"));
    assert!(!rendered.contains("Launch: worker · model-1"));
    assert!(!rendered.contains("Provider mock"));
    assert!(rendered.contains("Worker model-1 mock"));
    assert!(rendered.contains("ctrl+p commands"));
    assert!(!rendered.contains("Enter select"));
    assert!(rendered.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(!rendered.contains("● Tip"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions:"));
}

#[cfg(test)]
#[test]
fn startup_typing_moves_to_quick_start_prompt() {
    let mut app = app::AppState::new_startup(Vec::new(), None);

    assert_eq!(app.focus, app::Focus::List);
    assert!(app.prompt_buffer.is_empty());

    app.handle_key(key(crossterm::event::KeyCode::Char('x')));

    assert_eq!(app.focus, app::Focus::Prompt);
    assert_eq!(app.prompt_buffer, "x");
    assert_eq!(app.prompt_cursor, 1);

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Composer"));
    assert!(rendered.contains("x"));
    assert!(rendered.contains("ctrl+p commands"));
    assert!(!rendered.contains("● Tip"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions:"));
}

#[cfg(test)]
#[test]
fn startup_palette_remains_secondary_and_draft_safe() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );

    for ch in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions:"));
    assert_eq!(app.prompt_buffer, "keep this draft");
    assert_eq!(app.focus, app::Focus::Prompt);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    let overlay_render = render_live_lines(&app, 100, 24);
    assert!(overlay_render.contains("Commands"));
    assert!(overlay_render.contains("New session"));
    assert!(overlay_render.contains("Continue session"));

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.palette_visible);
    assert_eq!(app.prompt_buffer, "keep this draft");
    assert_eq!(app.prompt_cursor, "keep this draft".chars().count());
    assert_eq!(app.focus, app::Focus::Prompt);
}

#[cfg(test)]
#[test]
fn post_run_handoff_renders_next_actions() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.active_review_surface = Some(app::ReviewSurface::Events);
    app.focus = app::Focus::Prompt;
    app.prompt_buffer = "keep this draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert_eq!(app.focus, app::Focus::Details);
    assert!(!app.post_run_handoff_visible());
    assert!(app.completed_session_shell_active());

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("keep this draft"));
    assert!(!rendered.contains("Composer"));
    assert!(rendered.contains("keep this draft"));
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
}

#[cfg(test)]
#[test]
fn post_run_failure_handoff_renders_recovery_actions() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "tool execution failed".to_string(),
        }),
    ));

    assert_eq!(app.focus, app::Focus::Details);
    assert!(!app.post_run_handoff_visible());
    assert!(app.completed_session_shell_active());

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("Tab focus") || rendered.contains("q quit"));
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
}

#[cfg(test)]
#[test]
fn post_run_handoff_disables_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.prompt_buffer = "blocked prompt".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
    assert!(rendered.contains("blocked prompt"));
    assert!(!rendered.contains("Composer"));

    app.focus = app::Focus::Prompt;
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    assert!(app.prompt_buffer.is_empty());

    app.focus = app::Focus::List;
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        &*intents,
        &[UiIntent::SubmitPrompt {
            text: "blocked prompt".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: app::LaunchMetadata::default(),
        }]
    );
    assert!(!app.should_quit);
}

#[cfg(test)]
#[test]
fn double_escape_interrupts_active_live_turn_after_harness_confirmation() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.focus = app::Focus::Details;

    app.ingest_event(envelope_with_actor(
        1,
        Some("req_active"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_active".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_sibling"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_sibling".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_sibling".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-2".to_string()),
        }),
    ));

    assert!(app.interrupt_hint_visible());
    assert!(render_live_lines(&app, 100, 24).contains("esc interrupt"));

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(app.interrupt_confirmation_pending());
    assert!(intents.lock().expect("lock intents").is_empty());
    assert!(render_live_lines(&app, 100, 24).contains("esc again to interrupt"));

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.interrupt_confirmation_pending());
    assert_eq!(
        &*intents.lock().expect("lock intents"),
        &[UiIntent::InterruptSession {
            task_ids: vec!["task_active".to_string(), "task_sibling".to_string()],
        }]
    );
}

#[cfg(test)]
#[test]
fn interrupt_confirmation_is_scoped_to_current_active_turn_set() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.focus = app::Focus::Details;

    app.ingest_event(envelope_with_actor(
        1,
        Some("req_old"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_old".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
        }),
    ));

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(app.interrupt_confirmation_pending());

    app.ingest_event(envelope(
        2,
        Some("req_old"),
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_old".to_string(),
            reason: "cancelled externally".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_new"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_new".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
        }),
    ));

    assert!(!app.interrupt_confirmation_pending());
    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(app.interrupt_confirmation_pending());
    assert!(intents.lock().expect("lock intents").is_empty());

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert_eq!(
        &*intents.lock().expect("lock intents"),
        &[UiIntent::InterruptSession {
            task_ids: vec!["task_new".to_string()],
        }]
    );
}

#[cfg(test)]
#[test]
fn continued_quiescent_bootstrap_shows_handoff_before_reopening_live_conversation() {
    app::set_pending_live_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "default:model-1")
            .with_mode_label("Continued"),
    );
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume_quiescent")),
        false,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        Some("req_resume_terminal"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let handoff_render = render_live_lines(&app, 100, 24);
    assert!(!handoff_render.contains("Next action"));
    assert!(!handoff_render.contains("Continue this session"));
    assert!(!handoff_render.contains("Ask Harness to inspect, edit, or explain…"));
    assert!(!handoff_render.contains("Composer"));
    assert!(!app.composer_disabled());
}

#[cfg(test)]
#[test]
fn lifecycle_shell_state_transitions() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.prompt_buffer = "draft prompt".to_string();
    startup.prompt_cursor = startup.prompt_buffer.chars().count();

    assert_eq!(
        startup.lifecycle_shell_state(),
        app::LifecycleShellState::Startup
    );
    assert!(startup.startup_shell_visible());
    assert!(!startup.post_run_handoff_visible());
    assert!(!startup.composer_disabled());

    let mut post_run = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    post_run.ingest_event(envelope(
        1,
        Some("req_state_transition"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(
        post_run.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!post_run.startup_shell_visible());
    assert!(!post_run.post_run_handoff_visible());
    assert!(post_run.completed_session_shell_active());

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut missing_session_path = app::AppState::new_live(None, false, Some(fallback_sink));
    missing_session_path.ingest_event(envelope(
        1,
        Some("req_state_transition_missing_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(
        missing_session_path.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!missing_session_path.post_run_handoff_visible());
    assert!(missing_session_path.completed_session_shell_active());

    let replay = app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            Some("req_replay_state_transition"),
            harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(
        replay.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(replay.composer_disabled());
}

#[cfg(test)]
#[test]
fn lifecycle_shell_snapshots() {
    let mut startup = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    startup.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let startup_render = render_live_lines(&startup, 100, 24);
    assert!(startup_render.contains("╻ ╻  ┏━┓  ┏━┓  ┏┓╻") || startup_render.contains("Harness"));
    assert!(startup_render.contains("ctrl+p commands"));
    assert!(!startup_render.contains("Enter select"));
    assert!(startup_render.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(startup_render.contains("commands"));
    assert!(
        !startup_render.contains("Dispatch a new run, reopen live work, or inspect saved history.")
    );
    assert!(!startup_render.contains("Actions:"));

    let entries = vec![
        startup_session_entry_with_details(
            "run_resume",
            "/tmp/sessions/run_resume",
            "alpha-run",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        ),
        startup_session_entry_with_mode_and_details(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            "beta-prompt",
            Some(harness_core::proj::RunStatus::Failed),
            Some("2026-03-07T03:21:00Z"),
            "ops",
            "anthropic/claude-3.7",
            harness_core::proj::SessionModeSource::Prompt,
            false,
            Some("prompt runs are not resumable"),
        ),
        startup_session_entry_with_mode_and_details(
            "run_blocked",
            "/tmp/sessions/run_blocked",
            "blocked-interactive",
            Some(harness_core::proj::RunStatus::Running),
            Some("2026-03-06T09:15:00Z"),
            "ops",
            "openai/gpt-5.4-mini",
            harness_core::proj::SessionModeSource::InteractiveLive,
            false,
            Some("run is still active"),
        ),
    ];
    let mut picker = app::AppState::new_startup(entries, None);
    for ch in "keep this draft".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(picker.prompt_buffer, "keep this draft");
    assert_eq!(picker.focus, app::Focus::Prompt);

    picker.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    picker.handle_key(key(crossterm::event::KeyCode::Enter));

    let continue_render = render_live_lines(&picker, 120, 30);
    assert!(picker.session_history_visible);
    assert_eq!(picker.prompt_buffer, "keep this draft");
    assert!(continue_render.contains("Continue session"));
    assert!(continue_render.contains("continue ready"));
    assert!(continue_render.contains("run is still active"));
    assert!(!continue_render.contains("beta-prompt"));
    assert!(continue_render.contains("Harness") || continue_render.contains("Continue session"));

    picker.handle_key(key(crossterm::event::KeyCode::Esc));
    picker.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    picker.handle_key(key(crossterm::event::KeyCode::Enter));

    let replay_render = render_live_lines(&picker, 120, 30);
    assert!(picker.session_history_visible);
    assert_eq!(picker.prompt_buffer, "keep this draft");
    assert!(replay_render.contains("Replay session"));
    assert!(replay_render.contains("beta-prompt"));
    assert!(replay_render.contains("replay ready"));
    assert!(replay_render.contains("Harness") || replay_render.contains("Replay session"));

    let mut completed_shell = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed_shell.active_review_surface = Some(app::ReviewSurface::Events);
    completed_shell.focus = app::Focus::Prompt;
    completed_shell.prompt_buffer = "keep this draft".to_string();
    completed_shell.prompt_cursor = completed_shell.prompt_buffer.chars().count();
    completed_shell.ingest_event(envelope(
        1,
        Some("req_post_run"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let completed_shell_render = render_live_lines(&completed_shell, 100, 24);
    assert!(completed_shell_render.contains("keep this draft"));
    assert!(!completed_shell_render.contains("Composer"));
    assert!(!completed_shell_render.contains("Next action"));
    insta::with_settings!({ prepend_module_to_snapshot => false }, {
        insta::assert_snapshot!(
            "harness_tui__completed_shell_lifecycle",
            completed_shell_render
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        );
    });

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut fallback = app::AppState::new_live(None, false, Some(fallback_sink));
    fallback.ingest_event(envelope(
        1,
        Some("req_post_run_missing_session_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let fallback_render = render_live_lines(&fallback, 100, 24);
    assert!(!fallback_render.contains("Run complete"));
    assert!(fallback_render.contains("q quit"));
    assert!(!fallback_render.contains("Composer"));
    assert!(!fallback_render.contains("Next action"));
    insta::with_settings!({ prepend_module_to_snapshot => false }, {
        insta::assert_snapshot!(
            "harness_tui__completed_shell_fallback_lifecycle",
            fallback_render
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        );
    });
}

#[cfg(test)]
#[test]
fn session_history_browse_preserves_draft() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry("run_a", "/tmp/sessions/run_a", true, None),
            startup_session_entry("run_b", "/tmp/sessions/run_b", true, None),
        ],
        None,
    );
    for c in "startup draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    let before = app.prompt_buffer.clone();
    let cursor_before = app.prompt_cursor;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(app.prompt_buffer, before);
    assert_eq!(app.prompt_cursor, cursor_before);

    app.handle_key(key(crossterm::event::KeyCode::Down));
    assert_eq!(app.session_history_selected, 1);

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.session_history_visible);
    assert_eq!(app.prompt_buffer, before);
    assert_eq!(app.prompt_cursor, cursor_before);
}

#[cfg(test)]
#[test]
fn new_session_resets_transcript_but_keeps_unsent_draft() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_a",
            "/tmp/sessions/run_a",
            true,
            None,
        )],
        None,
    );
    app.ingest_event(envelope(
        1,
        Some("req_before_reset"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_before_reset".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "before reset".to_string(),
                request_digest: "digest-before-reset".to_string(),
                metadata: None,
            },
        ),
    ));
    app.prompt_history.push("older sent prompt".to_string());
    app.prompt_buffer = "unsent startup draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.events.is_empty());
    assert!(app.activities.is_empty());
    assert!(app.prompt_history.is_empty());
    assert_eq!(app.prompt_buffer, "unsent startup draft");
    assert_eq!(app.prompt_cursor, "unsent startup draft".chars().count());
}

#[cfg(test)]
#[test]
fn continue_disabled_session_shows_reason_banner() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            false,
            Some("prompt runs are not resumable"),
        )],
        Some(intent_sink),
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert!(intents.is_empty());
    drop(intents);
    assert!(app.session_history_visible);
    assert_eq!(
        app.continue_disabled_banner.as_deref(),
        Some("continue unavailable: prompt runs are not resumable")
    );
    assert!(app
        .runtime_state()
        .summary
        .contains("continue unavailable: prompt runs are not resumable"));
}

#[cfg(test)]
#[test]
fn replay_session_intent_never_enables_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_replay",
            "/tmp/sessions/run_replay",
            true,
            None,
        )],
        Some(intent_sink),
    );
    app.prompt_buffer = "do not submit".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "replay".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ReplaySession {
            run_id: "run_replay".to_string(),
            run_dir: PathBuf::from("/tmp/sessions/run_replay"),
        }]
    );
    drop(intents);
    assert_eq!(app.prompt_buffer, "do not submit");
    assert!(app.prompt_history.is_empty());
}

#[cfg(test)]
#[test]
fn overlay_wheel_routing_preserved() {
    let frame_area = ratatui::layout::Rect::new(0, 0, 140, 40);
    let mut palette_overlay = app::AppState::new_live(None, false, None);
    palette_overlay.details_scroll = 6;
    palette_overlay.transcript_scroll = 4;
    palette_overlay.follow_mode = false;
    palette_overlay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    palette_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Transcript),
        None,
        None,
    );
    palette_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 70,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Inspector),
        None,
        None,
    );

    assert!(palette_overlay.palette_visible);
    assert_eq!(palette_overlay.details_scroll, 6);
    assert_eq!(palette_overlay.transcript_scroll, 4);
    assert!(!palette_overlay.follow_mode);

    let mut permission_overlay = app::AppState::new_live(None, false, None);
    permission_overlay.details_scroll = 8;
    permission_overlay.transcript_scroll = 3;
    permission_overlay.follow_mode = false;
    permission_overlay.ingest_event(permission_requested_event(
        1,
        "perm_overlay_wheel",
        "tool_call_overlay_wheel",
    ));

    permission_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Transcript),
        None,
        None,
    );
    permission_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 70,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Inspector),
        None,
        None,
    );

    assert!(permission_overlay.active_permission().is_some());
    assert_eq!(permission_overlay.details_scroll, 8);
    assert_eq!(permission_overlay.transcript_scroll, 3);
    assert!(!permission_overlay.follow_mode);
}

#[cfg(test)]
#[test]
fn replay_secondary_surfaces_remain_reachable_after_live_shell_refactor() {
    let mut replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );

    replay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    replay.palette_filtered = vec!["open_event_log".to_string()];
    replay.palette_selected = 0;
    replay.handle_key(key(crossterm::event::KeyCode::Enter));
    assert_eq!(replay.review_surface(), Some(app::ReviewSurface::Events));

    replay.handle_key(key(crossterm::event::KeyCode::Char('?')));
    assert_eq!(replay.review_surface(), Some(app::ReviewSurface::Help));
    let replay_help_debug = render_live_buffer(&replay, 80, 24);
    assert!(!replay_help_debug.contains("Tabs"));
    assert!(replay_help_debug.contains("Replay · read-only"));
    assert!(replay_help_debug.contains("read-only"));
    assert!(replay_help_debug.contains("Keyboard Shortcuts:"));

    replay.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(replay.review_surface(), None);
}

#[cfg(test)]
#[test]
fn composer_enter_submits_and_shift_enter_inserts_newline() {
    use std::sync::Mutex;

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for c in "world".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "hello\nworld".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: app::LaunchMetadata::default(),
        }]
    );
    drop(intents);

    assert!(app.prompt_buffer.is_empty());
    assert_eq!(
        app.prompt_history.last().map(String::as_str),
        Some("hello\nworld")
    );

    let activity = app.activities.back().expect("submitted activity");
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("hello\nworld")
    );
    assert_eq!(activity.status, app::ActivityStatus::Streaming);
}

#[cfg(test)]
#[test]
fn composer_ctrl_j_inserts_newline() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('j'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for c in "world".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_buffer, "hello\nworld");
}

#[cfg(test)]
#[test]
fn composer_submits_queued_followup_while_streaming() {
    use std::sync::Mutex;

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));

    for c in "first".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    for c in "next".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "first".to_string(),
                request_digest: "digest-1".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_001"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_001".to_string(),
                delta: "streaming".to_string(),
            },
        ),
    ));

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[
            UiIntent::SubmitPrompt {
                text: "first".to_string(),
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                launch_metadata: app::LaunchMetadata::default(),
            },
            UiIntent::SubmitPrompt {
                text: "next".to_string(),
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                launch_metadata: app::LaunchMetadata::default(),
            },
        ]
    );
    drop(intents);

    assert!(app.prompt_buffer.is_empty());
    assert_eq!(app.prompt_cursor, 0);
    assert_eq!(app.prompt_history.last().map(String::as_str), Some("next"));
    let activity = app.activities.front().expect("streaming activity");
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.transcript_text, "streaming");
    assert_eq!(activity.status, app::ActivityStatus::Streaming);
    let queued_activity = app.activities.back().expect("submitted activity");
    assert_eq!(
        queued_activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("next")
    );
}

#[cfg(test)]
#[test]
fn session_shell_registry_only_exposes_home_and_session_shells() {
    let live_registry = app::default_shell_registry(false);
    assert_eq!(live_registry.len(), 2);
    assert_eq!(live_registry[0].label, "Home");
    assert_eq!(live_registry[1].label, "Session");

    let replay_registry = app::default_shell_registry(true);
    assert_eq!(replay_registry.len(), 2);
    assert_eq!(replay_registry[0].label, "Home");
    assert_eq!(replay_registry[1].label, "Replay");

    let replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    assert!(!replay.details_drawer_open());
    assert_eq!(replay.review_surface(), None);
}

#[cfg(test)]
#[test]
fn replay_mode_does_not_render_orchestration_summary() {
    let mut events = session_view_events();
    events.extend([
        envelope(
            100,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_replay".to_string(),
                profile: "researcher".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            101,
            Some("req_replay_orch"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("agent_replay".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_replay_orch".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("agent:queued:replay".to_string()),
            }),
        ),
        envelope_with_actor(
            102,
            Some("req_replay_orch"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("agent_replay".to_string()),
            ),
            harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
                task_id: "task_replay_orch".to_string(),
                stale_for_ms: 3001,
            }),
        ),
    ]);

    let mut replay =
        app::AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), events);

    let replay_run = render_live_lines(&replay, 120, 30);
    assert!(!replay_run.contains("Orchestration"));
    assert!(!replay_run.contains("agents "));
    assert!(!replay.details_drawer_open());

    replay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    replay.palette_filtered = vec!["open_event_log".to_string()];
    replay.palette_selected = 0;
    replay.handle_key(key(crossterm::event::KeyCode::Enter));
    let replay_events = render_live_lines(&replay, 120, 30);
    assert!(!replay_events.contains("Orchestration"));
    assert!(!replay_events.contains("agents "));

    replay.handle_key(key(crossterm::event::KeyCode::Char('?')));
    let replay_help = render_live_lines(&replay, 120, 30);
    assert!(!replay_help.contains("Orchestration"));
    assert!(!replay_help.contains("agents "));
}

#[cfg(test)]
#[test]
fn startup_shell_shows_profile_provider_and_model_chrome() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("╻ ╻  ┏━┓  ┏━┓  ┏┓╻"));
    assert!(!rendered.contains("Launch: deep · gpt-5.4"));
    assert!(!rendered.contains("Provider proxy"));
    assert!(rendered.contains("Deep gpt-5.4 proxy · Demo"));
    assert!(rendered.contains("ctrl+p commands"));
    assert!(!rendered.contains("Enter select"));
    assert!(rendered.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(rendered.contains("commands"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions:"));
}

#[cfg(test)]
#[test]
fn lifecycle_shell_narrow_layout_renders_primary_cta() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let title_row = find_line_containing(&lines, "╻ ╻  ┏━┓  ┏━┓  ┏┓╻").expect("startup logo row");
    let prompt_row = find_line_containing(
        &lines,
        "Ask anything... \"What is the tech stack of this project?\"",
    )
    .expect("startup prompt row");
    let footer_row = find_line_containing(&lines, "commands").expect("footer row");

    assert!(!rendered.contains("Actions:"));
    assert!(!rendered.contains("Dispatch a new run"));
    assert!(!rendered.contains("Launch: worker · model-1"));
    assert!(rendered.contains("commands"));
    assert!(title_row < prompt_row);
    assert!(prompt_row < footer_row);
}

#[cfg(test)]
#[test]
fn startup_card_uses_lifecycle_geometry_contract() {
    let theme = Theme::default();
    let minimum_area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let primary_area = ratatui::layout::Rect::new(0, 0, 100, 30);

    let minimum_layout = theme.lifecycle_surface_layout(minimum_area.width, minimum_area.height);
    let primary_layout = theme.lifecycle_surface_layout(primary_area.width, primary_area.height);

    let minimum_startup = layout::startup_shell_area(minimum_area, &theme);
    let primary_startup = layout::startup_shell_area(primary_area, &theme);

    assert_eq!(
        minimum_startup,
        layout::lifecycle_card_area(minimum_area, &theme, minimum_layout.startup_card)
    );
    assert_eq!(
        primary_startup,
        layout::lifecycle_card_area(primary_area, &theme, primary_layout.startup_card)
    );
    assert_eq!(minimum_startup, ratatui::layout::Rect::new(5, 7, 70, 12));
    assert_eq!(primary_startup, ratatui::layout::Rect::new(9, 10, 82, 12));
    assert_ne!(
        minimum_startup,
        layout::live_empty_state_area(minimum_area, &theme)
    );
    assert_ne!(
        primary_startup,
        layout::live_empty_state_area(primary_area, &theme)
    );
}

#[cfg(test)]
#[test]
fn startup_card_moves_closer_to_dock_in_tall_and_split_windows() {
    let theme = Theme::default();
    let tall_minimum_area = ratatui::layout::Rect::new(0, 0, 80, 48);
    let split_area = ratatui::layout::Rect::new(0, 0, 96, 40);

    let tall_minimum = layout::startup_shell_area(tall_minimum_area, &theme);
    let split = layout::startup_shell_area(split_area, &theme);

    assert_eq!(tall_minimum, ratatui::layout::Rect::new(5, 19, 70, 12));
    assert_eq!(split, ratatui::layout::Rect::new(0, 14, 96, 13));
}

#[cfg(test)]
#[test]
fn live_empty_state_uses_shared_startup_copy_without_mode_badges() {
    let mut demo = app::AppState::new_live(None, false, None);
    demo.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let demo_rendered = render_live_lines(&demo, 100, 24);
    assert!(demo_rendered.contains("Harness"));
    assert!(demo_rendered.contains("Launch: worker · model-1"));
    assert!(demo_rendered.contains("Start a conversation to begin"));
    assert!(!demo_rendered.contains("Demo mode · mock provider"));
    assert!(!demo_rendered.contains("Launch: worker · model-1 · Demo"));

    let mut mock = app::AppState::new_live(None, false, None);
    mock.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
    );

    let mock_rendered = render_live_lines(&mock, 100, 24);
    assert!(mock_rendered.contains("Harness"));
    assert!(mock_rendered.contains("Launch: worker · model-1"));
    assert!(mock_rendered.contains("Start a conversation to begin"));
    assert!(!mock_rendered.contains("Mock mode · mock provider"));
    assert!(!mock_rendered.contains("Launch: worker · model-1 · Mock"));
}

#[cfg(test)]
#[test]
fn live_shell_minimum_geometry_snapshot_renders_without_overlap() {
    assert_live_shell_geometry(80, 24);
}

#[cfg(test)]
#[test]
fn live_shell_primary_geometry_snapshot_renders_without_overlap() {
    assert_live_shell_geometry(100, 30);
}

#[cfg(test)]
#[test]
fn live_empty_state_snapshot_renders_input_first_shell() {
    let app = app::AppState::new_live(None, false, None);
    let rendered = render_live_lines(&app, 80, 24);

    assert_live_shell_frame_invariants(&rendered, 80, 24);
    assert!(rendered.contains("Session"));
    assert!(!rendered.contains('┌'));
    assert!(rendered.contains("Harness"));
    assert!(rendered.contains("Launch: default · -"));
    assert!(rendered.contains("Start a conversation to begin"));
    assert!(rendered.contains("0  Ctrl+p commands"));
    assert!(rendered.contains("Ctrl+p commands"));
    assert!(!rendered.contains("Enter send · Shift+Enter/Ctrl+j newline · ↑/↓ history"));
    assert!(!rendered.contains("Type to start a new session."));

    assert_live_shell_document_composer_contract(&app, 80, 24, None, None, "Ctrl+p commands");
    assert!(!rendered.contains("Ask Harness to inspect, edit, or explain…"));
}

#[cfg(test)]
#[test]
fn live_empty_state_disappears_after_first_activity() {
    let theme = Theme::default();
    let mut app = app::AppState::new_live(None, false, None);

    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains(theme.live_shell.empty_state.value_prop));
    assert!(!rendered.contains(theme.live_shell.empty_state.example_prompts[0].prompt));
    assert!(rendered.contains("ship it"));
    assert!(!rendered.contains("pending turn"));
}

#[cfg(test)]
#[test]
fn live_shell_orchestration_status_strip_snapshot() {
    let app = orchestration_status_strip_fixture();
    let status_row = live_status_strip_row(&app, 160, 30, "Ctrl+p commands");

    insta::assert_snapshot!(
        status_row,
        @"live 0  Ctrl+p commands"
    );
}

#[cfg(test)]
#[test]
fn live_status_strip_orchestration_summary_truncates_warning_last() {
    let app = orchestration_status_strip_fixture();

    let wide = render_live_lines(&app, 160, 30);
    let counts_only = render_live_lines(&app, 77, 24);
    assert!(wide.contains("orch 2a 1q 1r 1s") || !wide.contains("warn stale for 3001 ms"));
    assert!(!counts_only.contains("warn stale for 3001 ms"));
}

#[cfg(test)]
#[test]
fn live_status_strip_renders_zero_state_orchestration_counts() {
    let app = app::AppState::new_live(None, false, None);

    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains("orch 0a 0q 0r 0s"));
    assert!(!rendered.contains("warn"));
}

#[cfg(test)]
#[test]
fn live_empty_state_respects_compact_geometry() {
    let theme = Theme::default();
    let app = app::AppState::new_live(None, false, None);

    let rendered = render_live_lines(&app, 80, 24);
    assert_live_shell_frame_invariants(&rendered, 80, 24);

    let lines = rendered.lines().collect::<Vec<_>>();
    let title_row = find_line_containing(
        &lines,
        &theme.live_shell.empty_state.title.to_ascii_uppercase(),
    )
    .or_else(|| find_line_containing(&lines, theme.live_shell.empty_state.title))
    .expect("title row");
    let metadata_row = find_line_containing(&lines, "Launch: default · -").expect("metadata row");
    let value_prop_row = find_line_containing(&lines, theme.live_shell.empty_state.value_prop)
        .expect("value prop row");
    let help_row = find_line_containing(&lines, "Ctrl+p commands").expect("key hint row");

    assert!(
        title_row > 0,
        "empty state title should not render flush against the header"
    );
    assert!(title_row <= metadata_row);
    assert!(metadata_row < value_prop_row);
    assert!(value_prop_row < help_row);
    assert!(title_row < value_prop_row);
    assert!(value_prop_row < help_row);
}

#[cfg(test)]
#[test]
fn startup_home_matches_live_empty_shell_language() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let startup_render = render_live_lines(&startup, 100, 24);
    let live_render = render_live_lines(&live, 100, 24);

    for marker in [
        "╻ ╻  ┏━┓  ┏━┓  ┏┓╻",
        "Ask anything... \"What is the tech stack of this project?\"",
    ] {
        assert!(
            startup_render.contains(marker),
            "startup missing {marker}\n{startup_render}"
        );
    }
    for marker in ["Harness", "Launch: worker · model-1"] {
        assert!(
            live_render.contains(marker),
            "live empty missing {marker}\n{live_render}"
        );
    }

    assert!(!startup_render.contains("Dispatch a new run"));
    assert!(!startup_render.contains("Launch: worker · model-1"));
    assert!(live_render.contains("Start a conversation to begin"));
    assert!(!startup_render.contains("● Tip"));
    assert!(!live_render.contains("Waiting for first turn…"));
}

#[cfg(test)]
#[test]
fn live_empty_state_uses_shared_home_surface_tokens() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let startup_render = render_live_lines(&startup, 100, 24);
    let startup_buffer = render_live_cells(&startup, 100, 24);
    let live_render = render_live_lines(&live, 100, 24);
    let live_buffer = render_live_cells(&live, 100, 24);
    let _theme = Theme::default();

    assert!(
        !startup_render.contains("open a fresh session in this directory"),
        "startup should not render purpose copy below the logo\n{startup_render}"
    );
    assert!(
        startup_render.contains("Ask anything... \"What is the tech stack of this project?\""),
        "startup should keep the prompt accessible in the minimal shell\n{startup_render}"
    );
    assert!(live_render.contains("Session"));
    assert!(!live_render.contains('┌') && !live_render.contains('╭'));
    assert!(live_render.contains("Harness"));
    assert!(live_render.contains("Launch: worker · model-1"));
    assert_row_segment_background(
        &startup_buffer,
        100,
        "Worker model-1 mock",
        ratatui::style::Color::Rgb(0x1E, 0x1E, 0x1E),
    );
    assert_row_segment_background(
        &live_buffer,
        100,
        "Worker model-1 mock",
        ratatui::style::Color::Rgb(0x1E, 0x1E, 0x1E),
    );
}

#[cfg(test)]
#[test]
fn startup_and_live_empty_share_spacing_contract() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(app::LaunchMetadata::from_model_ref(
        "worker",
        "mock:model-1",
    ));

    let startup_render = render_live_lines(&startup, 100, 24);
    let startup_lines = startup_render.lines().collect::<Vec<_>>();
    let startup_title =
        find_line_containing(&startup_lines, "╻ ╻  ┏━┓  ┏━┓  ┏┓╻").expect("startup logo");
    let startup_prompt = find_line_containing(
        &startup_lines,
        "Ask anything... \"What is the tech stack of this project?\"",
    )
    .expect("startup prompt");
    let startup_keys = find_line_containing(&startup_lines, "commands").expect("startup key hints");

    let live_render = render_live_lines(&live, 100, 24);
    let live_lines = live_render.lines().collect::<Vec<_>>();
    let live_metadata =
        find_line_containing(&live_lines, "Launch: worker · model-1").expect("live metadata");
    let live_value = find_line_containing(&live_lines, "Start a conversation to begin")
        .expect("live value prop");
    let live_keys = find_line_containing(&live_lines, "Ctrl+p commands").expect("live key hints");

    assert!(startup_title < startup_prompt);
    assert!(startup_prompt < startup_keys);

    assert!(live_metadata < live_value);
    assert!(live_value < live_keys);
}

#[cfg(test)]
#[test]
fn live_shell_type_first_input_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "draft prompt".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let rendered = render_live_lines(&app, 80, 24);

    assert_live_shell_frame_invariants(&rendered, 80, 24);
    assert!(rendered.contains("Waiting for first turn…"));
    assert!(rendered.contains("draft prompt"));
    assert!(!rendered.contains("┌Session"));
    assert!(!rendered.contains("Start a conversation to begin"));
    assert_live_shell_document_composer_contract(
        &app,
        80,
        24,
        Some("draft prompt"),
        None,
        "q quit",
    );
}

#[cfg(test)]
#[test]
fn live_shell_shift_enter_keeps_draft_multiline() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "first line".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for c in "second line".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_history.len(), 0);
    assert_eq!(app.prompt_buffer, "first line\nsecond line");
    assert_live_shell_contains(&app, 80, 24, &["first line", "second line"]);
    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains("Composer ·"));
    assert!(rendered.contains("first line"));
    assert!(rendered.contains("second line"));
}

#[cfg(test)]
#[test]
fn live_shell_enter_submits_and_echoes_prompt_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(app.prompt_buffer, "");
    assert_eq!(
        app.prompt_history.last().map(String::as_str),
        Some("ship it")
    );
    let rendered = render_live_lines(&app, 80, 24);

    assert_live_shell_frame_invariants(&rendered, 80, 24);
    assert!(!rendered.contains("user (pending turn)"));
    assert!(rendered.contains("ship it"));
    assert!(!rendered.contains("   Waiting for response…"));
    assert!(rendered.contains("⠋ Assistant"));
    assert!(!rendered.contains('╭'));
}

#[cfg(test)]
#[test]
fn live_submitted_event_merges_duplicate_local_echo_before_rendering_response() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let local_echo = app.activities.back_mut().expect("optimistic local echo");
    local_echo.status = app::ActivityStatus::Done;
    local_echo.transcript_text = "Ack.".to_string();
    app.activities
        .push_back(transcript_turn_group_test_activity(
            "req_live_echo_merge",
            app::ActivityStatus::Done,
            None,
            "Ack.",
        ));
    app.selected_activity_index = 1;
    app.follow_mode = false;

    app.ingest_event(envelope(
        1,
        Some("req_live_echo_merge"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_live_echo_merge".to_string(),
                text: "ship it".to_string(),
            },
        ),
    ));

    assert_eq!(app.activities.len(), 1);
    assert_eq!(app.selected_activity_index, 0);
    let activity = app.activities.back().expect("merged activity");
    assert_eq!(activity.request_id, "req_live_echo_merge");
    assert_eq!(activity.status, app::ActivityStatus::Done);
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
    let rendered = render_live_lines(&app, 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(count_lines_containing(&lines, "Ack."), 1, "{rendered}");
    assert_eq!(
        count_lines_containing(&lines, "Waiting for response…"),
        0,
        "{rendered}"
    );
}

#[cfg(test)]
#[test]
fn live_provider_request_id_alias_reuses_local_turn_placeholder() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "hi".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    app.ingest_event(envelope(
        1,
        Some("turn_req_alias"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "turn_req_alias".to_string(),
                text: "hi".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("turn_req_alias"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "provider_req_alias".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "hi".to_string(),
                request_digest: "digest-provider-alias".to_string(),
                metadata: None,
            },
        ),
    ));

    assert_eq!(app.activities.len(), 1);
    assert_eq!(app.activities[0].request_id, "turn_req_alias");
    assert_eq!(
        app.activities[0]
            .request_data
            .as_ref()
            .map(|data| data.request_id.as_str()),
        Some("provider_req_alias")
    );

    let rendered = render_live_lines(&app, 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(
        count_lines_containing(&lines, "Waiting for response…"),
        0,
        "{rendered}"
    );
}

#[cfg(test)]
#[test]
fn live_submitted_event_adopts_matching_local_echo_that_is_not_last() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities
        .push_back(transcript_turn_group_test_activity(
            "",
            app::ActivityStatus::Streaming,
            Some("ship it"),
            "",
        ));
    app.activities
        .push_back(transcript_turn_group_test_activity(
            "",
            app::ActivityStatus::Streaming,
            Some("other draft"),
            "",
        ));

    app.ingest_event(envelope(
        1,
        Some("req_non_last_echo"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_non_last_echo".to_string(),
                text: "ship it".to_string(),
            },
        ),
    ));

    assert_eq!(app.activities.len(), 2);
    assert_eq!(app.activities[0].request_id, "req_non_last_echo");
    assert_eq!(
        app.activities[0]
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
    assert_eq!(app.activities[1].request_id, "");
    assert_eq!(
        app.activities[1]
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("other draft")
    );
}

#[cfg(test)]
#[test]
fn live_shell_inline_tool_state_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_inline_tool"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_inline_tool".to_string(),
                text: "Read the file".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_inline_tool"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_inline_tool".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-inline-tool".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_inline_tool"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_inline_tool".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs"}"#.to_string(),
                args_digest: "digest-inline-tool-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(permission_requested_event(
        4,
        "perm_inline_tool",
        "tc_inline_tool",
    ));

    let rendered = render_live_lines(&app, 80, 24);
    println!("{rendered}");

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission required",
            "Apply hashline edit to demo.txt",
            "tool fs.read · dig digest…",
            "Allow once",
        ],
    );
}

#[cfg(test)]
#[test]
fn narrow_transcript_wrapped_top_level_turns_keep_alignment() {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_wrap_alignment";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "wrapping transcript rows".to_string(),
                request_digest: "digest-wrap-alignment".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "assistant reply wraps across the narrow transcript column while keeping the same left alignment on each continuation row for readability".to_string(),
            },
        ),
    ));

    let rendered = render_live_lines(&app, 60, 18);
    let lines = rendered.lines().collect::<Vec<_>>();

    let user_first = find_line_containing(&lines, "alpha bravo").expect("wrapped user first row");
    let user_continuation = lines
        .iter()
        .enumerate()
        .skip(user_first + 1)
        .find_map(|(index, line)| {
            (line.contains('┃') && line.chars().any(char::is_alphanumeric)).then_some(index)
        })
        .expect("wrapped user continuation row");
    let assistant_first =
        find_line_containing_from(&lines, user_continuation + 1, "assistant reply wraps")
            .expect("wrapped assistant first row");
    let assistant_footer = find_line_containing_from(&lines, assistant_first + 1, "Assistant")
        .expect("assistant footer row");
    let assistant_continuation = lines
        .iter()
        .enumerate()
        .skip(assistant_first + 1)
        .take(assistant_footer.saturating_sub(assistant_first + 1))
        .find_map(|(index, line)| line.chars().any(char::is_alphanumeric).then_some(index))
        .expect("wrapped assistant continuation row");

    assert_eq!(
        first_alphanumeric_column(lines[user_first]),
        first_alphanumeric_column(lines[user_continuation]),
        "wrapped user continuations should keep the same text column in narrow layouts\n{rendered}"
    );
    assert!(lines[user_first].contains('┃'));
    assert!(lines[user_continuation].contains('┃'));
    assert_eq!(
        first_alphanumeric_column(lines[assistant_first]),
        first_alphanumeric_column(lines[assistant_continuation]),
        "wrapped assistant continuations should keep the same text column in narrow layouts\n{rendered}"
    );
}

#[cfg(test)]
#[test]
fn live_shell_permission_preserves_draft_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_snapshot",
        "tool_call_snapshot",
    ));

    let rendered = render_live_lines(&app, 80, 24);
    println!("{rendered}");

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Permission required",
            "Draft preserved · keep this draft",
            "Allow once",
        ],
    );
}

#[cfg(test)]
#[test]
fn live_shell_degraded_bootstrap_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    println!("{}", render_live_lines(&app, 80, 24));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Degraded",
            "live stream lagged by 2; replaying from seq 1",
            "Draft locally until recovery completes.",
        ],
    );
    assert!(!render_live_lines(&app, 80, 24)
        .contains("Draft preserved locally while recovery completes."));
}

#[cfg(test)]
#[test]
fn live_shell_disconnected_stream_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some("live event stream disconnected".to_string()));
    println!("{}", render_live_lines(&app, 80, 24));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Disconnected",
            "live event stream disconnected",
            "Reopen the TUI, then continue from the transcript.",
        ],
    );
    assert!(!render_live_lines(&app, 80, 24)
        .contains("Draft preserved locally — reopen the TUI to reconnect."));
}

#[cfg(test)]
#[test]
fn live_status_strip_suppresses_request_digest_banner_details() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(key(crossterm::event::KeyCode::Char('x')));
    app.set_status_banner(Some(
        "mock fixture missing for request_digest=digest-qa-crowding".to_string(),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("request_digest="));
    assert!(!rendered.contains("digest-qa-crowding"));
    assert!(!app.runtime_state().summary.contains("request_digest="));
    assert!(!app.runtime_state().summary.contains("digest-qa-crowding"));
}

#[cfg(test)]
#[test]
fn live_status_strip_suppresses_request_digest_from_cancelled_summary() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "req_cancelled_visual".to_string(),
            reason: "mock fixture missing for request_digest=digest-cancelled-visual".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));

    let rendered = render_live_lines(&app, 160, 24);
    assert!(!rendered.contains("request_digest="));
    assert!(!rendered.contains("digest-cancelled-visual"));
    assert!(!app.runtime_state().summary.contains("request_digest="));
    assert!(!app
        .runtime_state()
        .summary
        .contains("digest-cancelled-visual"));
}

#[cfg(test)]
fn orchestration_details_drawer_card_body(app: &app::AppState, height: u16, width: u16) -> String {
    ui::orchestration_card_text_for_test(app, height, width).join("\n")
}

#[cfg(test)]
fn operator_sidebar_text(app: &app::AppState) -> String {
    ui::operator_sidebar_text_for_test(app).join("\n")
}

#[cfg(test)]
fn operator_sidebar_edit_only_event(seq: u64) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        None,
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: format!("edit_{seq}"),
            path: "src/ui_secondary.rs".to_string(),
            new_file_digest: format!("digest-edit-{seq}"),
            diff_rel_path: Some(format!("artifacts/edit-{seq}.diff")),
            diff_digest: Some(format!("digest-edit-artifact-{seq}")),
        }),
    )
}

#[cfg(test)]
fn operator_sidebar_empty_live_app() -> app::AppState {
    app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    )
}

#[cfg(test)]
fn operator_sidebar_todo_live_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app
}

#[cfg(test)]
fn operator_sidebar_modified_files_live_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(operator_sidebar_edit_only_event(1));
    app
}

#[cfg(test)]
fn operator_sidebar_todo_replay_app() -> app::AppState {
    app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events())
}

#[cfg(test)]
fn operator_sidebar_modified_files_replay_app() -> app::AppState {
    app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![operator_sidebar_edit_only_event(1)],
    )
}

#[cfg(test)]
fn operator_sidebar_child_navigation_replay_app() -> app::AppState {
    let mut events = session_view_events();
    let metadata = harness_core::event::ToolCallMetadata {
        canonical_tool_id: Some("task".to_string()),
        lineage: Some(harness_core::event::TaskLineageMetadata {
            parent_session_id: Some("parent_run".to_string()),
            child_session_id: Some("child_run".to_string()),
            ..harness_core::event::TaskLineageMetadata::default()
        }),
        artifact_refs: vec![harness_core::event::EventArtifactRef {
            path: "artifacts/toolcalls/task/result.json".to_string(),
            digest: Some("digest-task-artifact".to_string()),
        }],
        ..harness_core::event::ToolCallMetadata::default()
    };
    events.push(envelope(
        11,
        Some("req_001"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_child_nav".to_string(),
                tool_id: "task".to_string(),
                args_summary: r#"{"title":"inspect child session"}"#.to_string(),
                args_digest: "digest-tool-child-nav".to_string(),
                metadata: Some(metadata.clone()),
            },
        ),
    ));
    events.push(envelope(
        12,
        Some("req_001"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_child_nav".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("child session recorded".to_string()),
                output_digest: Some("digest-tool-child-nav-output".to_string()),
                output_json: None,
                metadata: Some(metadata),
            },
        ),
    ));
    app::AppState::new_replay(PathBuf::from("/tmp/child_run"), events)
}

#[cfg(test)]
fn assert_operator_sidebar_expanded(
    app: &app::AppState,
    modified_files_heading: &str,
    expected_marker: &str,
    compact_width: u16,
) {
    let plan = layout::FrameLayoutPlan::for_app(app, ratatui::layout::Rect::new(0, 0, 160, 30));
    let sidebar = plan.operator_sidebar.unwrap_or_else(|| {
        panic!(
            "expanded operator sidebar for marker {expected_marker:?}; replay={}, startup={}, subagent={}",
            app.replay_mode,
            app.startup_shell_visible(),
            app.current_subagent_session_present()
        )
    });
    let sidebar_text = operator_sidebar_text(app);
    let rendered = render_live_lines(app, 160, 30);

    assert_eq!(
        sidebar.width, compact_width,
        "persistent operator rail width should stay fixed"
    );
    assert_eq!(plan.wheel_hit_areas.overlay, Some(sidebar));
    assert!(sidebar_text.contains("▼ MCP"));
    assert!(sidebar_text.contains("▼ LSP"));
    assert!(sidebar_text.contains(modified_files_heading));
    assert!(sidebar_text.contains(expected_marker));
    assert!(rendered.contains(expected_marker));
}

#[cfg(test)]
fn assert_markers_in_order(text: &str, markers: &[&str]) {
    let mut search_from = 0usize;
    for marker in markers {
        let relative = text[search_from..]
            .find(marker)
            .unwrap_or_else(|| panic!("missing marker {marker:?} in\n{text}"));
        search_from += relative;
    }
}

#[cfg(test)]
#[test]
fn live_shell_details_drawer_orchestration_snapshot() {
    let app = orchestration_details_drawer_app(0);
    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);

    println!("{card_body}");
    insta::assert_snapshot!(card_body, @r###"
overview · 1 active agents · 2 queued · 1 running · 1 stale
watch · stale for 3001 ms
 stale  task_stale · w1/deep · scan
 running  task_run · supervisor/n/a · queue:none
 queued  task_queue · system/n/a · tool:read
 queued  tool_call_1 · system/n/a · tool:fs.read
 completed  task_done · w2/scout · tool:done
"###);
}

#[cfg(test)]
#[test]
fn live_shell_details_drawer_orchestration_primary_snapshot() {
    let app = orchestration_details_drawer_app(0);

    let rendered = render_live_lines(&app, 100, 30);
    println!("{rendered}");
    assert!(rendered.contains("Explain the refactor"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▼ LSP"));
    assert!(rendered.contains("▶ Modified Files"));
    assert!(!rendered.contains("turn 1"));
    assert!(!rendered.contains("Current runtime: default · gpt-5-codex"));
    assert!(!rendered.contains("provider openai"));
    assert!(
        rendered.contains("No MCP integrations configured")
            || rendered.contains("No MCP servers configured")
            || rendered.contains("websearch Disconnected")
    );
    assert!(rendered.contains("No active LSP servers"));
    assert!(!rendered.contains("No modified files"));
}

#[cfg(test)]
#[test]
fn live_shell_details_drawer_orchestration_overflow_snapshot() {
    let app = orchestration_details_drawer_app(4);
    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);

    println!("{card_body}");
    insta::assert_snapshot!(card_body, @r###"
overview · 1 active agents · 2 queued · 1 running · 1 stale
watch · stale for 3001 ms
 stale  task_stale · w1/deep · scan
 running  task_run · supervisor/n/a · queue:none
 queued  task_queue · system/n/a · tool:read
 queued  tool_call_1 · system/n/a · tool:fs.read
+5 more
"###);
}

#[cfg(test)]
#[test]
fn live_details_drawer_orchestration_warning_fallback() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app.handle_key(focus_cycle_key());
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);
    assert!(card_body.contains("watch · none"));
    assert!(card_body.contains("overview · 0 active agents · 1 queued · 0 running · 0 stale"));
}

#[cfg(test)]
#[test]
fn layout_plan_primary_geometry_docks_live_details_sidebar() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app.handle_key(focus_cycle_key());
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));

    assert_eq!(plan.shell, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 100, 24))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(
        plan.details_overlay,
        Some(ratatui::layout::Rect::new(58, 0, 42, 24))
    );
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(0, 24, 100, 5))
    );
    assert_eq!(
        plan.disclosure,
        Some(ratatui::layout::Rect::new(0, 29, 100, 1))
    );
}

#[cfg(test)]
#[test]
fn layout_plan_minimum_geometry_stacks_live_details_drawer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    app.handle_key(focus_cycle_key());
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 80, 24));

    assert_eq!(plan.shell, ratatui::layout::Rect::new(2, 0, 76, 24));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(2, 0, 76, 18))
    );
    assert_eq!(
        plan.details_overlay,
        Some(ratatui::layout::Rect::new(36, 0, 42, 18))
    );
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(2, 18, 76, 5))
    );
    assert_eq!(
        plan.disclosure,
        Some(ratatui::layout::Rect::new(2, 23, 76, 1))
    );
}

#[cfg(test)]
#[test]
fn live_session_shell_removes_tab_chrome_and_debug_drawer() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 160, 48));
    let rendered = render_live_lines(&app, 160, 48);

    assert!(plan.operator_sidebar.is_some());
    assert!(plan.details_overlay.is_none());
    assert!(!rendered.contains("Tabs"));
    assert!(rendered.contains("Explain the refactor"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▶ Modified Files"));
}

#[cfg(test)]
#[test]
fn wide_shell_hides_header_when_sidebar_is_visible() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 160, 48));
    let rendered = render_live_lines(&app, 160, 48);
    let lines = rendered.lines().collect::<Vec<_>>();
    let first_line = lines.first().copied().unwrap_or_default().to_string();

    assert_eq!(plan.header.height, 0);
    assert!(plan.operator_sidebar.is_some());
    assert!(plan.live_anchor.is_none());
    assert!(!first_line.contains("run run_fixture"));
    assert!(
        lines.iter().take(4).any(|line| {
            line.contains("Explain the refactor")
                || line.contains("Working through the steps.")
                || line.contains("Read src/ui.rs")
        }),
        "wide shell transcript content should begin immediately at the top of the shell\n{rendered}"
    );
}

#[cfg(test)]
#[test]
fn replay_shell_uses_read_only_operator_layout() {
    let app = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 160, 48));
    let rendered = render_live_lines(&app, 160, 48);

    assert!(plan.header.height > 0);
    assert!(plan.operator_sidebar.is_some());
    assert!(plan.details_overlay.is_none());
    assert!(!rendered.contains("Tabs"));
    assert!(rendered.contains("Replay · read-only"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▶ Modified Files"));
}

#[cfg(test)]
#[test]
fn replay_read_only_composer_matches_quiet_contract() {
    let app =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());

    assert_replay_read_only_composer_contract(&app, 100, 24, "Replay · read-only", "? shortcuts");

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Replay archive · read-only"));
    assert!(!rendered.contains("Composer ·"));
}

#[cfg(test)]
#[test]
fn replay_read_only_quiet_contract_survives_primary_compact_and_dense_geometries() {
    let app =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());

    assert_replay_read_only_composer_contract(&app, 100, 30, "Replay · read-only", "? shortcuts");
    assert_replay_read_only_composer_contract(&app, 90, 36, "Replay · read-only", "? shortcuts");
    assert_replay_read_only_composer_contract(&app, 80, 24, "Replay · read-only", "? shortcuts");
    assert_replay_read_only_composer_contract(&app, 60, 18, "Replay · read-only", "q quit");
}

#[cfg(test)]
#[test]
fn completed_shell_uses_disabled_quiet_composer() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.ingest_event(envelope(
        1,
        Some("req_completed_quiet_composer"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    assert_live_shell_document_composer_contract(&app, 100, 30, None, None, "Tab focus");

    let rendered = render_live_lines(&app, 100, 30);
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
    assert!(!rendered.contains("Composer ·"));
}

#[cfg(test)]
#[test]
fn replay_and_completed_states_preserve_read_only_and_session_preserved_copy() {
    let mut replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    replay.focus = app::Focus::Prompt;
    for ch in "blocked in replay".chars() {
        replay.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    replay.handle_key(key(crossterm::event::KeyCode::Enter));

    let replay_render = render_live_lines(&replay, 100, 24);
    assert!(replay.prompt_buffer.is_empty());
    assert!(replay_render.contains("Replay · read-only"));
    assert!(replay_render.contains("Replay is read-only"));
    assert!(!replay_render.contains("blocked in replay"));

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_copy_guard"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    let completed_render = render_live_lines(&completed, 100, 30);
    assert!(completed_render.contains("Tab focus"));
    assert!(completed_render.contains("q quit"));
}

#[cfg(test)]
fn assert_live_shell_geometry(width: u16, height: u16) {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let rendered = render_live_lines(&app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    let lines = rendered.lines().collect::<Vec<_>>();
    assert_live_shell_composer_progressive_disclosure(&lines, None, "Ctrl+p commands");
}

#[cfg(test)]
fn assert_live_shell_contains(app: &app::AppState, width: u16, height: u16, markers: &[&str]) {
    let rendered = render_live_lines(app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    for marker in markers {
        assert!(
            rendered.contains(marker),
            "expected live shell to contain {marker:?}\n{rendered}"
        );
    }
}

#[cfg(test)]
fn runtime_overlay_text(app: &app::AppState, max_chars: usize) -> ui::RuntimeOverlayTextForTest {
    ui::runtime_overlay_text_for_test(app, max_chars).expect("runtime overlay")
}

#[cfg(test)]
fn assert_live_shell_document_composer_contract(
    app: &app::AppState,
    width: u16,
    height: u16,
    composer_marker: Option<&str>,
    composer_footer_marker: Option<&str>,
    global_footer_marker: &str,
) {
    let rendered = render_live_lines(app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    let plan =
        layout::FrameLayoutPlan::for_app(app, ratatui::layout::Rect::new(0, 0, width, height));
    let dock = plan.dock.expect("live shell dock layout");
    let composer = dock.composer;
    let dock_width = usize::from(composer.width);
    let lines = rendered.lines().collect::<Vec<_>>();
    let composer_first_row = usize::from(composer.y);
    let composer_last_row =
        composer_first_row.saturating_add(usize::from(composer.height.saturating_sub(1)));
    let disclosure_row = dock.disclosure.map(|band| usize::from(band.y));
    let composer_input_row = match composer_marker {
        Some(marker) => {
            let input_row = (composer_first_row..=composer_last_row)
                .find(|&index| line_has_composer_text(lines[index]))
                .unwrap_or_else(|| panic!("missing composer input row for {marker:?}\n{rendered}"));
            assert!(
                lines[composer_first_row..=composer_last_row]
                    .iter()
                    .any(|line| line.contains(marker)),
                "missing composer marker {marker:?} inside the prompt shell\n{rendered}"
            );
            input_row
        }
        None => {
            let legacy_markers = [
                "Ask Harness to inspect, edit, or explain…",
                "Queue the next turn while this one finishes…",
                "Ask Harness what to retry, inspect, or fix…",
                "Draft preserved locally while recovery completes.",
                "Draft preserved locally — reopen the TUI to reconnect.",
                "Run complete — use ctrl+p commands",
            ];
            assert!(
                !lines[composer_first_row..=composer_last_row]
                    .iter()
                    .any(|line| legacy_markers.iter().any(|marker| line.contains(marker))),
                "live composer should stay blank when no draft is present\n{rendered}"
            );
            composer_first_row
        }
    };
    let global_footer_row =
        find_line_containing_from(&lines, 0, global_footer_marker)
            .or_else(|| find_line_containing_from(&lines, 0, "Ctrl+p commands"))
            .or_else(|| find_line_containing_from(&lines, 0, "q quit"))
            .or_else(|| find_line_containing_from(&lines, 0, "Enter send"))
            .or_else(|| find_line_containing_from(&lines, composer_last_row + 1, "q quit"))
            .unwrap_or_else(|| {
                panic!(
                    "missing global footer marker {global_footer_marker:?} for {composer_marker:?}\n{rendered}"
                )
            });
    assert!(
        find_line_containing_in_range(&lines, composer_first_row, composer_input_row, "Composer ·")
            .is_none(),
        "metadata headline row must stay removed\n{rendered}"
    );
    assert!(
        lines[composer_first_row..=composer_last_row]
            .iter()
            .all(|line| line.chars().take(dock_width).count() <= dock_width),
        "composer shell rows must stay within the dock width\n{rendered}"
    );

    match composer_footer_marker {
        Some(marker) => {
            let composer_footer_row =
                find_line_containing_from(&lines, composer_last_row + 1, marker).unwrap_or_else(
                    || {
                        panic!(
                    "missing composer footer marker {marker:?} for {composer_marker:?}\n{rendered}"
                )
                    },
                );
            assert_eq!(
                composer_footer_row,
                composer_last_row + 1,
                "composer hint row should sit directly under the prompt shell\n{rendered}"
            );
            assert!(
                global_footer_row < composer_first_row
                    || global_footer_row <= composer_last_row
                    || global_footer_row >= composer_footer_row,
                "global footer should live above the dock, in the composer metadata row, or below the helper row\n{rendered}"
            );
        }
        None => {
            assert!(
                global_footer_row < composer_first_row
                    || global_footer_row <= composer_last_row
                    || Some(global_footer_row) == disclosure_row
                    || Some(global_footer_row) == disclosure_row.map(|row| row + 1),
                "the global footer should live above the dock, in the composer metadata row, the disclosure row, or directly under it\n{rendered}"
            );
        }
    }
}

#[cfg(test)]
fn assert_replay_read_only_composer_contract(
    app: &app::AppState,
    width: u16,
    height: u16,
    header_marker: &str,
    hint_marker: &str,
) {
    let rendered = render_live_lines(app, width, height);
    assert_live_shell_frame_invariants(&rendered, width, height);

    let lines = rendered.lines().collect::<Vec<_>>();
    let header_row = find_line_containing(&lines, header_marker).unwrap_or_else(|| {
        panic!("missing replay header marker {header_marker:?} in shell\n{rendered}")
    });
    let composer_row = find_line_containing_from(&lines, header_row + 1, "▎ Replay is read-only.")
        .unwrap_or_else(|| {
            panic!("missing replay read-only body row for header {header_marker:?}\n{rendered}")
        });
    let divider_row = composer_row.saturating_sub(1);
    let hint_row =
        find_line_containing_from(&lines, composer_row + 1, hint_marker).unwrap_or_else(|| {
            panic!(
            "missing replay shortcut row {hint_marker:?} for header {header_marker:?}\n{rendered}"
        )
        });

    assert!(
        header_row < divider_row,
        "replay identity should sit in header context\n{rendered}"
    );
    assert_eq!(
        hint_row,
        composer_row + 1,
        "replay should keep one compact shortcut row under the disabled rail row\n{rendered}"
    );
    assert!(
        find_line_containing_in_range(&lines, divider_row, composer_row, "Replay archive ·")
            .is_none(),
        "replay composer should not render a metadata headline row\n{rendered}"
    );
    assert!(
        !lines[composer_row].contains("run run_fixture"),
        "replay identity should stay out of the disabled rail row\n{rendered}"
    );
}

#[cfg(test)]
pub(super) fn render_live_lines(app: &app::AppState, width: u16, height: u16) -> String {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create live shell terminal");
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .expect("draw live shell frame");

    terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
fn transcript_turn_group_test_activity(
    request_id: &str,
    status: app::ActivityStatus,
    user_text: Option<&str>,
    transcript_text: &str,
) -> app::ActivityEntry {
    app::ActivityEntry {
        request_id: request_id.to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status,
        user_message: user_text.map(|text| harness_core::event::UserMessageSubmittedEvent {
            request_id: request_id.to_string(),
            text: text.to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: transcript_text.to_string(),
        usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }
}

#[cfg(test)]
fn live_status_strip_row(app: &app::AppState, width: u16, height: u16, marker: &str) -> String {
    let rendered = render_live_lines(app, width, height);
    let lines = rendered.lines().collect::<Vec<_>>();
    let row = lines
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, line)| line.contains(marker).then_some(index))
        .expect("status row");
    lines[row].trim().to_string()
}

#[cfg(test)]
fn assert_live_shell_frame_invariants(rendered: &str, width: u16, height: u16) {
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        height as usize,
        "row count must match geometry"
    );
    assert!(
        lines
            .iter()
            .all(|line| line.chars().count() == width as usize),
        "every row must preserve the requested width"
    );
}

#[cfg(test)]
fn find_line_containing(lines: &[&str], needle: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(needle))
}

#[cfg(test)]
fn find_line_containing_all(lines: &[&str], needles: &[&str]) -> Option<usize> {
    lines
        .iter()
        .position(|line| needles.iter().all(|needle| line.contains(needle)))
}

#[cfg(test)]
fn find_last_line_containing(lines: &[&str], needle: &str) -> Option<usize> {
    lines.iter().rposition(|line| line.contains(needle))
}

#[cfg(test)]
fn count_lines_containing(lines: &[&str], needle: &str) -> usize {
    lines.iter().filter(|line| line.contains(needle)).count()
}

#[cfg(test)]
fn find_line_containing_from(lines: &[&str], start: usize, needle: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| line.contains(needle).then_some(index))
}

#[cfg(test)]
fn find_line_containing_all_from(lines: &[&str], start: usize, needles: &[&str]) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| {
            needles
                .iter()
                .all(|needle| line.contains(needle))
                .then_some(index)
        })
}

#[cfg(test)]
fn find_line_containing_in_range(
    lines: &[&str],
    start: usize,
    end_exclusive: usize,
    needle: &str,
) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .take(end_exclusive.saturating_sub(start))
        .find_map(|(index, line)| line.contains(needle).then_some(index))
}

#[cfg(test)]
fn first_alphanumeric_column(line: &str) -> usize {
    line.chars()
        .position(char::is_alphanumeric)
        .unwrap_or_else(|| panic!("line is missing alphanumeric content: {line:?}"))
}

#[cfg(test)]
fn first_non_whitespace_column(line: &str) -> usize {
    line.chars()
        .position(|ch| !ch.is_whitespace())
        .unwrap_or_else(|| panic!("line is missing visible content: {line:?}"))
}

#[cfg(test)]
fn live_shell_composer_input_span(lines: &[&str]) -> (usize, usize, usize) {
    let composer_first_row = (0..lines.len())
        .find(|&index| composer_shell_line(lines[index]))
        .expect("composer shell row");
    let composer_input_row = (composer_first_row..lines.len())
        .find(|&index| line_has_composer_text(lines[index]))
        .expect("composer input row");
    let mut composer_last_row = composer_first_row;
    while composer_last_row + 1 < lines.len() && composer_shell_line(lines[composer_last_row + 1]) {
        composer_last_row += 1;
    }

    (composer_first_row, composer_input_row, composer_last_row)
}

#[cfg(test)]
fn composer_shell_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('▎') || trimmed.starts_with('┃') || trimmed.starts_with('╹')
}

#[cfg(test)]
fn line_has_composer_text(line: &str) -> bool {
    let trimmed = line.trim_start();
    (trimmed.starts_with('▎') || trimmed.starts_with('┃') || trimmed.starts_with('╹'))
        && trimmed.chars().skip(1).any(char::is_alphanumeric)
}

#[cfg(test)]
fn assert_live_shell_composer_progressive_disclosure(
    lines: &[&str],
    composer_marker: Option<&str>,
    footer_marker: &str,
) {
    let footer_row = find_line_containing(lines, footer_marker)
        .or_else(|| find_last_line_containing(lines, "q quit"))
        .or_else(|| {
            lines
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, line)| (!line.trim().is_empty()).then_some(index))
        })
        .expect("footer row");
    let composer_last_row = footer_row.saturating_sub(1);
    let mut composer_first_row = composer_last_row;
    while composer_first_row > 0 && composer_shell_line(lines[composer_first_row.saturating_sub(1)])
    {
        composer_first_row = composer_first_row.saturating_sub(1);
    }
    let composer_input = match composer_marker {
        Some(marker) => {
            let input_row = (composer_first_row..=composer_last_row)
                .find(|&index| line_has_composer_text(lines[index]))
                .expect("composer input row");
            assert!(
                lines[composer_first_row..=composer_last_row]
                    .iter()
                    .any(|line| line.contains(marker)),
                "composer marker should stay inside the prompt shell"
            );
            input_row
        }
        None => composer_first_row,
    };

    assert!(lines[..composer_first_row]
        .iter()
        .any(|line| !line.trim().is_empty()));
    assert!(composer_first_row <= composer_input);
    assert!(composer_input < footer_row);

    if let Some(headline_row) =
        find_line_containing_in_range(lines, composer_first_row, composer_input, "Composer")
    {
        assert!(headline_row < composer_input);
    }

    if let Some(hints_row) =
        find_line_containing_in_range(lines, composer_input + 1, footer_row, "Ctrl+p commands")
            .or_else(|| {
                find_line_containing_in_range(
                    lines,
                    composer_input + 1,
                    footer_row,
                    "ctrl+p commands",
                )
            })
    {
        assert!(composer_input < hints_row);
        assert!(hints_row < footer_row);
    }
}

#[cfg(test)]
#[test]
fn runtime_state_overlay_is_quiet_and_actionable() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));

    let overlay = runtime_overlay_text(&app, 72);
    let rendered = render_live_lines(&app, 80, 24);

    assert_eq!(overlay.badge, "Degraded");
    assert_eq!(overlay.title, "Recovery in progress");
    assert_eq!(
        overlay.summary,
        "Live updates are catching up before sending resumes."
    );
    assert_eq!(
        overlay.detail.as_deref(),
        Some("live stream lagged by 2; replaying from seq 1")
    );
    assert_eq!(overlay.guidance, "Draft locally until recovery completes.");
    assert!(rendered.contains("Recovery in progress"));
    assert!(rendered.contains("Draft locally until recovery completes."));
    assert!(!rendered.contains("Draft preserved locally while recovery completes."));
}

#[cfg(test)]
#[test]
fn runtime_state_overlay_never_stacks_over_permission_modal() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay_precedence",
        "tool_call_overlay_precedence",
    ));

    let rendered = render_live_lines(&app, 80, 24);

    assert!(ui::runtime_overlay_text_for_test(&app, 72).is_none());
    assert!(rendered.contains("Permission required"));
    assert!(rendered.contains("Allow once"));
    assert!(!rendered.contains("Recovery in progress"));
}

#[cfg(test)]
#[test]
fn degraded_and_disconnected_states_share_overlay_structure() {
    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));

    let degraded_overlay = runtime_overlay_text(&degraded, 72);
    let disconnected_overlay = runtime_overlay_text(&disconnected, 72);

    assert!(degraded_overlay.detail.is_some());
    assert!(disconnected_overlay.detail.is_some());
    assert_eq!(
        usize::from(degraded_overlay.detail.is_some()),
        usize::from(disconnected_overlay.detail.is_some())
    );
    assert!(degraded_overlay.title.len() <= 24);
    assert!(disconnected_overlay.title.len() <= 24);
    assert!(degraded_overlay.guidance.ends_with('.'));
    assert!(disconnected_overlay.guidance.ends_with('.'));
}

#[cfg(test)]
#[test]
fn failure_overlay_is_specific_without_visual_noise() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "runtime error: exit code 1\nstderr permission denied".to_string(),
    ));

    let overlay = runtime_overlay_text(&app, 72);
    let rendered = render_live_lines(&app, 80, 24);

    assert_eq!(overlay.badge, "Failure");
    assert_eq!(overlay.title, "Review required");
    assert_eq!(
        overlay.summary,
        "Review the latest failure before continuing."
    );
    assert_eq!(
        overlay.detail.as_deref(),
        Some("runtime error: exit code 1 stderr permission denied")
    );
    assert_eq!(
        overlay.guidance,
        "Review the failure, then retry or continue."
    );
    assert!(!overlay.summary.contains("request_digest="));
    assert!(!overlay
        .detail
        .as_deref()
        .unwrap_or_default()
        .contains("request_digest="));
    assert!(!rendered.contains("Turn attention required"));
}

#[cfg(test)]
#[test]
fn details_drawer_toggles_without_leaving_live_surface() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.details_drawer_open());

    app.handle_key(focus_cycle_key());
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(app.details_drawer_open());
    let open_debug = render_live_buffer(&app, 80, 24);
    assert!(open_debug.contains("▼ MCP"));

    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.details_drawer_open());
    let closed_debug = render_live_buffer(&app, 80, 24);
    assert!(!closed_debug.contains("▼ MCP"));
}

#[cfg(test)]
#[test]
fn operator_sidebar_matches_parity_information_architecture() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = orchestration_details_drawer_app(2);
    let sidebar = operator_sidebar_text(&app);

    assert_markers_in_order(
        &sidebar,
        &["Explain the refactor", "▼ MCP", "▼ LSP", "▶ Modified Files"],
    );
    assert!(
        sidebar.contains("No MCP integrations configured")
            || sidebar.contains("No MCP servers configured")
            || sidebar.contains("websearch Disconnected")
    );
    assert!(sidebar.contains("No active LSP servers"));
    assert!(!sidebar.contains("No modified files"));
    assert!(!sidebar.contains("Todo ·"));
    assert!(!sidebar.contains("Recovery ·"));
}

#[cfg(test)]
#[test]
fn operator_sidebar_uses_secondary_quiet_chrome() {
    let app = orchestration_details_drawer_app(2);
    let rendered = render_live_lines(&app, 160, 48);
    let buffer = render_live_cells(&app, 160, 48);
    let theme = Theme::default();
    let title = "Explain the refactor";
    let (row, _fg, bg) = row_text_and_palette(&buffer, 160, title).expect("sidebar title row");
    let start = row.find(title).expect("sidebar title starts");
    let start = row[..start].chars().count();
    let end = start + title.chars().count();

    assert!(!row[..row.find(title).expect("sidebar title bytes")].contains('│'));
    assert!(bg[start..end]
        .iter()
        .all(|color| *color == theme.surface.panel));
    assert!(!rendered.contains('┌'));
    assert!(!rendered.contains('┐'));
    assert!(!rendered.contains('└'));
    assert!(!rendered.contains('┘'));
}

#[cfg(test)]
#[test]
fn operator_sidebar_uses_explicit_empty_states() {
    harness_core::config::set_registered_integrations_config(
        harness_core::config::IntegrationsConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = app::AppState::new_live(None, false, None);
    let sidebar = operator_sidebar_text(&app);

    assert!(sidebar.contains("▼ MCP"));
    assert!(
        sidebar.contains("No MCP integrations configured")
            || sidebar.contains("No MCP servers configured")
            || sidebar.contains("websearch Disconnected")
    );
    assert!(sidebar.contains("▼ LSP"));
    assert!(sidebar.contains("No active LSP servers"));
    assert!(sidebar.contains("▶ Modified Files"));
    assert!(!sidebar.contains("No modified files"));
}

#[cfg(test)]
#[test]
fn operator_sidebar_recovery_section_surfaces_artifacts_and_navigation_hints() {
    let sidebar = operator_sidebar_text(&operator_sidebar_child_navigation_replay_app());

    assert!(sidebar.contains("▼ MCP"));
    assert!(sidebar.contains("▼ LSP"));
    assert!(sidebar.contains("▶ Modified Files"));
    assert!(!sidebar.contains("Recovery"));
    assert!(!sidebar.contains("Parent session · parent_run"));
    assert!(!sidebar.contains("Child session · child_run"));
    assert!(!sidebar.contains("Artifact · artifacts/toolcalls/task/result.json"));
}

#[cfg(test)]
#[test]
fn operator_sidebar_modified_files_include_diff_artifact_paths() {
    let sidebar = operator_sidebar_text(&operator_sidebar_modified_files_live_app());

    assert_markers_in_order(&sidebar, &["▼ Modified Files", "src/ui_secondary.rs"]);
    assert!(!sidebar.contains("artifacts/edit-1.diff"));
    assert!(!sidebar.contains("Recovery"));
}

#[cfg(test)]
#[test]
fn operator_sidebar_preserves_section_order_and_copy() {
    let app = orchestration_details_drawer_app(2);
    let sidebar = operator_sidebar_text(&app);

    assert_markers_in_order(
        &sidebar,
        &["Explain the refactor", "▼ MCP", "▼ LSP", "▶ Modified Files"],
    );

    let empty = operator_sidebar_text(&app::AppState::new_live(None, false, None));
    assert!(empty.contains("▼ MCP"));
    assert!(empty.contains("▼ LSP"));
    assert!(empty.contains("▶ Modified Files"));
}

#[cfg(test)]
#[test]
fn live_shell_no_longer_renders_debug_inspector_labels() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let rendered = render_live_lines(&app, 160, 48);
    assert!(!rendered.contains("Request ID"));
    assert!(!rendered.contains("Provider:"));
    assert!(!rendered.contains("Model:"));
    assert!(!rendered.contains("Prompt summary"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▶ Modified Files"));
    assert!(!rendered.contains("Todo · 1"));
}

#[cfg(test)]
#[test]
fn review_surfaces_are_command_driven_without_tab_contract() {
    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }
    live.focus = app::Focus::List;

    live.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    live.palette_filtered = vec!["open_event_log".to_string()];
    live.palette_selected = 0;
    live.handle_key(key(crossterm::event::KeyCode::Enter));
    assert_eq!(live.review_surface(), Some(app::ReviewSurface::Events));
    assert!(!live.details_drawer_open());
    let live_events_debug = render_live_buffer(&live, 80, 24);
    assert!(live_events_debug.contains("Event log"));
    assert!(live_events_debug.contains("Event details"));

    live.handle_key(key(crossterm::event::KeyCode::Char('?')));
    assert_eq!(live.review_surface(), Some(app::ReviewSurface::Help));
    let live_help_debug = render_live_buffer(&live, 80, 24);
    assert!(live_help_debug.contains("Live shell:"));

    live.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(live.review_surface(), None);
    assert!(!live.details_drawer_open());

    let mut replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    replay.focus = app::Focus::List;
    replay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    replay.palette_filtered = vec!["open_event_log".to_string()];
    replay.palette_selected = 0;
    replay.handle_key(key(crossterm::event::KeyCode::Enter));
    assert_eq!(replay.review_surface(), Some(app::ReviewSurface::Events));
    let replay_events_debug = render_live_buffer(&replay, 80, 24);
    assert!(!replay_events_debug.contains("Tabs"));
    assert!(replay_events_debug.contains("Selected event"));

    replay.handle_key(key(crossterm::event::KeyCode::Char('?')));
    assert_eq!(replay.review_surface(), Some(app::ReviewSurface::Help));
    let replay_help_debug = render_live_buffer(&replay, 80, 24);
    assert!(replay_help_debug.contains("Replay shell:"));
    assert!(!replay_help_debug.contains("Commands"));
    assert!(!replay_help_debug.contains("Permission required"));

    replay.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(replay.review_surface(), None);
}

#[cfg(test)]
#[test]
fn review_surfaces_restore_panel_chrome() {
    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }
    live.focus = app::Focus::List;

    run_palette_command(&mut live, "open_event_log");
    let events_rendered = render_live_lines(&live, 100, 30);
    assert!(!events_rendered.contains('│'));
    assert!(!events_rendered.contains('┌'));
    assert!(events_rendered.contains("Event details"));

    live.handle_key(key(crossterm::event::KeyCode::Char('?')));
    let help_rendered = render_live_lines(&live, 100, 30);
    assert!(!help_rendered.contains('┌'));
    assert!(help_rendered.contains("Help"));
}
