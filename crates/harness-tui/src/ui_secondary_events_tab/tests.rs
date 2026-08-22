use super::*;

#[test]
fn help_expanded_row_renders_from_intra_row_scroll_offset() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    let width = 30;
    let area = Rect::new(0, 0, width, 6);
    let description_width = crate::ui::ui_overlays::modal_list_row_layout(area, 1)
        .content
        .width
        .saturating_sub(4);
    let (action, description) = app
        .help_rows()
        .into_iter()
        .find_map(|row| match row {
            crate::app::HelpRow::Shortcut {
                action,
                label,
                description,
                ..
            } if description != label
                && rows::wrapped_lines(&description, description_width).len() > 1 =>
            {
                Some((action, description))
            }
            crate::app::HelpRow::Section { .. } | crate::app::HelpRow::Shortcut { .. } => None,
        })
        .expect("long help row");
    app.help_browser.expanded_actions.insert(action);
    let rows = app.help_rows();
    let index = rows
        .iter()
        .position(|row| matches!(row, crate::app::HelpRow::Shortcut { action: candidate, .. } if *candidate == action))
        .expect("expanded action row");
    app.help_browser.selected = index;
    app.help_browser.visual_offset = index.saturating_add(1);
    app.help_browser.follow_selection = false;
    let layout = rows::help_row_layout(&app, area, &rows);

    // act
    let rendered = crate::render_test::render_to_string(&app, area, |app, frame, area| {
        rows::render_browse(frame, app, app.theme(), area);
    });
    let first_description = rows::wrapped_lines(&description, description_width)
        .into_iter()
        .next()
        .expect("wrapped description");

    // assert
    assert!(rendered
        .lines()
        .next()
        .is_some_and(|line| line.contains(&first_description)));
    assert_eq!(
        layout
            .areas
            .iter()
            .find_map(|(row, area)| (*row == index).then_some(*area))
            .map(|area| area.y),
        Some(0)
    );
}

#[test]
fn submit_prompt_detail_includes_a_meaningful_description() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.help_browser.mode = crate::app::HelpMode::Detail {
        action: Action::SubmitPrompt,
        scroll: 0,
    };
    let area = Rect::new(0, 0, 120, 40);
    let layout = help_modal_rects(area, None).expect("help layout");

    // act
    let rendered = crate::render_test::render_to_string(&app, area, |app, frame, _| {
        render_detail(frame, app, app.theme(), layout, Action::SubmitPrompt, 0);
    });

    // assert
    assert!(rendered.contains("Submit the current prompt"), "{rendered}");
}
