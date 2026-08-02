//! Task 25 differential TDD: startup, welcome, trust, and first-prompt journeys.
//!
//! Contract: bordered welcome panel at 120x32 + responsive viewports
//! (60x20, 80x24, 100x30), Harness identity/title/version, changelog/local
//! notices, new/resume/worktree actions, folder-trust prompt grant/deny,
//! first-prompt composer focus, type-to-dismiss preserving typed text + cursor,
//! Escape/focus-CSI stability, resize-during-dismiss, missing-auth/config
//! fallback.
//!
//! RED phase: tests written FIRST to define the contract.
//! GREEN phase: implementation in ui_lifecycle.rs + app/{welcome,trust_prompt,first_prompt}.rs.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::keybindings::Action;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn startup_app() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app
}

fn idle_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app
}

fn render_at(app: &AppState, w: u16, h: u16) -> String {
    render_to_string(app, Rect::new(0, 0, w, h), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn render(app: &AppState) -> String {
    render_at(app, 120, 32)
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

fn trust_prompt_app() -> AppState {
    let mut app = startup_app();
    app.trust_folder_prompt_visible = true;
    app
}

// ---------------------------------------------------------------------------
// 1. Bordered welcome panel at 120x32 + responsive viewports
// ---------------------------------------------------------------------------

mod bordered_welcome_panel {
    use super::*;

    #[test]
    fn welcome_panel_has_rounded_borders_at_120x32() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains('\u{256d}') && rendered.contains('\u{2570}'),
            "welcome panel must use rounded borders at 120x32\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_has_rounded_borders_at_100x30() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 100, 30);
        // assert
        assert!(
            rendered.contains('\u{256d}') && rendered.contains('\u{2570}'),
            "welcome panel must use rounded borders at 100x30\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_drops_border_at_80x24() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 80, 24);
        let welcome_borders = count_char(&rendered, '\u{256d}');
        // assert: only the composer border remains, not the welcome panel border
        assert_eq!(
            welcome_borders, 1,
            "compact startup at 80x24 must drop welcome panel border, keep only composer\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_drops_border_at_60x20() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 60, 20);
        let welcome_borders = count_char(&rendered, '\u{256d}');
        // assert: only the composer border remains
        assert_eq!(
            welcome_borders, 1,
            "compact startup at 60x20 must drop welcome panel border, keep only composer\n{rendered}"
        );
    }

    #[test]
    fn welcome_content_visible_at_all_viewports() {
        // arrange
        let app = startup_app();
        for (w, h) in [(120u16, 32u16), (100, 30), (80, 24), (60, 20)] {
            // act
            let rendered = render_at(&app, w, h);
            // assert
            assert!(
                rendered.contains("Harness") || rendered.contains("Changelog"),
                "welcome content must be visible at {w}x{h}\n{rendered}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Harness identity/title/version
// ---------------------------------------------------------------------------

mod identity_title_version {
    use super::*;

    #[test]
    fn welcome_panel_contains_harness_title() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("Harness"),
            "welcome panel must contain Harness title\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_contains_version_string() {
        // arrange
        let app = startup_app();
        let version = env!("CARGO_PKG_VERSION");
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains(version),
            "welcome panel must contain version {version}\n{rendered}"
        );
    }

    #[test]
    fn welcome_identity_visible_at_100x30() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 100, 30);
        // assert
        assert!(
            rendered.contains("Harness"),
            "Harness identity must be visible at 100x30\n{rendered}"
        );
    }

    #[test]
    fn welcome_identity_visible_at_80x24() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 80, 24);
        // assert: compact welcome may not show the full title, but should show
        // at least one identity marker (Harness or changelog content)
        assert!(
            rendered.contains("Harness") || rendered.contains("Changelog"),
            "identity or changelog must be visible at 80x24\n{rendered}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Changelog/local notices
// ---------------------------------------------------------------------------

mod changelog_and_local_notices {
    use super::*;

    #[test]
    fn welcome_panel_contains_changelog_section() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("Changelog"),
            "welcome panel must contain Changelog section\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_contains_changelog_bullets() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains('\u{2022}'),
            "welcome panel must contain changelog bullets\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_contains_local_notices_section() {
        // arrange: startup app with no provider connected
        let mut app = AppState::new_startup(Vec::new(), None);
        app.maybe_set_no_provider_banner();
        // act
        let rendered = render(&app);
        // assert: local notices section should appear when there are environment notices
        assert!(
            rendered.contains("Notices") || rendered.contains("notices"),
            "welcome panel must contain local notices section when provider is missing\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_local_notices_shows_missing_provider() {
        // arrange: startup app with no provider connected
        let mut app = AppState::new_startup(Vec::new(), None);
        app.maybe_set_no_provider_banner();
        // act
        let rendered = render(&app);
        // assert: local notices should mention the missing provider
        assert!(
            rendered.contains("No provider") || rendered.contains("provider"),
            "local notices must mention missing provider\n{rendered}"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. New/resume/worktree actions
// ---------------------------------------------------------------------------

mod welcome_actions {
    use super::*;

    #[test]
    fn welcome_panel_contains_new_worktree_action() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("New worktree"),
            "welcome panel must contain New worktree action\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_contains_resume_session_action() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("Resume session"),
            "welcome panel must contain Resume session action\n{rendered}"
        );
    }

    #[test]
    fn welcome_panel_contains_quit_action() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("Quit"),
            "welcome panel must contain Quit action\n{rendered}"
        );
    }

    #[test]
    fn welcome_actions_visible_at_100x30() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 100, 30);
        // assert
        assert!(
            rendered.contains("New worktree") || rendered.contains("Resume session"),
            "welcome actions must be visible at 100x30\n{rendered}"
        );
    }

    #[test]
    fn welcome_actions_compact_at_80x24() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 80, 24);
        // assert: compact welcome should still show at least some actions
        assert!(
            rendered.contains("New worktree") || rendered.contains("Resume session"),
            "compact welcome must show actions at 80x24\n{rendered}"
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Folder-trust prompt grant/deny
// ---------------------------------------------------------------------------

mod folder_trust_prompt {
    use super::*;

    #[test]
    fn trust_folder_prompt_renders_when_visible() {
        // arrange
        let app = trust_prompt_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("Folder Trust"),
            "trust folder prompt must render when visible\n{rendered}"
        );
    }

    #[test]
    fn trust_folder_prompt_shows_allow_label() {
        // arrange
        let app = trust_prompt_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("Allow") || rendered.contains("[y]"),
            "trust prompt must show Allow label\n{rendered}"
        );
    }

    #[test]
    fn trust_folder_prompt_shows_deny_label() {
        // arrange
        let app = trust_prompt_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("Deny") || rendered.contains("[n]"),
            "trust prompt must show Deny label\n{rendered}"
        );
    }

    #[test]
    fn trust_folder_prompt_dismissed_by_escape() {
        // arrange
        let mut app = trust_prompt_app();
        assert!(app.trust_folder_prompt_visible);
        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        ));
        // assert
        assert!(
            !app.trust_folder_prompt_visible,
            "Escape must dismiss trust folder prompt"
        );
    }

    #[test]
    fn trust_folder_prompt_not_visible_by_default() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            !rendered.contains("Folder Trust"),
            "trust folder prompt must not be visible by default\n{rendered}"
        );
    }

    #[test]
    fn trust_folder_prompt_renders_at_compact_viewport() {
        // arrange
        let app = trust_prompt_app();
        // act
        let rendered = render_at(&app, 80, 24);
        // assert
        assert!(
            rendered.contains("Folder Trust"),
            "trust folder prompt must render at 80x24\n{rendered}"
        );
    }
}

