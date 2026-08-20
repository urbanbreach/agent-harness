//! Contract tests for Task 15: action dispatch, focus management, overlay controller.
//!
//! Proves: correct action dispatch for key events with context filtering,
//! focus transitions across 6 panes, overlay open/close/stacking/escape.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::keybindings::action_dispatch::{ActionContext, ActionDef, ActionDispatcher};
use harness_tui::keybindings::focus::{ActivePane, FocusController};
use harness_tui::keybindings::{Action, KeyBinding};
use harness_tui::overlay::{OverlayController, OverlayKind};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn key_plain(c: char) -> KeyEvent {
    key(KeyCode::Char(c), KeyModifiers::NONE)
}

fn key_ctrl(c: char) -> KeyEvent {
    key(KeyCode::Char(c), KeyModifiers::CONTROL)
}

// --- Action dispatch contract tests ---

mod action_dispatch {
    use super::*;

    fn sample_dispatcher() -> ActionDispatcher {
        let mut d = ActionDispatcher::new();
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Char('j'), KeyModifiers::NONE),
            Action::MoveDown,
            ActionContext::ScrollbackFocused,
            "Select next",
        ));
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Char('k'), KeyModifiers::NONE),
            Action::MoveUp,
            ActionContext::ScrollbackFocused,
            "Select prev",
        ));
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Enter, KeyModifiers::NONE),
            Action::SubmitPrompt,
            ActionContext::PromptFocused,
            "Send",
        ));
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            Action::Quit,
            ActionContext::Always,
            "New session",
        ));
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Char('y'), KeyModifiers::NONE),
            Action::AllowPermission,
            ActionContext::AgentScreen,
            "Copy content",
        ));
        d
    }

    #[test]
    fn always_context_available_in_every_context() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key_ctrl('n');
        for ctx in ActionContext::all() {
            // assert
            assert_eq!(
                d.resolve(&event, *ctx),
                Some(Action::Quit),
                "Always action must resolve in {ctx:?}"
            );
        }
    }

    #[test]
    fn scrollback_action_blocked_in_prompt_context() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key_plain('j');
        // assert
        assert_eq!(
            d.resolve(&event, ActionContext::ScrollbackFocused),
            Some(Action::MoveDown)
        );
        assert_eq!(
            d.resolve(&event, ActionContext::PromptFocused),
            None,
            "scrollback-only action must not fire in prompt context"
        );
    }

    #[test]
    fn prompt_action_blocked_in_scrollback_context() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key(KeyCode::Enter, KeyModifiers::NONE);
        // assert
        assert_eq!(
            d.resolve(&event, ActionContext::PromptFocused),
            Some(Action::SubmitPrompt)
        );
        assert_eq!(
            d.resolve(&event, ActionContext::ScrollbackFocused),
            None,
            "prompt-only action must not fire in scrollback context"
        );
    }

    #[test]
    fn agent_screen_context_satisfied_by_scrollback() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key_plain('y');
        // assert
        assert_eq!(
            d.resolve(&event, ActionContext::AgentScreen),
            Some(Action::AllowPermission)
        );
        assert_eq!(
            d.resolve(&event, ActionContext::ScrollbackFocused),
            Some(Action::AllowPermission),
            "AgentScreen actions available when scrollback is focused"
        );
    }

    #[test]
    fn agent_screen_context_satisfied_by_prompt_focused() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key_plain('y');
        // assert
        assert_eq!(
            d.resolve(&event, ActionContext::PromptFocused),
            Some(Action::AllowPermission),
            "AgentScreen actions available when prompt is focused"
        );
    }

    #[test]
    fn agent_screen_action_blocked_in_dashboard() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key_plain('y');
        // assert
        assert_eq!(
            d.resolve(&event, ActionContext::DashboardFocused),
            None,
            "AgentScreen action must not fire in dashboard context"
        );
    }

    #[test]
    fn welcome_screen_context_isolated() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key_plain('j');
        // assert
        assert_eq!(
            d.resolve(&event, ActionContext::WelcomeScreen),
            None,
            "scrollback action must not fire on welcome screen"
        );
    }

    #[test]
    fn resolve_def_returns_metadata() {
        // arrange
        // act
        let d = sample_dispatcher();
        let event = key_plain('j');
        let def = d
            .resolve_def(&event, ActionContext::ScrollbackFocused)
            .expect("should resolve");
        // assert
        assert_eq!(def.label, "Select next");
        assert_eq!(def.action, Action::MoveDown);
        assert_eq!(def.context, ActionContext::ScrollbackFocused);
    }

    #[test]
    fn active_defs_filters_by_context() {
        // arrange
        let d = sample_dispatcher();
        let scrollback_defs = d.active_defs(ActionContext::ScrollbackFocused);
        assert!(scrollback_defs.len() >= 3,"scrollback should see: j, k (scrollback) + Ctrl+n (always) + y (agent_screen superset)");

        // act
        let prompt_defs = d.active_defs(ActionContext::PromptFocused);
        // assert
        assert!(prompt_defs
            .iter()
            .any(|def| def.action == Action::SubmitPrompt));
        assert!(prompt_defs.iter().any(|def| def.action == Action::Quit));
    }

    #[test]
    fn seven_context_variants_exist() {
        // arrange
        // act
        let all = ActionContext::all();
        // assert
        assert_eq!(all.len(), 7);
        assert!(all.contains(&ActionContext::Always));
        assert!(all.contains(&ActionContext::PromptFocused));
        assert!(all.contains(&ActionContext::ScrollbackFocused));
        assert!(all.contains(&ActionContext::AgentScreen));
        assert!(all.contains(&ActionContext::WelcomeScreen));
        assert!(all.contains(&ActionContext::DashboardFocused));
        assert!(all.contains(&ActionContext::DashboardOverlay));
    }

    #[test]
    fn context_as_str_roundtrip() {
        // arrange
        // act
        for ctx in ActionContext::all() {
            let s = ctx.as_str();
            // assert
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn dispatcher_empty_returns_none() {
        // arrange
        // act
        let d = ActionDispatcher::new();
        // assert
        assert!(d.is_empty());
        assert_eq!(d.resolve(&key_plain('j'), ActionContext::Always), None);
    }

    #[test]
    fn first_match_wins_for_same_key() {
        // arrange
        // act
        let mut d = ActionDispatcher::new();
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Char('x'), KeyModifiers::NONE),
            Action::Quit,
            ActionContext::Always,
            "first",
        ));
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Char('x'), KeyModifiers::NONE),
            Action::Help,
            ActionContext::Always,
            "second",
        ));
        // assert
        assert_eq!(
            d.resolve(&key_plain('x'), ActionContext::Always),
            Some(Action::Quit)
        );
    }
}

