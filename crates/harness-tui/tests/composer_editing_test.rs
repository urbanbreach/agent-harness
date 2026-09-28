#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner traces use fail-fast assertions for deterministic fixtures"
)]

use harness_tui::composer_atoms::{AtomKind, AttachmentId};
use harness_tui::composer_editing::{ComposerEditor, DeleteKind, MousePoint};

fn assert_deterministic<F>(trace: F)
where
    F: Fn() -> harness_tui::composer_editing::EditorState,
{
    let first = trace();
    let second = trace();
    assert_eq!(first, second);
}

#[test]
fn keyboard_word_and_line_movement_is_grapheme_safe_and_deterministic() {
    // arrange
    // act
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("alpha beta\n界🙂");
        editor.move_word_left();
        // assert
        assert_eq!(editor.cursor().insertion_index(), 11);
        editor.move_line_start();
        assert_eq!(editor.cursor().insertion_index(), 11);
        editor.state()
    });
}

#[test]
fn mouse_selection_keeps_structured_atoms_whole() {
    // arrange
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("a界b");
        editor.move_buffer_start();
        editor.move_right();
        editor
            .insert_attachment(AttachmentId::new(7))
            .expect("attachment insertion is valid");
        editor
            .begin_mouse_selection(MousePoint::new(0, 0), 10)
            .expect("mouse anchor is on the first visual line");
        editor
            .update_mouse_selection(MousePoint::new(0, 5), 10)
            .expect("mouse active point is on the first visual line");

        // act
        let selection = editor.selection().expect("selection is active");
        // assert
        assert_eq!(selection.start().insertion_index(), 0);
        assert_eq!(selection.end().insertion_index(), 4);
        assert!(matches!(
            editor.buffer().atoms()[1].kind,
            AtomKind::Attachment(_)
        ));
        editor.state()
    });
}

#[test]
fn paste_preserves_cjk_emoji_grapheme_and_multiline_boundaries() {
    // arrange
    // act
    assert_deterministic(|| {
        let mut editor = ComposerEditor::new();
        editor
            .paste("e\u{301}界🙂\r\nnext")
            .expect("paste is valid");

        // assert
        assert_eq!(editor.text(), "e\u{301}界🙂\nnext");
        assert_eq!(editor.buffer().atoms().len(), 8);
        assert_eq!(editor.cursor().insertion_index(), 8);
        editor.state()
    });
}

#[test]
fn attachment_insertion_is_one_undo_group() {
    // arrange
    // act
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("a");
        editor
            .insert_attachment(AttachmentId::new(11))
            .expect("attachment insertion is valid");
        // assert
        assert_eq!(editor.undo_depth(), 1);
        assert_eq!(editor.text(), "a[attachment:11]");
        assert!(editor.undo());
        assert_eq!(editor.text(), "a");
        assert!(editor.redo());

        let snapshot = |editor: &ComposerEditor| {
            let state = editor.state();
            (state.buffer, state.cursor, state.selection, state.history)
        };
        let prepare: [fn(&mut ComposerEditor); 3] = [
            ComposerEditor::move_left,
            ComposerEditor::select_all,
            |editor| editor.set_history(vec!["saved prompt".into()]),
        ];
        for prepare in prepare {
            let mut branch = editor.clone();
            prepare(&mut branch);
            let before = snapshot(&branch);
            branch.insert_text("界").expect("branch edit");
            let after = snapshot(&branch);
            assert!(branch.undo());
            assert_eq!(snapshot(&branch), before);
            assert!(branch.redo());
            assert_eq!(snapshot(&branch), after);
            assert!(branch.undo());
            branch.insert_text("new").expect("replace redo branch");
            assert!(!branch.redo());
        }
        editor.state()
    });
}

