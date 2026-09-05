use super::*;

#[path = "subagent_inline_exit_tests.rs"]
mod subagent_inline_exit_tests;
#[path = "subagent_navigation_keyboard_tests.rs"]
mod subagent_navigation_keyboard_tests;
#[path = "subagent_task_general_navigation_tests.rs"]
mod subagent_task_general_navigation_tests;
#[path = "subagent_task_inline_navigation_tests.rs"]
mod subagent_task_inline_navigation_tests;
#[path = "subagent_task_metadata_navigation_tests.rs"]
mod subagent_task_metadata_navigation_tests;

pub(super) use subagent_inline_exit_tests::slash_exit_from_inline_subagent_restores_parent_before_quit;
pub(super) use subagent_navigation_keyboard_tests::{
    disk_backed_child_navigation_stays_in_live_tui_stack as keyboard_disk_backed_child_navigation_stays_in_live_tui_stack,
    keyboard_sidebar_subagent_selection_opens_child_session as keyboard_keyboard_sidebar_subagent_selection_opens_child_session,
    live_subagent_hitbox_uses_rendered_transcript_area as keyboard_live_subagent_hitbox_uses_rendered_transcript_area,
};
pub(super) use subagent_task_general_navigation_tests::mouse_up_on_completed_general_task_row_opens_child_session;
pub(super) use subagent_task_inline_navigation_tests::{
    mouse_click_on_task_inline_row_opens_subagent_session as keyboard_mouse_click_on_task_inline_row_opens_subagent_session,
    mouse_click_on_task_inline_row_uses_task_row_child_session,
};
pub(super) use subagent_task_metadata_navigation_tests::mouse_click_on_task_row_uses_harness_session_metadata;

#[test]
fn subagent_navigation_task_row_click_opens_child_directly() {
    subagent_task_inline_navigation_tests::mouse_click_on_task_inline_row_uses_task_row_child_session();
}