// ---------------------------------------------------------------------------
// 6. First-prompt composer focus
// ---------------------------------------------------------------------------

mod first_prompt_focus {
    use super::*;

    #[test]
    fn startup_composer_has_focus_glyph() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains('\u{276f}'),
            "startup composer must have focus glyph (\u{276f})\n{rendered}"
        );
    }

    #[test]
    fn startup_composer_focus_at_100x30() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 100, 30);
        // assert
        assert!(
            rendered.contains('\u{276f}'),
            "composer must have focus glyph at 100x30\n{rendered}"
        );
    }

    #[test]
    fn startup_composer_focus_at_80x24() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 80, 24);
        // assert
        assert!(
            rendered.contains('\u{276f}'),
            "composer must have focus glyph at 80x24\n{rendered}"
        );
    }

    #[test]
    fn startup_composer_focus_at_60x20() {
        // arrange
        let app = startup_app();
        // act
        let rendered = render_at(&app, 60, 20);
        // assert
        assert!(
            rendered.contains('\u{276f}'),
            "composer must have focus glyph at 60x20\n{rendered}"
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Type-to-dismiss preserving typed text + cursor
// ---------------------------------------------------------------------------

mod type_to_dismiss {
    use super::*;

    #[test]
    fn typing_dismisses_welcome_panel() {
        // arrange
        let mut app = startup_app();
        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('h'),
            crossterm::event::KeyModifiers::empty(),
        ));
        let rendered = render(&app);
        // assert
        assert!(
            !rendered.contains("New worktree"),
            "typing must dismiss welcome panel\n{rendered}"
        );
    }

    #[test]
    fn typing_preserves_text_in_composer() {
        // arrange
        let mut app = startup_app();
        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('h'),
            crossterm::event::KeyModifiers::empty(),
        ));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('i'),
            crossterm::event::KeyModifiers::empty(),
        ));
        let rendered = render(&app);
        // assert
        assert!(
            rendered.contains("hi"),
            "typed text must be preserved in composer after dismiss\n{rendered}"
        );
    }

    #[test]
    fn typing_preserves_cursor_position() {
        // arrange
        let mut app = startup_app();
        // act: type "abc"
        for ch in ['a', 'b', 'c'] {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::empty(),
            ));
        }
        // assert: cursor should be at position 3 (after "abc")
        assert_eq!(
            app.composer.prompt_buffer, "abc",
            "prompt buffer must contain typed text"
        );
        assert_eq!(
            app.composer.prompt_cursor, 3,
            "cursor must be at end of typed text"
        );
    }

    #[test]
    fn typing_single_char_dismisses_and_preserves() {
        // arrange
        let mut app = startup_app();
        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::empty(),
        ));
        // assert
        assert_eq!(
            app.composer.prompt_buffer, "x",
            "single char must be preserved"
        );
        assert_eq!(
            app.composer.prompt_cursor, 1,
            "cursor must be after the typed char"
        );
    }

    #[test]
    fn typing_unicode_preserves_text_and_cursor() {
        // arrange
        let mut app = startup_app();
        // act: type CJK characters
        for ch in ['\u{4f60}', '\u{597d}'] {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::empty(),
            ));
        }
        // assert
        assert_eq!(
            app.composer.prompt_buffer, "\u{4f60}\u{597d}",
            "CJK text must be preserved"
        );
        assert_eq!(
            app.composer.prompt_cursor, 2,
            "cursor must be at char position 2 (not byte position)"
        );
    }
}