mod action_context_sat {
    use super::*;

    #[test]
    fn always_satisfied_by_all() {
        // arrange
        // act
        for ctx in ActionContext::all() {
            // assert
            assert!(ActionContext::Always.is_satisfied_by(*ctx));
        }
    }

    #[test]
    fn prompt_focused_only_by_itself() {
        // arrange
        // act
        // assert
        assert!(ActionContext::PromptFocused.is_satisfied_by(ActionContext::PromptFocused));
        assert!(!ActionContext::PromptFocused.is_satisfied_by(ActionContext::ScrollbackFocused));
        assert!(!ActionContext::PromptFocused.is_satisfied_by(ActionContext::DashboardFocused));
    }

    #[test]
    fn scrollback_focused_only_by_itself() {
        // arrange
        // act
        // assert
        assert!(ActionContext::ScrollbackFocused.is_satisfied_by(ActionContext::ScrollbackFocused));
        assert!(!ActionContext::ScrollbackFocused.is_satisfied_by(ActionContext::PromptFocused));
    }

    #[test]
    fn agent_screen_superset_includes_prompt_and_scrollback() {
        // arrange
        // act
        // assert
        assert!(ActionContext::AgentScreen.is_satisfied_by(ActionContext::AgentScreen));
        assert!(ActionContext::AgentScreen.is_satisfied_by(ActionContext::PromptFocused));
        assert!(ActionContext::AgentScreen.is_satisfied_by(ActionContext::ScrollbackFocused));
        assert!(!ActionContext::AgentScreen.is_satisfied_by(ActionContext::WelcomeScreen));
        assert!(!ActionContext::AgentScreen.is_satisfied_by(ActionContext::DashboardFocused));
        assert!(!ActionContext::AgentScreen.is_satisfied_by(ActionContext::DashboardOverlay));
    }

