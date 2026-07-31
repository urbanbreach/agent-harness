//! Evidence layer owners for the TUI overlay surfaces behind two capability rows:
//! - `memory.tui_palette` (palette `/memory` -> memory browser overlay)
//! - `worktree.tui_select_switch` (palette `/worktree` -> worktree picker overlay)
//!
//! Layers covered here (grok-build-parity-loop-contract.md §5.2):
//! - L0 reference/inventory crosswalk: palette command id -> dispatch action ownership.
//! - L1 state/action + semantic terminal-cell result: action opens overlay, cells render.
//! - L3 input trace + error/cancellation/recovery: key events drive the overlay state.
//! - L4 settled pixel / fixed-tick comparison: settled render is byte-stable across draws.
//!
//! Every overlay is rendered full-bleed at 80x24 so the panel covers the shell and the
//! captured cells are the overlay surface alone. Entries are injected directly so the
//! assertions are deterministic and independent of the host workspace or git state.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::worktree::ListedWorktree;
use harness_tui::app::{AppState, LaunchMetadata, MemoryBrowserEntry};
use harness_tui::keybindings::palette_model::{self, PaletteDispatch};
use harness_tui::keybindings::Action;
use harness_tui::overlay::OverlayKind;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn mk_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_ref("worker", "mock:mem-wt-test"));
    app
}

fn render_overlay(app: &AppState) -> String {
    render_text(app, 80, 24)
}

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn mem_entry(id: &str, label: &str) -> MemoryBrowserEntry {
    MemoryBrowserEntry::new(id, label)
}

fn worktree(path: &str, branch: &str, slug: &str) -> ListedWorktree {
    ListedWorktree {
        path: PathBuf::from(path),
        branch: Some(branch.to_string()),
        head: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
        bare: false,
        detached: false,
        harness_managed: true,
        slug: Some(slug.to_string()),
    }
}

// ---------- L0: reference / inventory crosswalk ----------

#[test]
fn l0_palette_crosswalk_pins_memory_and_worktree_ownership() {
    // arrange / act
    let memory =
        palette_model::find("context.memory").expect("palette command `context.memory` must exist");
    let worktree = palette_model::find("worktree.switch")
        .expect("palette command `worktree.switch` must exist");

    // assert: each palette id maps to its owning overlay action.
    assert!(
        matches!(
            &memory.dispatch,
            PaletteDispatch::Action(Action::OpenMemoryBrowser)
        ),
        "context.memory must dispatch to Action::OpenMemoryBrowser, got {:?}",
        memory.dispatch
    );
    assert!(
        matches!(
            worktree.dispatch,
            PaletteDispatch::Action(Action::OpenWorktreePicker)
        ),
        "worktree.switch must dispatch to Action::OpenWorktreePicker, got {:?}",
        worktree.dispatch
    );
}

// ---------- L1: state/action + semantic terminal-cell result ----------

#[test]
fn l1_memory_browser_action_state_and_terminal_cells() {
    // arrange
    let mut app = mk_app();

    // act: the action surfaces the overlay.
    app.open_memory_browser();
    app.memory_browser.entries = vec![
        mem_entry("zephyr-key-a", "zephyr-memory-alpha"),
        mem_entry("zephyr-key-b", "zephyr-memory-beta"),
    ];
    app.memory_browser.selected = 0;

    let rendered = render_overlay(&app);

    // assert: action -> state.
    assert!(app.memory_browser.visible, "memory browser must be visible");
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::MemoryBrowser),
        "memory browser must be the topmost overlay"
    );

    // assert: semantic terminal cells.
    assert!(
        rendered.contains("Memory"),
        "title cell missing\n{rendered}"
    );
    assert!(
        rendered.contains("esc"),
        "esc hint cell missing\n{rendered}"
    );
    assert!(
        rendered.contains("zephyr-memory-alpha"),
        "selected entry label cell missing\n{rendered}"
    );
    assert!(
        rendered.contains("zephyr-memory-beta"),
        "second entry label cell missing\n{rendered}"
    );
}

#[test]
fn l1_worktree_picker_action_state_and_terminal_cells() {
    // arrange
    let mut app = mk_app();

    // act: the action surfaces the overlay.
    app.open_worktree_picker();
    app.worktree_picker.error = None;
    app.worktree_picker.entries = vec![
        worktree("/tmp/wt/zt-primary", "main", "zt-primary"),
        worktree("/tmp/wt/zt-feature", "feature", "zt-feature"),
    ];
    app.worktree_picker.selected = 0;

    let rendered = render_overlay(&app);

    // assert: action -> state.
    assert!(
        app.worktree_picker.visible,
        "worktree picker must be visible"
    );
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::WorktreePicker),
        "worktree picker must be the topmost overlay"
    );

    // assert: semantic terminal cells (rows render the worktree slug).
    assert!(
        rendered.contains("Switch worktree"),
        "title cell missing\n{rendered}"
    );
    assert!(
        rendered.contains("esc"),
        "esc hint cell missing\n{rendered}"
    );
    assert!(
        rendered.contains("zt-primary"),
        "selected slug cell missing\n{rendered}"
    );
    assert!(
        rendered.contains("zt-feature"),
        "second slug cell missing\n{rendered}"
    );
}

