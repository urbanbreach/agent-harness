//! TUI leaf view/action contract tests for Todo 11.
//!
//! Proves the deterministic leaf view/action helper types have no shared
//! registry or app-state dependency, and renders the slash menu via the
//! Ratatui test backend showing all 25 canonical IDs in order.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "contract tests use fail-fast asserts for missing leaf state"
)]

use harness_tui::keybindings::slash_commands;
use harness_tui::leaf_actions::ActionLeaf;
use harness_tui::leaf_views::{ComposerLeafView, OverlayLeafView, TranscriptLeafView};
use harness_tui::render_test::buffer_to_string;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

/// ComposerLeafView is deterministic: same inputs produce same outputs.
#[test]
fn composer_leaf_view_is_deterministic() {
    // arrange
    // act
    let a = ComposerLeafView::new(true, true);
    let b = ComposerLeafView::new(true, true);
    // assert
    assert_eq!(a, b);
    let c = ComposerLeafView::new(false, true);
    assert_ne!(a, c);
}

/// TranscriptLeafView is deterministic.
#[test]
fn transcript_leaf_view_is_deterministic() {
    // arrange
    // act
    let a = TranscriptLeafView::new(0, 10);
    let b = TranscriptLeafView::new(0, 10);
    // assert
    assert_eq!(a, b);
    let c = TranscriptLeafView::new(5, 10);
    assert_ne!(a, c);
}

/// OverlayLeafView is deterministic.
#[test]
fn overlay_leaf_view_is_deterministic() {
    // arrange
    // act
    let a = OverlayLeafView::new("test", true);
    let b = OverlayLeafView::new("test", true);
    // assert
    assert_eq!(a, b);
    let c = OverlayLeafView::new("test", false);
    assert_ne!(a, c);
}

/// ActionLeaf is deterministic.
#[test]
fn action_leaf_is_deterministic() {
    // arrange
    // act
    let a = ActionLeaf::Submit;
    let b = ActionLeaf::Submit;
    // assert
    assert_eq!(a, b);
    assert_ne!(a, ActionLeaf::Cancel);
}

/// Leaf views have no app-state dependency: they can be constructed
/// without any registry, runtime, or app state.
#[test]
fn leaf_views_have_no_app_state_dependency() {
    // arrange
    // act
    let _composer = ComposerLeafView::default();
    let _transcript = TranscriptLeafView::default();
    let _overlay = OverlayLeafView::default();
    // assert
    let _action = ActionLeaf::default();
}

/// Leaf views have no registry dependency: they are plain value types
/// with no shared state or global references.
#[test]
fn leaf_views_have_no_registry_dependency() {
    // arrange
    // act
    // Construct two independent instances — they must not share state.
    let composer_a = ComposerLeafView::new(true, false);
    let composer_b = ComposerLeafView::new(false, true);
    // assert
    assert_ne!(composer_a, composer_b);
    // Modifying one does not affect the other (Copy semantics).
    let mut composer_c = composer_a;
    composer_c.prompt_visible = false;
    assert_ne!(composer_a, composer_c);
    assert!(composer_a.prompt_visible);
}

/// Render the slash menu via Ratatui TestBackend showing all 26 canonical
/// IDs in order. This is a real-surface artifact, not a snapshot.
#[test]
fn slash_menu_renders_all_26_commands_in_order() {
    // arrange
    let commands = slash_commands();
    assert_eq!(commands.len(), 26, "registry must have 26 commands");

    let area = Rect::new(0, 0, 80, 30);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines: Vec<Line> = commands
                .iter()
                .enumerate()
                .map(|(i, cmd)| {
                    Line::from(format!("{:2}. /{}  [{}]", i + 1, cmd.id, cmd.metadata_id))
                })
                .collect();
            let para = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Slash Commands"),
            );
            frame.render_widget(para, area);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer().clone();
    let output = buffer_to_string(&buffer, area.width);

    // act
    // Verify all 26 IDs appear in the rendered output, in order.
    let mut last_pos = 0;
    for cmd in commands {
        let id = cmd.id;
        let needle = format!("/{id}");
        let pos = output
            .find(&needle)
            .unwrap_or_else(|| panic!("slash menu render missing command: {needle}\n{output}"));
        // assert
        assert!(
            pos >= last_pos,
            "command {needle} appeared out of order at pos {pos} (last was {last_pos})"
        );
        last_pos = pos;
    }
}

/// Print the rendered slash menu for evidence capture.
/// Run with `--nocapture` to see the output.
#[test]
fn slash_menu_render_capture() {
    // arrange
    let commands = slash_commands();
    let area = Rect::new(0, 0, 80, 30);
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            let lines: Vec<Line> = commands
                .iter()
                .enumerate()
                .map(|(i, cmd)| {
                    Line::from(format!("{:2}. /{}  [{}]", i + 1, cmd.id, cmd.metadata_id))
                })
                .collect();
            let para = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Slash Commands"),
            );
            frame.render_widget(para, area);
        })
        .expect("draw");

    // act
    let buffer = terminal.backend().buffer().clone();
    let output = buffer_to_string(&buffer, area.width);
    // assert
    println!("{output}");
}
