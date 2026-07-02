use super::*;

pub(super) fn operator_sidebar_pins_summary_and_hides_empty_sections() {
    ui::exact_test_operator_rail_low_activity_presentation_prefers_primary_stack();
    ui::exact_test_operator_rail_section_model_builds_pinned_summary();
    ui::exact_test_operator_rail_sanitizes_control_chars_in_sidebar_strings();
    ui::exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order();
    ui::exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity();
    ui::exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp();
    ui::exact_test_operator_rail_section_model_surfaces_pending_permissions_first();
}

pub(super) fn operator_sidebar_compact_empty_mode_preserves_anchor_copy_with_fixed_width() {
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

pub(super) fn operator_sidebar_width_stays_fixed_when_todo_or_modified_files_exist() {
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

pub(super) fn replay_narrow_layout_does_not_overlay_operator_rail() {
    let app = app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 60, 40));

    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(plan.details_overlay, None);
    assert_eq!(plan.transcript.expect("transcript area").width, 60);
}