// ---------------------------------------------------------------------------
// 8. Escape/focus-CSI stability
// ---------------------------------------------------------------------------

mod escape_focus_stability {
    use super::*;

    #[test]
    fn escape_during_welcome_does_not_crash() {
        // arrange
        let mut app = startup_app();
        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        ));
        let rendered = render(&app);
        // assert: app is still renderable
        assert!(
            rendered.contains("Harness") || rendered.contains("Changelog"),
            "app must remain stable after Escape during welcome\n{rendered}"
        );
    }

    #[test]
    fn escape_during_trust_prompt_dismisses_only() {
        // arrange
        let mut app = trust_prompt_app();
        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        ));
        let rendered = render(&app);
        // assert: trust prompt is dismissed, welcome panel is still visible
        assert!(
            !app.trust_folder_prompt_visible,
            "Escape must dismiss trust prompt"
        );
        assert!(
            !rendered.contains("Folder Trust"),
            "trust prompt must not render after Escape\n{rendered}"
        );
    }

    #[test]
    fn escape_does_not_corrupt_composer_state() {
        // arrange
        let mut app = startup_app();
        app.composer.prompt_buffer = "draft text".to_string();
        app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
        // act
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        ));
        // assert: composer state is preserved
        assert_eq!(
            app.composer.prompt_buffer, "draft text",
            "Escape must not corrupt composer buffer"
        );
        assert_eq!(
            app.composer.prompt_cursor, 10,
            "Escape must not corrupt composer cursor"
        );
    }

    #[test]
    fn repeated_escape_is_stable() {
        // arrange
        let mut app = trust_prompt_app();
        // act: press Escape multiple times
        for _ in 0..3 {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            ));
        }
        let rendered = render(&app);
        // assert: app is still renderable and stable
        assert!(
            !app.trust_folder_prompt_visible,
            "trust prompt must be dismissed after first Escape"
        );
        assert!(
            rendered.contains("Harness") || rendered.contains("Changelog"),
            "app must remain stable after repeated Escape\n{rendered}"
        );
    }
}

// ---------------------------------------------------------------------------
// 9. Resize-during-dismiss
// ---------------------------------------------------------------------------

mod resize_during_dismiss {
    use super::*;

