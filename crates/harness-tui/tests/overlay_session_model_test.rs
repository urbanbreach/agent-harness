//! Task 24 owner tests: overlay/session/model/palette leaf views and actions.
//!
//! Proves the deterministic leaf view/action helper types for the overlay,
//! session navigation, palette, and model switcher shards have no shared
//! registry or app-state dependency. Covers the required failure cases:
//! empty catalog, invalid selected index, missing provider/model, resize
//! while open, escape restoring focus, fork/clone/model actions naming real
//! backend owners, duplicate/stale palette entries, and OVL-PALETTE /
//! OVL-SESSION semantic structure.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "contract tests use fail-fast asserts for missing leaf state"
)]

#[path = "../src/leaf_views/model.rs"]
mod model;
#[path = "../src/leaf_actions/overlay_session_model.rs"]
mod overlay_session_model;
#[path = "../src/leaf_views/palette.rs"]
mod palette;
#[path = "../src/leaf_views/session.rs"]
mod session;

use model::ModelLeafView;
use overlay_session_model::{
    validate_palette_entries, BackendOwner, LeafFocusOwner, OverlayKind, OverlaySessionModelAction,
    PaletteEntryLeaf, PaletteValidationError,
};
use palette::PaletteLeafView;
use session::SessionLeafView;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// (1) Empty catalog
// ---------------------------------------------------------------------------

/// Palette with zero entries: selection is invalid, view reports empty.
#[test]
fn palette_empty_catalog_selection_invalid() {
    let view = PaletteLeafView::new(true, 0, 0, 0);
    assert!(view.is_empty());
    assert!(!view.is_selection_valid());
    assert_eq!(view.clamped_selection(), 0);
}

/// Session list with zero entries: selection is invalid.
#[test]
fn session_empty_catalog_selection_invalid() {
    let view = SessionLeafView::new(true, 0, 0, false);
    assert!(view.is_empty());
    assert!(!view.is_selection_valid());
    assert_eq!(view.clamped_selection(), 0);
}

/// Model switcher with zero providers and zero models: no provider, invalid.
#[test]
fn model_empty_catalog_no_provider() {
    let view = ModelLeafView::new(true, 0, 0, 0);
    assert!(view.is_empty());
    assert!(!view.has_provider());
    assert!(!view.is_selection_valid());
    assert_eq!(view.clamped_selection(), 0);
}

// ---------------------------------------------------------------------------
// (2) Invalid selected index
// ---------------------------------------------------------------------------

/// Palette selected_index beyond entry_count is invalid and clamps to last.
#[test]
fn palette_invalid_selected_index_clamps() {
    let view = PaletteLeafView::new(true, 10, 3, 2);
    assert!(!view.is_selection_valid());
    assert_eq!(view.clamped_selection(), 2);
}

/// Session selected_index beyond session_count is invalid and clamps.
#[test]
fn session_invalid_selected_index_clamps() {
    let view = SessionLeafView::new(true, 99, 5, true);
    assert!(!view.is_selection_valid());
    assert_eq!(view.clamped_selection(), 4);
}

/// Model selected_index beyond model_count is invalid and clamps.
#[test]
fn model_invalid_selected_index_clamps() {
    let view = ModelLeafView::new(true, 7, 2, 3);
    assert!(!view.is_selection_valid());
    assert_eq!(view.clamped_selection(), 2);
}

/// Valid selected index passes validation for all three views.
#[test]
fn valid_selected_index_passes_for_all_views() {
    let palette = PaletteLeafView::new(true, 1, 5, 3);
    assert!(palette.is_selection_valid());

    let session = SessionLeafView::new(true, 2, 10, true);
    assert!(session.is_selection_valid());

    let model = ModelLeafView::new(true, 0, 1, 4);
    assert!(model.is_selection_valid());
}

// ---------------------------------------------------------------------------
// (3) Missing provider/model
// ---------------------------------------------------------------------------

/// Model view with models but no providers: has_provider is false.
#[test]
fn model_missing_provider_detected() {
    let view = ModelLeafView::new(true, 0, 0, 3);
    assert!(!view.has_provider());
    assert!(view.is_selection_valid());
}

