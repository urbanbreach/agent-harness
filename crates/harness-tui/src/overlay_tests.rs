use super::*;

#[test]
fn test_from_state_empty() {
    let state = OverlayState::default();
    let stack = OverlayStack::from_state(state);
    assert_eq!(stack.ordered(), &[]);
}

#[test]
fn test_command_palette_channel_visible_fields() {
    let visible_states = [
        OverlayState {
            palette_visible: true,
            ..Default::default()
        },
        OverlayState {
            session_history_visible: true,
            ..Default::default()
        },
        OverlayState {
            model_switcher_visible: true,
            ..Default::default()
        },
        OverlayState {
            toggles_menu_visible: true,
            ..Default::default()
        },
        OverlayState {
            lineage_browser_visible: true,
            ..Default::default()
        },
        OverlayState {
            fork_selector_visible: true,
            ..Default::default()
        },
    ];

    for state in visible_states {
        assert!(state.command_palette_channel_visible());
    }

    let unrelated_state = OverlayState {
        details_drawer_open: true,
        slash_visible: true,
        file_mention_visible: true,
        status_dialog_visible: true,
        permission_pending: true,
        ..Default::default()
    };
    assert!(!unrelated_state.command_palette_channel_visible());
}

#[test]
fn test_from_state_all_independent() {
    let state = OverlayState {
        details_drawer_open: true,
        slash_visible: true,
        file_mention_visible: true,
        status_dialog_visible: true,
        ..Default::default()
    };
    let stack = OverlayStack::from_state(state);
    assert_eq!(
        stack.ordered(),
        &[
            OverlayKind::DetailsDrawer,
            OverlayKind::SlashCommands,
            OverlayKind::FileMentions,
            OverlayKind::StatusDialog,
        ]
    );
}

#[test]
fn test_from_state_permission_pending_overrides() {
    let state = OverlayState {
        details_drawer_open: true,
        slash_visible: true,
        file_mention_visible: true,
        palette_visible: true,
        status_dialog_visible: true,
        permission_pending: true,
        ..Default::default()
    };
    let stack = OverlayStack::from_state(state);
    assert_eq!(
        stack.ordered(),
        &[OverlayKind::DetailsDrawer, OverlayKind::PermissionModal,]
    );
}

#[test]
fn test_from_state_command_palette_hierarchy() {
    let mut state = OverlayState {
        palette_visible: true,
        toggles_menu_visible: true,
        lineage_browser_visible: true,
        fork_selector_visible: true,
        ..Default::default()
    };
    assert_eq!(
        OverlayStack::from_state(state).ordered(),
        &[OverlayKind::TogglesMenu]
    );

    state.toggles_menu_visible = false;
    assert_eq!(
        OverlayStack::from_state(state).ordered(),
        &[OverlayKind::LineageBrowser]
    );

    state.lineage_browser_visible = false;
    assert_eq!(
        OverlayStack::from_state(state).ordered(),
        &[OverlayKind::ForkSelector]
    );

    state.fork_selector_visible = false;
    assert_eq!(
        OverlayStack::from_state(state).ordered(),
        &[OverlayKind::CommandPalette]
    );

    state.palette_visible = false;
    state.session_history_visible = true;
    assert_eq!(
        OverlayStack::from_state(state).ordered(),
        &[OverlayKind::CommandPalette]
    );

    state.session_history_visible = false;
    state.model_switcher_visible = true;
    assert_eq!(
        OverlayStack::from_state(state).ordered(),
        &[OverlayKind::CommandPalette]
    );
}

#[test]
fn test_command_palette_channel_emits_single_entry_when_all_set() {
    let state = OverlayState {
        palette_visible: true,
        session_history_visible: true,
        model_switcher_visible: true,
        toggles_menu_visible: true,
        lineage_browser_visible: true,
        fork_selector_visible: true,
        ..Default::default()
    };
    let stack = OverlayStack::from_state(state);
    let palette_channel_entries = stack
        .ordered()
        .iter()
        .filter(|kind| {
            matches!(
                kind,
                OverlayKind::CommandPalette
                    | OverlayKind::TogglesMenu
                    | OverlayKind::LineageBrowser
                    | OverlayKind::ForkSelector
            )
        })
        .count();
    assert_eq!(
        palette_channel_entries, 1,
        "command-palette channel must emit at most one entry even when every channel flag is set"
    );
}

#[test]
fn test_theme_dialog_and_error_details_are_independent_channels() {
    let state = OverlayState {
        theme_dialog_visible: true,
        error_details_visible: true,
        ..Default::default()
    };
    let stack = OverlayStack::from_state(state);
    assert_eq!(
        stack.ordered(),
        &[OverlayKind::ThemeDialog, OverlayKind::ErrorDetails,]
    );
}

#[test]
fn test_theme_dialog_and_error_details_coexist_with_status_dialog() {
    let state = OverlayState {
        status_dialog_visible: true,
        theme_dialog_visible: true,
        error_details_visible: true,
        ..Default::default()
    };
    let stack = OverlayStack::from_state(state);
    assert_eq!(
        stack.ordered(),
        &[
            OverlayKind::StatusDialog,
            OverlayKind::ThemeDialog,
            OverlayKind::ErrorDetails,
        ]
    );
}

#[test]
fn test_permission_pending_preempts_all_other_overlays() {
    let state = OverlayState {
        details_drawer_open: true,
        slash_visible: true,
        file_mention_visible: true,
        palette_visible: true,
        session_history_visible: true,
        model_switcher_visible: true,
        toggles_menu_visible: true,
        lineage_browser_visible: true,
        fork_selector_visible: true,
        status_dialog_visible: true,
        theme_dialog_visible: true,
        error_details_visible: true,
        prompt_stash_list_visible: true,
        permission_pending: true,
    };
    let stack = OverlayStack::from_state(state);
    assert_eq!(
        stack.ordered(),
        &[OverlayKind::DetailsDrawer, OverlayKind::PermissionModal]
    );
    assert_eq!(stack.top(), Some(OverlayKind::PermissionModal));
}

#[test]
fn test_top_returns_single_focus_owner() {
    let state = OverlayState {
        status_dialog_visible: true,
        theme_dialog_visible: true,
        error_details_visible: true,
        prompt_stash_list_visible: true,
        ..Default::default()
    };
    let stack = OverlayStack::from_state(state);
    let top = stack.top();
    assert_eq!(top, Some(OverlayKind::PromptStashList));
    assert_eq!(
        stack
            .ordered()
            .iter()
            .filter(|kind| **kind == top.unwrap())
            .count(),
        1,
        "top overlay must appear exactly once in the ordered stack"
    );
}
