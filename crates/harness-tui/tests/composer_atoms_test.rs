#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for contract fixtures"
)]

use harness_tui::composer_atoms::{
    deserialize, serialize, AtomBoundary, AtomBuffer, AtomCursor, AtomKind, AttachmentId,
    ComposerAtom, FileMentionId, GraphemeCluster,
};

#[test]
fn grapheme_clusters_preserve_combining_marks_zwj_and_regional_pairs() {
    // Given: text containing the grapheme boundaries that byte/char editors split.
    let buffer = AtomBuffer::from_text("e\u{301} 👨‍👩‍👧‍👦 👍🏽 🇺🇳");

    // When: the text is inspected as typed atoms.
    let atoms = buffer.atoms();

    // Then: every grapheme remains one identity-bearing atom with terminal width.
    assert_eq!(atoms.len(), 7);
    assert!(matches!(atoms[0].kind, AtomKind::Text(_)));
    assert_eq!(atoms[0].display_width, 1);
    assert_eq!(atoms[2].display_width, 2);
    assert_eq!(atoms[4].display_width, 2);
    assert_eq!(atoms[6].display_width, 2);
}

#[test]
fn cjk_and_fullwidth_graphemes_use_two_terminal_cells() {
    // Given: a CJK string with a fullwidth Latin character.
    let buffer = AtomBuffer::from_text("界Ａ");

    // When: each text atom reports its display width.
    let widths: Vec<u16> = buffer
        .atoms()
        .iter()
        .map(|atom| atom.display_width)
        .collect();

    // Then: each atom occupies two cells.
    assert_eq!(widths, vec![2, 2]);
}

#[test]
fn newlines_are_atoms_and_text_round_trips_without_byte_offsets() {
    // Given: multiline content with a combining grapheme on the second line.
    let text = "first\nse\u{301}cond\n";

    // When: content is parsed into atoms and projected back to text.
    let buffer = AtomBuffer::from_text(text);

    // Then: newlines have their own atoms and projection preserves the source.
    assert_eq!(
        buffer
            .atoms()
            .iter()
            .filter(|atom| matches!(atom.kind, AtomKind::Newline))
            .count(),
        2
    );
    assert_eq!(buffer.text(), text);
}

#[test]
fn insertion_and_deletion_preserve_unaffected_atom_ids() {
    // Given: a buffer whose middle atom will be edited.
    let mut buffer = AtomBuffer::from_text("abc");
    let first = buffer.atoms()[0].id;
    let last = buffer.atoms()[2].id;

    // When: one atom is inserted before the middle atom, then that atom is removed.
    buffer
        .insert_text_at(AtomCursor::before(1), "界")
        .expect("insertion uses an atom boundary");
    let inserted = buffer.atoms()[1].id;
    buffer
        .delete_range(AtomCursor::before(1), AtomCursor::after(1))
        .expect("deletion uses atom boundaries");

    // Then: existing identities survive and the new identity is never reused.
    assert_eq!(buffer.atoms()[0].id, first);
    assert_eq!(buffer.atoms()[2].id, last);
    assert!(buffer.atoms().iter().all(|atom| atom.id != inserted));
    assert_eq!(buffer.text(), "abc");
}

#[test]
fn empty_buffers_and_empty_text_atoms_are_valid_round_trip_values() {
    // Given: both legal empty representations.
    let empty = AtomBuffer::new();
    let single_empty =
        AtomBuffer::from_atoms(vec![ComposerAtom::text(41, GraphemeCluster::new(""))])
            .expect("an empty text atom is valid");

    // When: both values are serialized and deserialized.
    let empty_back: AtomBuffer = deserialize(&serialize(&empty).unwrap()).unwrap();
    let single_back: AtomBuffer = deserialize(&serialize(&single_empty).unwrap()).unwrap();

    // Then: zero atoms and one zero-width atom remain distinguishable.
    assert!(empty_back.atoms().is_empty());
    assert_eq!(single_back.atoms().len(), 1);
    assert_eq!(single_back.atoms()[0].display_width, 0);
    assert_eq!(single_back.atoms()[0].id.get(), 41);
}

#[test]
fn serialization_preserves_atom_identity_and_typed_nontext_kinds() {
    // Given: a buffer containing every non-text atom kind.
    let atoms = vec![
        ComposerAtom::file_mention(7, FileMentionId::new(9)),
        ComposerAtom::attachment(8, AttachmentId::new(11)),
        ComposerAtom::newline(12),
    ];
    let buffer = AtomBuffer::from_atoms(atoms).expect("unique stable atom ids");

    // When: its stable JSON shape is round-tripped.
    let json = serialize(&buffer).expect("buffer serializes");
    let restored: AtomBuffer = deserialize(&json).expect("buffer deserializes");

    // Then: IDs, kinds, and cursor metadata survive exactly.
    assert_eq!(restored, buffer);
    assert!(matches!(restored.atoms()[0].kind, AtomKind::FileMention(_)));
    assert!(matches!(restored.atoms()[1].kind, AtomKind::Attachment(_)));
    assert!(matches!(restored.atoms()[2].kind, AtomKind::Newline));
}

#[test]
fn viewport_wrapping_keeps_atoms_whole_and_identity_ordered() {
    // Given: atoms whose widths force a wrap, plus an explicit newline.
    let buffer = AtomBuffer::from_text("ab界c\nde");
    let ids: Vec<_> = buffer.atoms().iter().map(|atom| atom.id).collect();

    // When: wrapping is measured in terminal cells, not bytes or chars.
    let lines = buffer.wrap(4);

    // Then: every atom appears once, in order, on a wrapped line.
    let wrapped_ids: Vec<_> = lines
        .iter()
        .flat_map(|line| line.atom_ids.iter())
        .copied()
        .collect();
    assert_eq!(wrapped_ids, ids);
    assert_eq!(
        lines
            .iter()
            .map(|line| line.display_width)
            .collect::<Vec<_>>(),
        vec![4, 1, 2]
    );
    assert!(lines.iter().all(|line| line.display_width <= 4));
    assert_eq!(AtomBoundary::Before, AtomCursor::start().boundary);
}