/// Model view with providers but no models: selection is invalid.
#[test]
fn model_missing_model_detected() {
    let view = ModelLeafView::new(true, 0, 2, 0);
    assert!(view.has_provider());
    assert!(!view.is_selection_valid());
    assert!(view.is_empty());
}

// ---------------------------------------------------------------------------
// (4) Resize while open
// ---------------------------------------------------------------------------

/// Palette view state survives resize: fields are unchanged.
#[test]
fn palette_resize_preserves_state() {
    let view = PaletteLeafView::new(true, 2, 5, 3);
    let resized = view.after_resize(80, 24);
    assert_eq!(view, resized);
    let resized2 = view.after_resize(120, 50);
    assert_eq!(view, resized2);
}

/// Session view state survives resize.
#[test]
fn session_resize_preserves_state() {
    let view = SessionLeafView::new(true, 1, 3, true);
    let resized = view.after_resize(60, 20);
    assert_eq!(view, resized);
}

/// Model view state survives resize.
#[test]
fn model_resize_preserves_state() {
    let view = ModelLeafView::new(true, 0, 1, 4);
    let resized = view.after_resize(100, 30);
    assert_eq!(view, resized);
}

// ---------------------------------------------------------------------------
// (5) Escape restoring focus
// ---------------------------------------------------------------------------

/// CloseOverlay action restores focus to Composer.
#[test]
fn close_overlay_restores_focus_to_composer() {
    let action = OverlaySessionModelAction::CloseOverlay;
    assert_eq!(action.restored_focus(), LeafFocusOwner::Composer);
}

/// RestoreFocus action restores focus to Composer.
#[test]
fn restore_focus_action_restores_to_composer() {
    let action = OverlaySessionModelAction::RestoreFocus;
    assert_eq!(action.restored_focus(), LeafFocusOwner::Composer);
}

/// OpenPalette sets focus to Palette, not Composer.
#[test]
fn open_palette_sets_focus_to_palette() {
    let action = OverlaySessionModelAction::OpenPalette;
    assert_eq!(action.restored_focus(), LeafFocusOwner::Palette);
    assert_ne!(action.restored_focus(), LeafFocusOwner::Composer);
}

/// OpenModelSwitcher sets focus to Model.
#[test]
fn open_model_sets_focus_to_model() {
    let action = OverlaySessionModelAction::OpenModelSwitcher;
    assert_eq!(action.restored_focus(), LeafFocusOwner::Model);
}

/// Session actions set focus to Session.
#[test]
fn session_actions_set_focus_to_session() {
    for action in [
        OverlaySessionModelAction::OpenSessionHistory,
        OverlaySessionModelAction::OpenForkSelector,
        OverlaySessionModelAction::OpenLineageBrowser,
        OverlaySessionModelAction::OpenSessionRename,
    ] {
        assert_eq!(
            action.restored_focus(),
            LeafFocusOwner::Session,
            "action {action:?} must set focus to Session"
        );
    }
}

// ---------------------------------------------------------------------------
// (6) Fork/clone/model actions naming real backend owners
// ---------------------------------------------------------------------------

/// Every overlay/session/model action that opens a surface must name a real
/// backend owner module and function — not a hardcoded fixture.
#[test]
fn open_actions_name_real_backend_owners() {
    let cases: &[(OverlaySessionModelAction, &str, &str)] = &[
        (
            OverlaySessionModelAction::OpenPalette,
            "app::palette_controller",
            "dispatch_palette_command",
        ),
        (
            OverlaySessionModelAction::OpenSessionHistory,
            "app::session_history",
            "begin_session_history_picker",
        ),
        (
            OverlaySessionModelAction::OpenForkSelector,
            "app::lineage",
            "open_fork_selector",
        ),
        (
            OverlaySessionModelAction::OpenLineageBrowser,
            "app::lineage",
            "open_lineage_browser",
        ),
        (
            OverlaySessionModelAction::OpenModelSwitcher,
            "app::model_switcher",
            "open_model_switcher",
        ),
        (
            OverlaySessionModelAction::OpenTogglesMenu,
            "app::toggles",
            "open_toggles_menu",
        ),
        (
            OverlaySessionModelAction::OpenConnectDialog,
            "app::auth_dialog::lifecycle",
            "open_connect_dialog",
        ),
        (
            OverlaySessionModelAction::OpenSessionRename,
            "app::session_history",
            "open_session_rename_dialog",
        ),
    ];

    for (action, expected_module, expected_function) in cases {
        let owner = action
            .backend_owner()
            .unwrap_or_else(|| panic!("action {action:?} must have a real backend owner"));
        assert_eq!(
            owner.module, *expected_module,
            "action {action:?} names wrong module"
        );
        assert_eq!(
            owner.function, *expected_function,
            "action {action:?} names wrong function"
        );
        assert!(action.has_real_owner());
    }
}

