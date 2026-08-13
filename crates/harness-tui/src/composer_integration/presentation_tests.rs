use crate::composer_atoms::AtomCursor;
use crate::composer_editing::{ComposerEditor, Selection};

use super::{
    ComposerChrome, ComposerEditorModel, ComposerPresentation, ComposerPresentationConfig,
    ComposerPresentationError, ComposerSurface, ComposerTone,
};

fn editor_with_selection() -> ComposerEditor {
    let mut editor = ComposerEditor::from_text("A界👩🏽‍💻\nsecond line");
    editor.select_all();
    editor
}

#[test]
fn all_composer_surfaces_share_one_typed_editor_and_presentation_contract() {
    let editor = editor_with_selection();
    let model = ComposerEditorModel::new(&editor, 12, 4).expect("valid editor model");
    let surfaces = [
        ComposerSurface::Startup,
        ComposerSurface::Live,
        ComposerSurface::Shell,
        ComposerSurface::Plan,
        ComposerSurface::Permission,
        ComposerSurface::InlinePrompt,
    ];

    for surface in surfaces {
        let presentation = ComposerPresentation::resolve(
            &model,
            ComposerPresentationConfig::new(surface, true, false, 8),
        )
        .expect("valid presentation");
        assert!(std::ptr::eq(
            presentation.editor(),
            std::ptr::from_ref(&model)
        ));
        assert_eq!(presentation.editor().text(), editor.text());
        assert_eq!(presentation.editor().selection(), editor.selection());
        assert_eq!(presentation.editor().viewport_rows(), 2);
    }
}

#[test]
fn grapheme_cursor_selection_atoms_and_wrapping_survive_surface_switches() {
    let mut editor = ComposerEditor::from_text("e\u{301}界👩🏽‍💻 tail");
    editor.move_buffer_start();
    editor.move_right();
    editor.select_char_right();
    let model = ComposerEditorModel::new(&editor, 4, 3).expect("valid editor model");

    assert_eq!(model.cursor(), AtomCursor::before(2));
    assert_eq!(
        model.selection(),
        Some(Selection::new(AtomCursor::before(1), AtomCursor::before(2)))
    );
    assert_eq!(
        model.atoms().len(),
        8,
        "emoji and combining mark stay atomic"
    );
    assert_eq!(model.viewport_rows(), 3);

    for surface in [
        ComposerSurface::Live,
        ComposerSurface::Permission,
        ComposerSurface::Live,
    ] {
        let presentation = ComposerPresentation::resolve(
            &model,
            ComposerPresentationConfig::new(surface, true, false, 3),
        )
        .expect("surface switch must preserve editor state");
        assert_eq!(presentation.editor().cursor(), model.cursor());
        assert_eq!(presentation.editor().selection(), model.selection());
        assert_eq!(presentation.editor().wrapped_lines(), model.wrapped_lines());
    }
}

#[test]
fn compact_empty_collapse_and_optional_chrome_follow_priority() {
    let editor = ComposerEditor::new();
    let model = ComposerEditorModel::new(&editor, 20, 4).expect("valid editor model");
    let collapsed = ComposerPresentation::resolve(
        &model,
        ComposerPresentationConfig::new(ComposerSurface::Live, false, false, 1),
    )
    .expect("valid compact presentation");
    assert!(collapsed.collapsed());
    assert_eq!(collapsed.visible_chrome(), &[]);

    let startup = ComposerPresentation::resolve(
        &model,
        ComposerPresentationConfig::new(ComposerSurface::Startup, false, false, 1)
            .with_placeholder("Build anything"),
    )
    .expect("valid startup presentation");
    assert!(!startup.collapsed());
    assert_eq!(startup.body(), "Build anything");
    assert_eq!(startup.visible_chrome(), &[ComposerChrome::Border]);

    let focused = ComposerPresentation::resolve(
        &model,
        ComposerPresentationConfig::new(ComposerSurface::Live, true, false, 1),
    )
    .expect("valid focused presentation");
    assert_eq!(focused.text_rows(), 1);
    assert_eq!(focused.visible_chrome(), &[ComposerChrome::Border]);

    let spacious = ComposerPresentation::resolve(
        &model,
        ComposerPresentationConfig::new(ComposerSurface::Startup, true, false, 3),
    )
    .expect("valid spacious presentation");
    assert_eq!(
        spacious.visible_chrome(),
        &[
            ComposerChrome::Border,
            ComposerChrome::Metadata,
            ComposerChrome::Title
        ]
    );
}