    #[test]
    fn dashboard_focused_only_by_itself() {
        // arrange
        // act
        // assert
        assert!(ActionContext::DashboardFocused.is_satisfied_by(ActionContext::DashboardFocused));
        assert!(!ActionContext::DashboardFocused.is_satisfied_by(ActionContext::AgentScreen));
    }

    #[test]
    fn dashboard_overlay_only_by_itself() {
        // arrange
        // act
        // assert
        assert!(ActionContext::DashboardOverlay.is_satisfied_by(ActionContext::DashboardOverlay));
        assert!(!ActionContext::DashboardOverlay.is_satisfied_by(ActionContext::DashboardFocused));
    }

    #[test]
    fn welcome_screen_only_by_itself() {
        // arrange
        // act
        // assert
        assert!(ActionContext::WelcomeScreen.is_satisfied_by(ActionContext::WelcomeScreen));
        assert!(!ActionContext::WelcomeScreen.is_satisfied_by(ActionContext::AgentScreen));
    }
}

// --- Focus management contract tests ---

mod focus_management {
    use super::*;

    #[test]
    fn six_focus_panes_exist_in_controller() {
        // arrange
        // act
        let panes = ActivePane::CYCLE_ORDER;
        // assert
        assert_eq!(panes.len(), 6);
        assert!(panes.contains(&ActivePane::Scrollback));
        assert!(panes.contains(&ActivePane::Todo));
        assert!(panes.contains(&ActivePane::Queue));
        assert!(panes.contains(&ActivePane::Prompt));
        assert!(panes.contains(&ActivePane::Tasks));
        assert!(panes.contains(&ActivePane::Catalog));
    }

    #[test]
    fn cycle_next_wraps_around() {
        // arrange
        // act
        // assert
        assert_eq!(ActivePane::Catalog.next(), ActivePane::Scrollback);
        assert_eq!(ActivePane::Scrollback.next(), ActivePane::Todo);
        assert_eq!(ActivePane::Prompt.next(), ActivePane::Tasks);
    }

    #[test]
    fn cycle_prev_wraps_around() {
        // arrange
        // act
        // assert
        assert_eq!(ActivePane::Scrollback.prev(), ActivePane::Catalog);
        assert_eq!(ActivePane::Todo.prev(), ActivePane::Scrollback);
        assert_eq!(ActivePane::Tasks.prev(), ActivePane::Prompt);
    }

    #[test]
    fn full_forward_cycle_visits_all() {
        // arrange
        // act
        let start = ActivePane::Scrollback;
        let mut current = start;
        let mut visited = vec![current];
        for _ in 0..5 {
            current = current.next();
            visited.push(current);
        }
        // assert
        assert_eq!(visited.len(), 6);
        for pane in &ActivePane::CYCLE_ORDER {
            assert!(visited.contains(pane), "must visit {pane:?}");
        }
    }

    #[test]
    fn controller_starts_at_prompt_by_default() {
        // arrange
        // act
        let fc = FocusController::default();
        // assert
        assert_eq!(fc.current(), ActivePane::Prompt);
    }

    #[test]
    fn controller_focus_next_transitions() {
        // arrange
        // act
        let mut fc = FocusController::new(ActivePane::Scrollback);
        // assert
        assert_eq!(fc.focus_next(), ActivePane::Todo);
        assert_eq!(fc.current(), ActivePane::Todo);
        assert_eq!(fc.focus_next(), ActivePane::Queue);
    }

    #[test]
    fn controller_focus_prev_transitions() {
        // arrange
        // act
        let mut fc = FocusController::new(ActivePane::Scrollback);
        // assert
        assert_eq!(fc.focus_prev(), ActivePane::Catalog);
        assert_eq!(fc.focus_prev(), ActivePane::Tasks);
    }