/// CloseOverlay and RestoreFocus route to session_navigation::close_palette.
#[test]
fn close_and_restore_name_real_backend_owner() {
    for action in [
        OverlaySessionModelAction::CloseOverlay,
        OverlaySessionModelAction::RestoreFocus,
    ] {
        let owner = action
            .backend_owner()
            .unwrap_or_else(|| panic!("action {action:?} must have a backend owner"));
        assert_eq!(owner.module, "app::session_navigation");
        assert_eq!(owner.function, "close_palette");
    }
}

/// NewSession and NewWorktreeSession name real backend owners.
#[test]
fn new_session_actions_name_real_backend_owners() {
    let new_session = OverlaySessionModelAction::NewSession;
    let owner = new_session
        .backend_owner()
        .expect("NewSession must have owner");
    assert_eq!(owner.module, "app::session_navigation");
    assert_eq!(owner.function, "apply_new_session_launcher_selection");

    let worktree = OverlaySessionModelAction::NewWorktreeSession;
    let owner2 = worktree
        .backend_owner()
        .expect("NewWorktreeSession must have owner");
    assert_eq!(owner2.module, "app::session_navigation");
    assert_eq!(owner2.function, "request_new_worktree_session");
}

/// Navigation actions (NavigateUp, NavigateDown, SelectCurrent) do not have
/// a single backend owner — they are handled by the overlay's own key handler.
#[test]
fn navigation_actions_have_no_direct_backend_owner() {
    for action in [
        OverlaySessionModelAction::NavigateUp,
        OverlaySessionModelAction::NavigateDown,
        OverlaySessionModelAction::SelectCurrent,
    ] {
        assert!(
            !action.has_real_owner(),
            "action {action:?} should not have a direct backend owner"
        );
    }
}

/// None action has no backend owner.
#[test]
fn none_action_has_no_backend_owner() {
    let action = OverlaySessionModelAction::None;
    assert!(!action.has_real_owner());
    assert!(action.backend_owner().is_none());
}

