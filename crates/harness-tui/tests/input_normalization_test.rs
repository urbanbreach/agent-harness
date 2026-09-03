#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for contract fixtures"
)]

use std::time::Duration;

use harness_tui::input::{
    EscAction, EscLayer, InputNormalizer, KeyProtocol, ModifierVariant, NormalizedInput, PasteKind,
    MODIFIER_VARIANTS, PASTE_BURST_WINDOW, PASTE_START_WINDOW,
};
use harness_tui::terminal::{KeyCode, KeyEvent, KeyModifiers, ResizeEvent, TerminalInputEvent};

fn key_event(code: KeyCode, modifiers: KeyModifiers) -> TerminalInputEvent {
    TerminalInputEvent::Key(KeyEvent::new(code, modifiers))
}

fn one_key(bytes: &[u8]) -> harness_tui::input::NormalizedKey {
    let mut normalizer = InputNormalizer::new();
    let mut output = normalizer.ingest_bytes_at(Duration::ZERO, bytes).unwrap();
    assert_eq!(output.len(), 1, "expected one normalized key for {bytes:?}");
    match output.pop().unwrap() {
        NormalizedInput::Key(key) => key,
        other => panic!("expected key, got {other:?}"),
    }
}

#[test]
fn protocol_variants_share_one_canonical_key() {
    // arrange
    let variants = [
        (b"\x1b[A".as_slice(), KeyCode::Up, KeyModifiers::NONE),
        (b"\x1bOA".as_slice(), KeyCode::Up, KeyModifiers::NONE),
        (b"\x1b[1;1A".as_slice(), KeyCode::Up, KeyModifiers::NONE),
        (
            b"\x1b[97;5u".as_slice(),
            KeyCode::Char('A'),
            KeyModifiers::CTRL,
        ),
        (
            b"\x1b[27;5;97~".as_slice(),
            KeyCode::Char('A'),
            KeyModifiers::CTRL,
        ),
        (&[0x01], KeyCode::Char('A'), KeyModifiers::CTRL),
    ];

    // act
    for (bytes, code, modifiers) in variants {
        // assert
        assert_eq!(one_key(bytes).code, code);
        assert_eq!(one_key(bytes).modifiers, modifiers);
    }
    assert_eq!(KeyProtocol::all().len(), 6);
}

#[test]
fn modifier_wire_table_is_canonical_and_complete() {
    // arrange
    // act
    for variant in MODIFIER_VARIANTS {
        let key = harness_tui::input::NormalizedKey::from_event(KeyEvent::new(
            KeyCode::Char('x'),
            variant.modifiers,
        ));
        // assert
        assert_eq!(key.modifiers, variant.modifiers);
        assert_eq!(ModifierVariant::from_wire(variant.wire), variant);
    }
}

#[test]
fn bracketed_cjk_and_emoji_paste_is_byte_exact() {
    // arrange
    // act
    let text = "你好，世界 👩‍💻 é";
    let mut normalizer = InputNormalizer::new();
    let output = normalizer
        .ingest_at(Duration::ZERO, TerminalInputEvent::Paste(text.to_string()))
        .unwrap();

    // assert
    assert_eq!(
        output,
        vec![NormalizedInput::paste(text, PasteKind::Bracketed)]
    );
    if let NormalizedInput::Paste(paste) = &output[0] {
        assert_eq!(paste.text.as_bytes(), text.as_bytes());
    }
}

#[test]
fn heuristic_paste_windows_have_explicit_boundaries() {
    // arrange
    // act
    let mut normalizer = InputNormalizer::new();
    // assert
    assert!(normalizer
        .ingest_at(
            Duration::ZERO,
            key_event(KeyCode::Char('a'), KeyModifiers::NONE),
        )
        .unwrap()
        .is_empty());
    assert!(normalizer
        .ingest_at(
            PASTE_START_WINDOW,
            key_event(KeyCode::Char('b'), KeyModifiers::NONE),
        )
        .unwrap()
        .is_empty());
    let output = normalizer.flush_at(Duration::from_millis(20)).unwrap();
    assert_eq!(
        output,
        vec![NormalizedInput::paste("ab", PasteKind::Heuristic)]
    );
    assert_eq!(PASTE_BURST_WINDOW, Duration::from_millis(10));
}

#[test]
fn resize_storm_emits_only_the_latest_after_sixteen_ms_quiet() {
    // arrange
    // act
    let mut normalizer = InputNormalizer::new();
    for (at, cols, rows) in [(0, 80, 24), (5, 100, 30), (10, 120, 40)] {
        // assert
        assert!(normalizer
            .ingest_at(
                Duration::from_millis(at),
                TerminalInputEvent::Resize(ResizeEvent::new(cols, rows)),
            )
            .unwrap()
            .is_empty());
    }
    assert!(normalizer
        .flush_at(Duration::from_millis(15))
        .unwrap()
        .is_empty());
    assert_eq!(
        normalizer.flush_at(Duration::from_millis(16)).unwrap(),
        vec![NormalizedInput::Resize(ResizeEvent::new(120, 40))]
    );
}

#[test]
fn esc_dismisses_child_before_parent() {
    // arrange
    let mut normalizer = InputNormalizer::new();
    normalizer.esc_mut().push(EscLayer::Modal);
    normalizer.esc_mut().push(EscLayer::ChildOverlay);

    let first = normalizer
        .ingest_at(Duration::ZERO, key_event(KeyCode::Esc, KeyModifiers::NONE))
        .unwrap();
    assert_eq!(
        first,
        vec![NormalizedInput::Escape(EscAction::Dismiss(
            EscLayer::ChildOverlay
        ))]
    );

    // act
    let second = normalizer
        .ingest_at(
            Duration::from_millis(1),
            key_event(KeyCode::Esc, KeyModifiers::NONE),
        )
        .unwrap();
    // assert
    assert_eq!(
        second,
        vec![NormalizedInput::Escape(EscAction::Dismiss(EscLayer::Modal))]
    );
}

#[test]
fn ctrl_c_is_interrupt_then_kill_for_empty_and_nonempty_input() {
    // arrange
    // act
    let mut normalizer = InputNormalizer::new();
    normalizer.set_composer_nonempty(false);
    // assert
    assert_eq!(
        normalizer
            .ingest_at(
                Duration::ZERO,
                key_event(KeyCode::Char('c'), KeyModifiers::CTRL),
            )
            .unwrap(),
        vec![NormalizedInput::interrupt(false)]
    );
    normalizer.set_composer_nonempty(true);
    assert_eq!(
        normalizer
            .ingest_at(
                Duration::from_millis(500),
                key_event(KeyCode::Char('C'), KeyModifiers::CTRL),
            )
            .unwrap(),
        vec![NormalizedInput::kill()]
    );
}

#[test]
fn heuristic_text_has_no_character_loss_or_duplicate_action() {
    // arrange
    // act
    let text = "你好🌍";
    let mut normalizer = InputNormalizer::new();
    for (index, character) in text.chars().enumerate() {
        let output = normalizer
            .ingest_at(
                Duration::from_millis(index as u64),
                key_event(KeyCode::Char(character), KeyModifiers::NONE),
            )
            .unwrap();
        // assert
        assert!(output.is_empty());
    }
    let output = normalizer.flush_at(Duration::from_millis(30)).unwrap();
    assert_eq!(
        output,
        vec![NormalizedInput::paste(text, PasteKind::Heuristic)]
    );
    assert!(normalizer
        .flush_at(Duration::from_millis(31))
        .unwrap()
        .is_empty());
}
