from pathlib import Path
p=Path('crates/harness-tui/src/lib_tests/theme_layout.rs');s=p.read_text();needle='    assert_eq!(themed_target, Some(ui::WheelTarget::Inspector));\n';assert s.count(needle)==1
s=s.replace(needle,needle+'''
    themed_app.terminal_panel.visible = true;
    themed_app.todo_pane.visible = true;
    let plan = layout::FrameLayoutPlan::for_app(&themed_app, area);
    let todo = plan.todo.unwrap_or_abort();
    let terminal = plan.terminal_panel.unwrap_or_abort();
    let overlay = plan.details_overlay.unwrap_or_abort();
    assert!(overlay.y > todo.y);
    assert_eq!(overlay.bottom(), terminal.bottom());
    assert_eq!(
        ui::hovered_wheel_target(&themed_app, area, overlay.x, terminal.y),
        Some(ui::WheelTarget::Inspector)
    );
    assert_eq!(
        ui::hovered_wheel_target(&themed_app, area, terminal.x, terminal.y),
        Some(ui::WheelTarget::Terminal)
    );
''');p.write_text(s)
p=Path('crates/harness-tui/src/app/tests/permission_modal_tests_part3_test.rs');s=p.read_text();start=s.index('fn question_mouse_click_preserves_shell_state');end=s.index('\n#[test]',start);a=s[start:end]
a=a.replace('    let composer_before = FrameLayoutPlan::for_app(&app, frame_area).composer;\n','')
needle='    // When: the selected row is clicked a second time.\n';assert needle in a
a=a.replace(needle,'''    // Resize before the second click and use the option's new painted location.
    let frame_area = Rect::new(0, 0, 60, 20);
    let option_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::QuestionChoice(1)).then_some(area)
        })
        .unwrap_or_abort();
    let buffer = crate::render_test::render_to_buffer(&app, frame_area, |app, frame, _| {
        crate::ui::render_app(frame, app);
    });
    let row = (option_area.x..option_area.right())
        .map(|x| buffer[(x, option_area.y)].symbol())
        .collect::<String>();
    assert!(row.contains('B'), "painted option: {row}");

'''+needle)
a=a.replace('        composer_before.map(|area| area.width)','        Some(frame_area.width - 4)')
s=s[:start]+a+s[end:];p.write_text(s)