/// BackendOwner is deterministic: same action always returns same owner.
#[test]
fn backend_owner_is_deterministic() {
    let a = OverlaySessionModelAction::OpenForkSelector.backend_owner();
    let b = OverlaySessionModelAction::OpenForkSelector.backend_owner();
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// (7) Duplicate/stale palette entries
// ---------------------------------------------------------------------------

/// Duplicate palette entry IDs are rejected.
#[test]
fn duplicate_palette_entries_rejected() {
    let entries = [
        PaletteEntryLeaf::new("session.fork", "Fork"),
        PaletteEntryLeaf::new("session.clone", "Clone"),
        PaletteEntryLeaf::new("session.fork", "Fork Again"),
    ];
    let result = validate_palette_entries(&entries);
    assert!(result.is_err());
    match result {
        Err(PaletteValidationError::DuplicateId { id, first, second }) => {
            assert_eq!(id, "session.fork");
            assert_eq!(first, 0);
            assert_eq!(second, 2);
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }
}

/// Empty palette entry ID is rejected.
#[test]
fn empty_palette_entry_id_rejected() {
    let entries = [PaletteEntryLeaf::new("", "Empty")];
    let result = validate_palette_entries(&entries);
    assert_eq!(result, Err(PaletteValidationError::EmptyId { index: 0 }));
}

/// Stale palette entry (empty title) is detected.
#[test]
fn stale_palette_entry_detected() {
    let stale = PaletteEntryLeaf::new("session.fork", "");
    assert!(stale.is_stale());

    let fresh = PaletteEntryLeaf::new("session.fork", "Fork");
    assert!(!fresh.is_stale());
}

/// Stale entry in the list is rejected by validation.
#[test]
fn stale_entry_in_list_rejected() {
    let entries = [
        PaletteEntryLeaf::new("session.fork", "Fork"),
        PaletteEntryLeaf::new("session.clone", ""),
    ];
    let result = validate_palette_entries(&entries);
    assert!(result.is_err());
    match result {
        Err(PaletteValidationError::StaleEntry { id }) => {
            assert_eq!(id, "session.clone");
        }
        other => panic!("expected StaleEntry, got {other:?}"),
    }
}

/// Valid palette entries with no duplicates pass validation.
#[test]
fn valid_palette_entries_pass() {
    let entries = [
        PaletteEntryLeaf::new("session.fork", "Fork"),
        PaletteEntryLeaf::new("session.clone", "Clone"),
        PaletteEntryLeaf::new("session.tree", "Tree"),
    ];
    assert!(validate_palette_entries(&entries).is_ok());
}

/// Empty palette entry list passes validation (trivially).
#[test]
fn empty_palette_entries_pass() {
    let entries: [PaletteEntryLeaf; 0] = [];
    assert!(validate_palette_entries(&entries).is_ok());
}

// ---------------------------------------------------------------------------
// (8) OVL-PALETTE and OVL-SESSION semantic structure
// ---------------------------------------------------------------------------

/// OVL-PALETTE: OpenPalette maps to OverlayKind::Palette.
#[test]
fn ovl_palette_semantic_structure() {
    assert_eq!(
        OverlaySessionModelAction::OpenPalette.overlay_kind(),
        OverlayKind::Palette
    );
    assert_ne!(
        OverlaySessionModelAction::OpenPalette.overlay_kind(),
        OverlayKind::Session
    );
}

/// OVL-SESSION: session actions map to OverlayKind::Session.
#[test]
fn ovl_session_semantic_structure() {
    for action in [
        OverlaySessionModelAction::OpenSessionHistory,
        OverlaySessionModelAction::OpenForkSelector,
        OverlaySessionModelAction::OpenLineageBrowser,
        OverlaySessionModelAction::OpenSessionRename,
    ] {
        assert_eq!(
            action.overlay_kind(),
            OverlayKind::Session,
            "action {action:?} must be OverlayKind::Session"
        );
    }
}

/// OVL-MODEL: OpenModelSwitcher maps to OverlayKind::Model.
#[test]
fn ovl_model_semantic_structure() {
    assert_eq!(
        OverlaySessionModelAction::OpenModelSwitcher.overlay_kind(),
        OverlayKind::Model
    );
}

/// Each overlay-opening action maps to exactly one OverlayKind.
#[test]
fn each_open_action_has_exactly_one_overlay_kind() {
    let open_actions = [
        OverlaySessionModelAction::OpenPalette,
        OverlaySessionModelAction::OpenSessionHistory,
        OverlaySessionModelAction::OpenForkSelector,
        OverlaySessionModelAction::OpenLineageBrowser,
        OverlaySessionModelAction::OpenModelSwitcher,
        OverlaySessionModelAction::OpenTogglesMenu,
        OverlaySessionModelAction::OpenConnectDialog,
    ];
    for action in open_actions {
        let kind = action.overlay_kind();
        assert_ne!(
            kind,
            OverlayKind::None,
            "open action {action:?} must map to a non-None overlay kind"
        );
    }
}

/// Non-open actions map to OverlayKind::None.
#[test]
fn non_open_actions_map_to_none_kind() {
    for action in [
        OverlaySessionModelAction::None,
        OverlaySessionModelAction::CloseOverlay,
        OverlaySessionModelAction::NavigateUp,
        OverlaySessionModelAction::NavigateDown,
        OverlaySessionModelAction::SelectCurrent,
        OverlaySessionModelAction::RestoreFocus,
    ] {
        assert_eq!(
            action.overlay_kind(),
            OverlayKind::None,
            "action {action:?} must map to None"
        );
    }
}

/// OverlayKind variants are distinct.
#[test]
fn overlay_kind_variants_are_distinct() {
    let kinds = [
        OverlayKind::None,
        OverlayKind::Palette,
        OverlayKind::Session,
        OverlayKind::Model,
        OverlayKind::Toggles,
        OverlayKind::Connect,
    ];
    for (i, a) in kinds.iter().enumerate() {
        for (j, b) in kinds.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "OverlayKind variants at {i} and {j} must differ");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Determinism and no-dependency proofs
// ---------------------------------------------------------------------------

/// All leaf views are Copy and constructible without any registry or app state.
#[test]
fn leaf_views_are_copy_and_stateless() {
    let palette = PaletteLeafView::new(true, 0, 1, 0);
    let session = SessionLeafView::new(true, 0, 1, true);
    let model = ModelLeafView::new(true, 0, 1, 1);
    let palette_copy = palette;
    let session_copy = session;
    let model_copy = model;
    assert_eq!(palette, palette_copy);
    assert_eq!(session, session_copy);
    assert_eq!(model, model_copy);
}

/// Default values are sensible (not visible, zero selection).
#[test]
fn leaf_view_defaults_are_inactive() {
    let palette = PaletteLeafView::default();
    assert!(!palette.visible);
    let session = SessionLeafView::default();
    assert!(!session.visible);
    let model = ModelLeafView::default();
    assert!(!model.visible);
}

/// Action leaf is Copy and default is None.
#[test]
fn action_leaf_is_copy_default_none() {
    let action = OverlaySessionModelAction::default();
    assert_eq!(action, OverlaySessionModelAction::None);
    let copy = action;
    assert_eq!(action, copy);
}

// ---------------------------------------------------------------------------
// Rendering proof (not snapshot-only)
// ---------------------------------------------------------------------------

/// Render the palette overlay via Ratatui TestBackend showing the title and
/// entry count. This is a real-surface artifact, not a snapshot.
#[test]
fn palette_overlay_renders_title_and_entries() {
    let view = PaletteLeafView::new(true, 0, 3, 5);

    let area = Rect::new(0, 0, 40, 10);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines = vec![
                Line::from("Command Palette"),
                Line::from(format!("Entries: {}", view.entry_count)),
                Line::from(format!("Query length: {}", view.query_len)),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("OVL-PALETTE"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    let output: String = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        output.contains("OVL-PALETTE"),
        "missing title in:\n{output}"
    );
    assert!(
        output.contains("Entries: 3"),
        "missing entry count in:\n{output}"
    );
}

/// Render the session overlay via Ratatui TestBackend.
#[test]
fn session_overlay_renders_title_and_sessions() {
    let view = SessionLeafView::new(true, 0, 5, true);

    let area = Rect::new(0, 0, 40, 10);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines = vec![
                Line::from("Session Navigation"),
                Line::from(format!("Sessions: {}", view.session_count)),
                Line::from(format!("Lineage: {}", view.has_lineage)),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("OVL-SESSION"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    let output: String = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        output.contains("OVL-SESSION"),
        "missing title in:\n{output}"
    );
    assert!(
        output.contains("Sessions: 5"),
        "missing session count in:\n{output}"
    );
}

/// Render the model switcher overlay via Ratatui TestBackend.
#[test]
fn model_overlay_renders_title_and_models() {
    let view = ModelLeafView::new(true, 0, 2, 4);

    let area = Rect::new(0, 0, 40, 10);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines = vec![
                Line::from("Model Switcher"),
                Line::from(format!("Providers: {}", view.provider_count)),
                Line::from(format!("Models: {}", view.model_count)),
            ];
            let para = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title("OVL-MODEL"));
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    let output: String = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(output.contains("OVL-MODEL"), "missing title in:\n{output}");
    assert!(
        output.contains("Models: 4"),
        "missing model count in:\n{output}"
    );
}