    #[test]
    fn resize_after_typing_preserves_draft() {
        // arrange
        let mut app = startup_app();
        app.composer.prompt_buffer = "draft".to_string();
        app.composer.prompt_cursor = 5;
        // act: render at different sizes
        let rendered_120 = render_at(&app, 120, 32);
        let rendered_80 = render_at(&app, 80, 24);
        let rendered_60 = render_at(&app, 60, 20);
        // assert: draft text is preserved across resizes
        assert!(
            rendered_120.contains("draft"),
            "draft must be visible at 120x32\n{rendered_120}"
        );
        assert!(
            rendered_80.contains("draft"),
            "draft must be visible at 80x24\n{rendered_80}"
        );
        assert!(
            rendered_60.contains("draft"),
            "draft must be visible at 60x20\n{rendered_60}"
        );
    }

    #[test]
    fn resize_from_large_to_small_preserves_composer() {
        // arrange
        let app = idle_app();
        // act: render at large then small
        let rendered_large = render_at(&app, 120, 32);
        let rendered_small = render_at(&app, 60, 20);
        // assert: composer is visible at both sizes
        assert!(
            rendered_large.contains('\u{276f}'),
            "composer must be visible at 120x32\n{rendered_large}"
        );
        assert!(
            rendered_small.contains('\u{276f}'),
            "composer must be visible at 60x20 after resize\n{rendered_small}"
        );
    }

    #[test]
    fn resize_from_small_to_large_restores_welcome() {
        // arrange: startup app with empty prompt (welcome visible)
        let app = startup_app();
        // act: render at small then large
        let rendered_small = render_at(&app, 60, 20);
        let rendered_large = render_at(&app, 120, 32);
        // assert: welcome content is visible at both sizes
        assert!(
            rendered_small.contains("Harness") || rendered_small.contains("Changelog"),
            "welcome content must be visible at 60x20\n{rendered_small}"
        );
        assert!(
            rendered_large.contains("Harness"),
            "welcome identity must be visible at 120x32 after resize up\n{rendered_large}"
        );
    }

    #[test]
    fn resize_during_trust_prompt_preserves_overlay() {
        // arrange
        let app = trust_prompt_app();
        // act: render at different sizes
        let rendered_120 = render_at(&app, 120, 32);
        let rendered_80 = render_at(&app, 80, 24);
        // assert: trust prompt is visible at both sizes
        assert!(
            rendered_120.contains("Folder Trust"),
            "trust prompt must be visible at 120x32\n{rendered_120}"
        );
        assert!(
            rendered_80.contains("Folder Trust"),
            "trust prompt must be visible at 80x24 after resize\n{rendered_80}"
        );
    }
}

// ---------------------------------------------------------------------------
// 10. Missing-auth/config fallback
// ---------------------------------------------------------------------------

mod missing_auth_config_fallback {
    use super::*;

    #[test]
    fn missing_provider_shows_notice_in_welcome() {
        // arrange: startup app with no provider
        let mut app = AppState::new_startup(Vec::new(), None);
        app.maybe_set_no_provider_banner();
        // act
        let rendered = render(&app);
        // assert: welcome panel should show a notice about missing provider
        assert!(
            rendered.contains("No provider") || rendered.contains("provider"),
            "welcome must show missing-provider notice\n{rendered}"
        );
    }

    #[test]
    fn missing_provider_notice_visible_at_100x30() {
        // arrange
        let mut app = AppState::new_startup(Vec::new(), None);
        app.maybe_set_no_provider_banner();
        // act
        let rendered = render_at(&app, 100, 30);
        // assert
        assert!(
            rendered.contains("No provider") || rendered.contains("provider"),
            "missing-provider notice must be visible at 100x30\n{rendered}"
        );
    }

    #[test]
    fn missing_provider_notice_visible_at_80x24() {
        // arrange
        let mut app = AppState::new_startup(Vec::new(), None);
        app.maybe_set_no_provider_banner();
        // act
        let rendered = render_at(&app, 80, 24);
        // assert
        assert!(
            rendered.contains("No provider") || rendered.contains("provider"),
            "missing-provider notice must be visible at 80x24\n{rendered}"
        );
    }

    #[test]
    fn startup_without_launch_metadata_renders_without_crash() {
        // arrange: startup app with no launch metadata (no model, no mode label)
        let app = AppState::new_startup(Vec::new(), None);
        // act
        let rendered = render(&app);
        // assert: app renders without crash, welcome panel is visible
        assert!(
            rendered.contains("Harness") || rendered.contains("Changelog"),
            "startup without launch metadata must still render welcome\n{rendered}"
        );
    }

    #[test]
    fn startup_without_launch_metadata_shows_composer() {
        // arrange
        let app = AppState::new_startup(Vec::new(), None);
        // act
        let rendered = render(&app);
        // assert: composer is visible
        assert!(
            rendered.contains('\u{276f}'),
            "composer must be visible even without launch metadata\n{rendered}"
        );
    }
}
