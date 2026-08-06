#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner contract tests use direct fail-fast assertions"
)]

use harness_tui::completion_controller::{
    choose_preferred_trigger, insert_completion, CompletionController, CompletionGeometryInput,
    CompletionItem, CompletionRange, CompletionSource, CompletionStatus, CompletionTrigger,
    ShellCompletionGeometry,
};
use harness_tui::composer_atoms::{
    AtomBuffer, AtomKind, AttachmentId, ComposerAtom, GraphemeCluster,
};
use harness_tui::composer_editing::ComposerEditor;
use harness_tui::shell_geometry::ShellState;
use ratatui::layout::Rect;

fn trigger(source: CompletionSource, start: usize, end: usize, query: &str) -> CompletionTrigger {
    CompletionTrigger::new(CompletionRange::new(start, end).unwrap(), query, source)
}

fn loaded_controller() -> CompletionController {
    let mut controller = CompletionController::new();
    let request = controller.begin(trigger(CompletionSource::Slash, 0, 2, "/mo"));
    controller
        .apply_results(
            &request,
            vec![
                CompletionItem::new(1, "model", "model"),
                CompletionItem::new(2, "mode", "mode"),
            ],
        )
        .unwrap();
    controller
}

#[test]
fn overlapping_triggers_use_documented_source_precedence() {
    let slash = trigger(CompletionSource::Slash, 0, 2, "/");
    let file = trigger(CompletionSource::File, 1, 3, "~/");
    let shell = trigger(CompletionSource::Shell, 1, 3, "~");
    let history = trigger(CompletionSource::History, 1, 3, "~");

    let selected = choose_preferred_trigger(&[file, shell, history, slash]);

    assert_eq!(
        selected.map(|value| value.source),
        Some(CompletionSource::Slash)
    );
}

#[test]
fn stale_async_results_are_rejected_after_a_new_query() {
    let mut controller = CompletionController::new();
    let first = controller.begin(trigger(CompletionSource::File, 0, 2, "~/"));
    let second = controller.begin(trigger(CompletionSource::File, 0, 3, "~/s"));

    let stale = controller.apply_results(&first, vec![CompletionItem::new(1, "old", "old")]);
    assert!(stale.is_err());

    controller
        .apply_results(&second, vec![CompletionItem::new(2, "src", "src")])
        .unwrap();
    assert_eq!(controller.status(), CompletionStatus::Ready);
}

#[test]
fn mouse_and_keyboard_acceptance_return_the_same_event() {
    let keyboard = loaded_controller();
    let mut mouse = loaded_controller();

    let keyboard_event = keyboard.accept_keyboard().unwrap();
    let mouse_event = mouse.accept_mouse(0).unwrap();

    assert_eq!(keyboard_event, mouse_event);
}

#[test]
fn resize_repositions_dropdown_inside_the_wrapped_composer() {
    let editor = ComposerEditor::from_text("one two three four five");
    let wide = ShellCompletionGeometry::calculate(&CompletionGeometryInput {
        viewport: Rect::new(0, 0, 80, 24),
        state: ShellState::Drafting,
        buffer: editor.buffer(),
        cursor: editor.cursor(),
        item_count: 4,
        max_rows: 5,
    });
    let narrow = ShellCompletionGeometry::calculate(&CompletionGeometryInput {
        viewport: Rect::new(0, 0, 40, 10),
        state: ShellState::Drafting,
        buffer: editor.buffer(),
        cursor: editor.cursor(),
        item_count: 4,
        max_rows: 5,
    });

    assert!(wide.rect.right() <= 80 && wide.rect.bottom() <= 24);
    assert!(narrow.rect.right() <= 40 && narrow.rect.bottom() <= 10);
    assert_ne!(wide.rect, narrow.rect);
    assert!(narrow.wrapped_lines >= wide.wrapped_lines);
}

#[test]
fn completion_insertion_preserves_attachment_identity() {
    let buffer = AtomBuffer::from_atoms(vec![
        ComposerAtom::attachment(1, AttachmentId::new(77)),
        ComposerAtom::text(2, GraphemeCluster::new("x")),
    ])
    .unwrap();
    let editor = ComposerEditor::from_buffer(buffer);
    let replacement = trigger(CompletionSource::File, 1, 2, "x");

    let inserted = insert_completion(&editor, &replacement, "é").unwrap();

    assert_eq!(inserted.buffer().atoms()[0].id.get(), 1);
    assert!(matches!(
        inserted.buffer().atoms()[0].kind,
        AtomKind::Attachment(id) if id == AttachmentId::new(77)
    ));
    assert_eq!(inserted.text(), "[attachment:77]é");
}

#[test]
fn empty_query_and_no_results_have_distinct_states() {
    let mut empty = CompletionController::new();
    let empty_request = empty.begin(trigger(CompletionSource::History, 0, 0, ""));
    empty.apply_results(&empty_request, Vec::new()).unwrap();

    let mut no_results = CompletionController::new();
    let no_result_request = no_results.begin(trigger(CompletionSource::Shell, 0, 1, "z"));
    no_results
        .apply_results(&no_result_request, Vec::new())
        .unwrap();

    assert_eq!(empty.status(), CompletionStatus::Empty);
    assert_eq!(no_results.status(), CompletionStatus::NoResults);
}