// ---------- L3: input trace + error / cancellation / recovery ----------

#[test]
fn l3_memory_browser_input_trace_navigate_cancel_recover() {
    // arrange
    let mut app = mk_app();
    app.open_memory_browser();
    app.memory_browser.entries = vec![
        mem_entry("a", "entry-alpha"),
        mem_entry("b", "entry-beta"),
        mem_entry("c", "entry-gamma"),
    ];

    // act + assert: input trace drives the selection.
    assert_eq!(app.memory_browser.selected, 0);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.memory_browser.selected, 1);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.memory_browser.selected, 2);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.memory_browser.selected, 1);
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.memory_browser.selected, 2);

    // act + assert: Esc cancels the overlay.
    app.handle_key(key(KeyCode::Esc));
    assert!(
        !app.memory_browser.visible,
        "Esc must close the memory browser"
    );

    // act + assert: recovery by reopening and navigating again.
    app.open_memory_browser();
    assert!(
        app.memory_browser.visible,
        "reopen must restore the overlay"
    );
    assert_eq!(
        app.memory_browser.selected, 0,
        "reopen must reset selection"
    );
}

#[test]
fn l3_worktree_picker_input_trace_error_cancel_confirm_safe() {
    // arrange
    let mut app = mk_app();
    app.open_worktree_picker();
    app.worktree_picker.error = None;
    app.worktree_picker.entries = vec![
        worktree("/tmp/wt/zt-primary", "main", "zt-primary"),
        worktree("/tmp/wt/zt-feature", "feature", "zt-feature"),
    ];
    app.worktree_picker.selected = 0;

    // act + assert: input trace drives the selection.
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.worktree_picker.selected, 1);
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.worktree_picker.selected, 0);

    // act + assert: error path renders the surfaced reason without panicking.
    app.worktree_picker.error =
        Some("No workspace root available for worktree listing".to_string());
    let error_rendered = render_overlay(&app);
    assert!(
        error_rendered.contains("No workspace root"),
        "error reason cell missing\n{error_rendered}"
    );

    // act + assert: Esc cancels the overlay (clears the error).
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.worktree_picker.visible, "Esc must close the picker");
    assert!(
        app.worktree_picker.error.is_none(),
        "close must clear the error"
    );

    // act + assert: Enter with no selectable worktree is a safe no-op (no panic, stays open).
    app.worktree_picker.visible = true;
    app.worktree_picker.error = None;
    app.worktree_picker.entries.clear();
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.worktree_picker.visible,
        "confirm with nothing selected must not crash or auto-close"
    );
}

// ---------- L4: settled pixel / fixed-tick comparison ----------

#[test]
fn l4_memory_browser_settled_render_is_pixel_stable() {
    // arrange: two independently constructed apps in an identical settled state.
    let mut first = mk_app();
    first.open_memory_browser();
    first.memory_browser.entries = vec![mem_entry("k", "settled-memory-row")];

    let mut second = mk_app();
    second.open_memory_browser();
    second.memory_browser.entries = vec![mem_entry("k", "settled-memory-row")];

    // act: fixed-geometry settled draws across two construction ticks.
    let first_render = render_overlay(&first);
    let second_render = render_overlay(&second);

    // assert: the settled pixel grid is reproducible across fixed ticks.
    assert_eq!(
        first_render, second_render,
        "settled memory browser render must be byte-stable across fixed-tick draws"
    );
    assert!(
        first_render.contains("settled-memory-row"),
        "settled render must show the entry\n{first_render}"
    );
}

#[test]
fn l4_worktree_picker_settled_render_is_pixel_stable() {
    // arrange: two independently constructed apps in an identical settled state.
    let mut first = mk_app();
    first.open_worktree_picker();
    first.worktree_picker.error = None;
    first.worktree_picker.entries = vec![worktree("/tmp/wt/zt-settled", "main", "zt-settled")];

    let mut second = mk_app();
    second.open_worktree_picker();
    second.worktree_picker.error = None;
    second.worktree_picker.entries = vec![worktree("/tmp/wt/zt-settled", "main", "zt-settled")];

    // act: fixed-geometry settled draws across two construction ticks.
    let first_render = render_overlay(&first);
    let second_render = render_overlay(&second);

    // assert: the settled pixel grid is reproducible across fixed ticks.
    assert_eq!(
        first_render, second_render,
        "settled worktree picker render must be byte-stable across fixed-tick draws"
    );
    assert!(
        first_render.contains("zt-settled"),
        "settled render must show the worktree slug\n{first_render}"
    );
}
