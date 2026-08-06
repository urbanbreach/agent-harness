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
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("alpha beta\n界🙂");
        editor.move_word_left();
        assert_eq!(editor.cursor().insertion_index(), 11);
        editor.move_line_start();
        assert_eq!(editor.cursor().insertion_index(), 11);
        editor.state()
    });
}

#[test]
fn mouse_selection_keeps_structured_atoms_whole() {
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

        let selection = editor.selection().expect("selection is active");
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
    assert_deterministic(|| {
        let mut editor = ComposerEditor::new();
        editor
            .paste("e\u{301}界🙂\r\nnext")
            .expect("paste is valid");

        assert_eq!(editor.text(), "e\u{301}界🙂\nnext");
        assert_eq!(editor.buffer().atoms().len(), 8);
        assert_eq!(editor.cursor().insertion_index(), 8);
        editor.state()
    });
}

#[test]
fn attachment_insertion_is_one_undo_group() {
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("a");
        editor
            .insert_attachment(AttachmentId::new(11))
            .expect("attachment insertion is valid");
        assert_eq!(editor.undo_depth(), 1);
        assert_eq!(editor.text(), "a[attachment:11]");
        assert!(editor.undo());
        assert_eq!(editor.text(), "a");
        assert!(editor.redo());
        editor.state()
    });
}

#[test]
fn history_edit_restores_scratch_without_clobbering_saved_prompt() {
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("scratch");
        editor.set_history(vec!["old one".into(), "old two".into()]);
        editor.history_previous();
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
    assert_deterministic(|| {
        let mut editor = ComposerEditor::from_text("one two");
        editor.backspace().expect("backspace is valid");
        editor.backspace().expect("backspace is valid");
        editor.backspace().expect("backspace is valid");
        assert_eq!(editor.text(), "one ");
        assert_eq!(editor.undo_depth(), 1);
        assert!(editor.undo());
        assert_eq!(editor.text(), "one two");
        editor
            .delete(DeleteKind::WordBackward)
            .expect("word delete");
        assert_eq!(editor.text(), "one ");
        assert_eq!(editor.undo_depth(), 1);
        editor.state()
    });
}