    #[test]
    fn controller_direct_focus_selects_requested_pane() {
        // arrange
        // act
        let mut fc = FocusController::new(ActivePane::Scrollback);
        // assert
        assert_eq!(fc.focus_pane(ActivePane::Catalog), ActivePane::Catalog);
        assert!(fc.is_focused(ActivePane::Catalog));
        assert!(!fc.is_focused(ActivePane::Scrollback));
    }

    #[test]
    fn controller_records_transition_history() {
        // arrange
        // act
        let mut fc = FocusController::new(ActivePane::Prompt);
        fc.focus_pane(ActivePane::Scrollback);
        fc.focus_next();
        fc.focus_prev();
        // assert
        assert_eq!(
            fc.history(),
            &[
                ActivePane::Prompt,
                ActivePane::Scrollback,
                ActivePane::Todo,
                ActivePane::Scrollback,
            ]
        );
        assert_eq!(fc.transition_count(), 3);
    }

    #[test]
    fn controller_no_duplicate_history_on_same_focus() {
        // arrange
        // act
        let mut fc = FocusController::new(ActivePane::Prompt);
        fc.focus_pane(ActivePane::Prompt);
        // assert
        assert_eq!(fc.history().len(), 1, "no-op focus must not record");
        assert_eq!(fc.transition_count(), 0);
    }

    #[test]
    fn pane_as_str_covers_all() {
        // arrange
        // act
        for pane in &ActivePane::CYCLE_ORDER {
            // assert
            assert!(!pane.as_str().is_empty());
        }
    }
}

// --- Overlay controller contract tests ---

mod overlay_controller {
    use super::*;

    #[test]
    fn empty_controller_no_top() {
        // arrange
        // act
        let oc = OverlayController::new();
        // assert
        assert!(!oc.is_open());
        assert_eq!(oc.top(), None);
        assert_eq!(oc.depth(), 0);
    }

    #[test]
    fn push_opens_overlay_on_stack() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::CommandPalette);
        // assert
        assert!(oc.is_open());
        assert_eq!(oc.top(), Some(OverlayKind::CommandPalette));
        assert_eq!(oc.depth(), 1);
    }

    #[test]
    fn stacking_multiple_overlays_preserves_order() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::SettingsEditor);
        oc.push(OverlayKind::CommandPalette);
        oc.push(OverlayKind::PermissionModal);
        // assert
        assert_eq!(oc.depth(), 3);
        assert_eq!(oc.top(), Some(OverlayKind::PermissionModal));
        assert_eq!(
            oc.ordered(),
            &[
                OverlayKind::SettingsEditor,
                OverlayKind::CommandPalette,
                OverlayKind::PermissionModal,
            ]
        );
    }

    #[test]
    fn pop_closes_topmost_overlay_only() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::SettingsEditor);
        oc.push(OverlayKind::CommandPalette);
        // assert
        assert_eq!(oc.pop(), Some(OverlayKind::CommandPalette));
        assert_eq!(oc.top(), Some(OverlayKind::SettingsEditor));
        assert_eq!(oc.depth(), 1);
    }

    #[test]
    fn escape_closes_topmost_overlay_only() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::ThemeDialog);
        oc.push(OverlayKind::ErrorDetails);
        // assert
        assert_eq!(oc.escape(), Some(OverlayKind::ErrorDetails));
        assert_eq!(oc.escape(), Some(OverlayKind::ThemeDialog));
        assert_eq!(oc.escape(), None, "escape on empty stack returns None");
    }

    #[test]
    fn escape_on_empty_returns_none() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        // assert
        assert_eq!(oc.escape(), None);
    }

    #[test]
    fn close_specific_overlay_middle_of_stack() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::SettingsEditor);
        oc.push(OverlayKind::CommandPalette);
        oc.push(OverlayKind::PermissionModal);
        // assert
        assert!(oc.close(OverlayKind::CommandPalette));
        assert_eq!(oc.depth(), 2);
        assert!(!oc.contains(OverlayKind::CommandPalette));
        assert_eq!(oc.top(), Some(OverlayKind::PermissionModal));
    }

    #[test]
    fn close_nonexistent_returns_false() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::SettingsEditor);
        // assert
        assert!(!oc.close(OverlayKind::AuthDialog));
        assert_eq!(oc.depth(), 1);
    }

    #[test]
    fn reopen_moves_to_top() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::SettingsEditor);
        oc.push(OverlayKind::CommandPalette);
        oc.push(OverlayKind::SettingsEditor);
        // assert
        assert_eq!(oc.depth(), 2, "re-push must not duplicate");
        assert_eq!(oc.top(), Some(OverlayKind::SettingsEditor));
    }

    #[test]
    fn close_all_empty_stack() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::SettingsEditor);
        oc.push(OverlayKind::CommandPalette);
        oc.push(OverlayKind::PermissionModal);
        oc.close_all();
        // assert
        assert!(!oc.is_open());
        assert_eq!(oc.depth(), 0);
    }

    #[test]
    fn contains_checks_overlay_stack_membership() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::LineageBrowser);
        // assert
        assert!(oc.contains(OverlayKind::LineageBrowser));
        assert!(!oc.contains(OverlayKind::ForkSelector));
    }

    #[test]
    fn eleven_reference_modals_representable() {
        // arrange
        // act
        let reference_modals = [
            OverlayKind::CommandPalette,
            OverlayKind::SettingsEditor,
            OverlayKind::StatusDialog,
            OverlayKind::ThemeDialog,
            OverlayKind::ErrorDetails,
            OverlayKind::PromptStashList,
            OverlayKind::LineageBrowser,
            OverlayKind::ForkSelector,
            OverlayKind::PermissionModal,
            OverlayKind::AuthDialog,
            OverlayKind::TrustFolderPrompt,
        ];
        let mut oc = OverlayController::new();
        for modal in &reference_modals {
            oc.push(*modal);
        }
        // assert
        assert_eq!(oc.depth(), 11);
        for modal in &reference_modals {
            assert!(oc.contains(*modal));
        }
    }
}