#[test]
fn malformed_presentation_fails_closed_without_partial_geometry() {
    let editor = ComposerEditor::from_text("draft");
    assert_eq!(
        ComposerEditorModel::new(&editor, 0, 3),
        Err(ComposerPresentationError::ZeroWrapWidth)
    );
    let model = ComposerEditorModel::new(&editor, 10, 3).expect("valid editor model");
    assert_eq!(
        ComposerPresentation::resolve(
            &model,
            ComposerPresentationConfig::new(ComposerSurface::Live, true, false, 0),
        ),
        Err(ComposerPresentationError::ZeroAvailableRows)
    );
}

#[test]
fn production_wrappers_delegate_body_geometry_to_one_resolver() {
    let bordered = include_str!("../ui_composer/bordered.rs");
    let collapsed = include_str!("../ui_composer/collapsed.rs");
    let document = include_str!("../ui_composer/document.rs");
    for wrapper in [bordered, document] {
        assert!(wrapper.contains("presentation::resolve_composer("));
        assert!(!wrapper.contains("composer_viewport("));
        assert!(wrapper.contains(".marker()"));
        assert!(wrapper.contains(".right_label()"));
    }
    assert!(bordered.contains("collapsed::render_collapsed_composer("));
    assert!(collapsed.contains("presentation::resolve_composer("));
    assert!(collapsed.contains("resolved.body"));
}

#[test]
fn plan_surface_owns_a_distinct_presentation_tone() {
    // Given: one editor rendered through the live, shell, and plan surfaces.
    let editor = ComposerEditor::from_text("draft");
    let model = ComposerEditorModel::new(&editor, 20, 2).expect("valid editor model");

    // When: each presentation is resolved through the shared contract.
    let tone_for = |surface| {
        ComposerPresentation::resolve(
            &model,
            ComposerPresentationConfig::new(surface, true, false, 3),
        )
        .expect("valid presentation")
        .tone()
    };

    // Then: plan remains distinct from ordinary and shell presentation styling.
    assert_eq!(tone_for(ComposerSurface::Live), ComposerTone::Standard);
    assert_eq!(tone_for(ComposerSurface::Shell), ComposerTone::Shell);
    assert_eq!(tone_for(ComposerSurface::Plan), ComposerTone::Plan);
}

#[test]
fn shell_surface_owns_reference_marker_and_semantic_label() {
    assert_eq!(ComposerSurface::Shell.marker(), Some("!"));
    assert_eq!(
        ComposerSurface::Shell.right_label(),
        Some("Run shell command")
    );
    assert_eq!(ComposerSurface::Live.marker(), None);
    assert_eq!(ComposerSurface::Plan.right_label(), None);
}

#[test]
fn compact_draft_hint_priority_keeps_submit_newline_and_mode() {
    use crate::keybindings::Action;

    assert_eq!(
        super::compact_draft_hint_priority(false),
        &[
            Action::SubmitPrompt,
            Action::InsertNewline,
            Action::VariantCycle,
            Action::Help,
        ]
    );
}

#[test]
fn file_and_attachment_atoms_survive_production_reflow_unchanged() {
    use crate::composer_atoms::{AtomBuffer, AtomKind, AttachmentId, ComposerAtom, FileMentionId};

    let atoms = vec![
        ComposerAtom::file_mention(11, FileMentionId::new(7)),
        ComposerAtom::attachment(12, AttachmentId::new(9)),
    ];
    let buffer = AtomBuffer::from_atoms(atoms.clone()).expect("unique atoms");
    let editor = ComposerEditor::from_buffer(buffer);
    let model = ComposerEditorModel::new(&editor, 80, 4).expect("valid model");
    let compact = model.reflow(4, 2).expect("production reflow");

    assert_eq!(compact.atoms(), atoms.as_slice());
    assert!(matches!(compact.atoms()[0].kind, AtomKind::FileMention(_)));
    assert!(matches!(compact.atoms()[1].kind, AtomKind::Attachment(_)));
    assert_eq!(compact.cursor(), model.cursor());
    assert_eq!(compact.selection(), model.selection());
}
