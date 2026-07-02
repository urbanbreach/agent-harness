use super::*;

pub(super) fn transcript_turn_sections_render_open_rail_surfaces() {
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
    app.transcript_view.selected_activity_index = 0;
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = usize::MAX;

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
    assert!(!lines[user_body].contains('›'));
    assert!(
        user_body == 0 || !lines[user_body - 1].contains("Group these turns"),
        "user message should not duplicate the body above the boxed row\n{rendered}"
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
    let mut plan_activity = transcript_turn_group_test_activity(
        "req_plan_turn_groups",
        app::ActivityStatus::Done,
        Some("Plan this work"),
        "Planned response",
    );
    plan_activity.profile_label = "plan".to_string();
    plan_app.activities = std::collections::VecDeque::from(vec![plan_activity]);
    plan_app.transcript_view.selected_activity_index = 0;
    plan_app.transcript_view.follow_mode = false;
    plan_app.transcript_view.transcript_scroll = usize::MAX;

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
    follow_app.transcript_view.selected_activity_index = 7;
    follow_app.transcript_view.follow_mode = true;

    let followed = render_live_lines(&follow_app, 60, 18);
    assert!(
        followed.contains("question 7") && followed.contains("reply 7"),
        "follow mode should keep the newest grouped turn visible\n{followed}"
    );
    assert!(
        !followed.contains("question 0"),
        "follow mode should scroll past the earliest grouped turn\n{followed}"
    );

    follow_app.transcript_view.follow_mode = false;
    follow_app.transcript_view.transcript_scroll = usize::MAX;

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

pub(super) fn transcript_turn_sections_keep_nested_tool_details() {
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
    app.transcript_view.selected_activity_index = 0;
    app.transcript_view.transcript_scroll = usize::MAX;

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