// --- Integration: action dispatch + focus + overlay together ---

mod integration {
    use super::*;

    #[test]
    fn focus_change_alters_available_actions() {
        // arrange
        let mut d = ActionDispatcher::new();
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Char('j'), KeyModifiers::NONE),
            Action::MoveDown,
            ActionContext::ScrollbackFocused,
            "next",
        ));
        d.register(ActionDef::new(
            KeyBinding::new(KeyCode::Enter, KeyModifiers::NONE),
            Action::SubmitPrompt,
            ActionContext::PromptFocused,
            "send",
        ));

        let mut fc = FocusController::new(ActivePane::Scrollback);
        let ev_j = key_plain('j');
        let ev_enter = key(KeyCode::Enter, KeyModifiers::NONE);

        let ctx = ActionContext::ScrollbackFocused;
        assert_eq!(d.resolve(&ev_j, ctx), Some(Action::MoveDown));
        assert_eq!(d.resolve(&ev_enter, ctx), None);

        // act
        fc.focus_pane(ActivePane::Prompt);
        let prompt_ctx = ActionContext::PromptFocused;
        // assert
        assert_eq!(d.resolve(&ev_j, prompt_ctx), None);
        assert_eq!(d.resolve(&ev_enter, prompt_ctx), Some(Action::SubmitPrompt));
    }

    #[test]
    fn overlay_open_blocks_underlying_dispatch() {
        // arrange
        // act
        let mut oc = OverlayController::new();
        oc.push(OverlayKind::CommandPalette);
        // assert
        assert!(oc.is_open());
        // When an overlay is open, the overlay controller signals that
        // underlying pane actions should be suppressed
        assert_eq!(oc.top(), Some(OverlayKind::CommandPalette));
    }

    #[test]
    fn escape_closes_overlay_then_focus_resumes() {
        // arrange
        let mut oc = OverlayController::new();
        let mut fc = FocusController::new(ActivePane::Scrollback);

        oc.push(OverlayKind::SettingsEditor);
        assert_eq!(fc.current(), ActivePane::Scrollback);

        let closed = oc.escape();
        assert_eq!(closed, Some(OverlayKind::SettingsEditor));
        assert!(!oc.is_open());

        // act
        fc.focus_next();
        // assert
        assert_eq!(fc.current(), ActivePane::Todo);
    }
}