#[test]
fn history_edit_restores_scratch_without_clobbering_saved_prompt() {
    // arrange
    // act
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("scratch");
        editor.set_history(vec!["old one".into(), "old two".into()]);
        editor.history_previous();
        // assert
        assert_eq!(editor.text(), "old two");
        editor.insert_text("!").expect("text insertion is valid");
        assert_eq!(editor.text(), "old two!");
        editor.history_next();
        assert_eq!(editor.text(), "scratch");
        assert_eq!(editor.history_entries(), &["old one", "old two"]);
        editor.state()
    });
}

#[test]
fn contiguous_char_deletes_group_but_word_delete_is_separate() {
    // arrange
    // act
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("one two");
        let original = editor.buffer().clone();
        editor.backspace().expect("backspace is valid");
        editor.backspace().expect("backspace is valid");
        editor.backspace().expect("backspace is valid");
        // assert
        assert_eq!(editor.text(), "one ");
        assert_eq!(editor.undo_depth(), 1);
        assert!(editor.undo());
        assert_eq!(editor.text(), "one two");
        assert_eq!(editor.buffer(), &original);
        let mut branch = editor.clone();
        branch.insert_text("界").expect("insert after undo");
        let mut expected = original;
        expected
            .insert_text_at(editor.cursor(), "界")
            .expect("expected atom identity");
        assert_eq!(branch.buffer(), &expected);
        editor
            .delete(DeleteKind::WordBackward)
            .expect("word delete");
        assert_eq!(editor.text(), "one ");
        assert_eq!(editor.undo_depth(), 1);
        editor.state()
    });
}

#[test]
fn undo_snapshots_preserve_gaps_current_values_and_redo() {
    use harness_tui::composer_atoms::{AtomBuffer, AtomCursor};
    use harness_tui::composer_editing::{
        EditGroup, EditorSnapshot, PromptHistory, Selection, UndoStack,
    };
    let snapshot = |text: &str| EditorSnapshot {
        buffer: AtomBuffer::from_text(text),
        cursor: AtomCursor::after(0),
        selection: Some(Selection::new(AtomCursor::after(0), AtomCursor::before(0))),
        history: PromptHistory::new(vec![text.into()]),
    };
    let [initial, first, mut gap, deleted, grouped, undo_current, redo_current] =
        ["a", "👩‍💻", "👩‍💻", "d", "e", "x", "y"].map(snapshot);
    gap.buffer
        .insert_text_at(AtomCursor::before(1), "!")
        .expect("allocate atom");
    gap.buffer
        .delete_range(AtomCursor::before(1), AtomCursor::before(2))
        .expect("remove atom");
    assert_ne!(gap.buffer, AtomBuffer::from_text("👩‍💻"));
    gap.cursor = AtomCursor::before(1);
    gap.selection = Some(Selection::new(AtomCursor::before(0), AtomCursor::before(0)));
    gap.history.previous("scratch", 0);
    let mut stack = UndoStack::default();
    stack.record(initial.clone(), first.clone(), EditGroup::CharacterDelete);
    stack.record(gap.clone(), deleted.clone(), EditGroup::CharacterDelete);
    stack.record(deleted, grouped.clone(), EditGroup::CharacterDelete);
    assert_eq!(stack.undo_depth(), 2);
    let mut branch = stack.clone();
    assert_eq!(stack.undo(&undo_current), Some(gap.clone()));
    assert_eq!(branch.undo(&redo_current), Some(gap));
    assert_ne!(branch, stack);
    // Start again to check the current value used by redo becomes the next undo target.
    stack = branch;
    stack.record(grouped.clone(), grouped.clone(), EditGroup::Paste);
    assert_eq!(stack.redo_depth(), 1);
    assert_eq!(stack.redo(&redo_current), Some(grouped));
    assert_eq!(stack.undo(&undo_current), Some(redo_current));
    assert_eq!(stack.undo(&undo_current), Some(initial));
    assert_eq!(stack.redo(&undo_current), Some(first));
    stack.record(undo_current.clone(), snapshot("new"), EditGroup::Paste);
    assert_eq!(stack.redo(&undo_current), None);
}
