//! Task 9: terminal input decode + capability fallback contract tests.
//!
//! Differential TDD ownership for the terminal input shard. Proves:
//! - P1 (contract): ANSI/xterm key sequences, SGR + legacy mouse, focus,
//!   bracketed paste, and resize decode into the typed event model.
//! - P2 (owner): the decoder is streaming and retains incomplete trailing
//!   sequences across `feed` calls.
//! - P3 (terminal): all 20 terminal brands are detected; brand/context
//!   conditionals resolve deterministically.
//! - P6 (rejection): unrecognized sequences surface as `Unknown` rather than
//!   panicking, and capability fallbacks degrade gracefully per brand.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::too_many_lines,
    reason = "contract tests use fail-fast asserts and exhaustive matrices"
)]

use harness_tui::mouse::{MouseButton, MouseEvent, MouseEventKind, MouseScrollDirection};
use harness_tui::terminal::{
    decode_all, terminal_capability_fallback, ColorMode, Decoder, FocusEvent, KeyCode, KeyEvent,
    KeyModifiers, ResizeEvent, TerminalCapabilityLeaf, TerminalContext, TerminalEnv,
    TerminalInputEvent, TerminalMultiplexer, TerminalName,
};

fn events(bytes: &[u8]) -> Vec<TerminalInputEvent> {
    decode_all(bytes)
}

fn one_key(bytes: &[u8]) -> KeyEvent {
    let mut events = events(bytes);
    assert_eq!(events.len(), 1, "expected one event for {bytes:?}");
    match events.pop().unwrap() {
        TerminalInputEvent::Key(key) => key,
        other => panic!("expected a key event, got {other:?}"),
    }
}

fn one_mouse(bytes: &[u8]) -> MouseEvent {
    let mut events = events(bytes);
    assert_eq!(events.len(), 1, "expected one event for {bytes:?}");
    match events.pop().unwrap() {
        TerminalInputEvent::Mouse(mouse) => mouse,
        other => panic!("expected a mouse event, got {other:?}"),
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

// ---------------------------------------------------------------------------
// P1 — printable characters and control bytes
// ---------------------------------------------------------------------------

#[test]
fn empty_input_yields_no_events() {
    // arrange
    // act
    // assert
    assert!(events(b"").is_empty());
}

#[test]
fn printable_ascii_decodes_to_char() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"a"), KeyEvent::char('a'));
    assert_eq!(one_key(b"Z"), KeyEvent::char('Z'));
    assert_eq!(one_key(b" "), KeyEvent::char(' '));
    assert_eq!(one_key(b"~"), KeyEvent::char('~'));
}

#[test]
fn multibyte_utf8_decodes_to_char() {
    // arrange
    // act
    // assert
    assert_eq!(one_key("川".as_bytes()), KeyEvent::char('川'));
    assert_eq!(one_key("✅".as_bytes()), KeyEvent::char('✅'));
    assert_eq!(one_key("❯".as_bytes()), KeyEvent::char('❯'));
}

#[test]
fn control_bytes_decode_to_named_keys() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"\r"), key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(one_key(b"\n"), key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(one_key(b"\t"), key(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(
        one_key(&[0x7F]),
        key(KeyCode::Backspace, KeyModifiers::NONE)
    );
    assert_eq!(
        one_key(&[0x08]),
        key(KeyCode::Backspace, KeyModifiers::NONE)
    );
    assert_eq!(one_key(&[0x00]), key(KeyCode::Null, KeyModifiers::NONE));
}

#[test]
fn ctrl_letter_combinations_carry_ctrl_modifier() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_key(&[0x01]),
        key(KeyCode::Char('A'), KeyModifiers::CTRL)
    );
    assert_eq!(
        one_key(&[0x1A]),
        key(KeyCode::Char('Z'), KeyModifiers::CTRL)
    );
    assert_eq!(
        one_key(&[0x03]),
        key(KeyCode::Char('C'), KeyModifiers::CTRL)
    );
}

#[test]
fn ctrl_punctuation_controls_decode() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_key(&[0x1C]),
        key(KeyCode::Char('\\'), KeyModifiers::CTRL)
    );
    assert_eq!(
        one_key(&[0x1D]),
        key(KeyCode::Char(']'), KeyModifiers::CTRL)
    );
}

#[test]
fn bare_escape_flushes_to_esc_key() {
    // arrange
    // act
    let mut decoder = Decoder::new();
    // assert
    assert!(decoder.feed(&[0x1B]).is_empty(), "lone ESC waits for more");
    let flushed = decoder.flush();
    assert_eq!(
        flushed,
        vec![TerminalInputEvent::Key(KeyEvent::plain(KeyCode::Esc))]
    );
}

// ---------------------------------------------------------------------------
// P1 — CSI cursor / navigation / function keys
// ---------------------------------------------------------------------------

#[test]
fn csi_cursor_keys_decode() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"\x1b[A"), key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[B"), key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[C"), key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[D"), key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[H"), key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[F"), key(KeyCode::End, KeyModifiers::NONE));
}

#[test]
fn csi_cursor_with_modifier_decodes_modifier() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"\x1b[1;5A"), key(KeyCode::Up, KeyModifiers::CTRL));
    assert_eq!(
        one_key(b"\x1b[1;2B"),
        key(KeyCode::Down, KeyModifiers::SHIFT)
    );
    assert_eq!(
        one_key(b"\x1b[1;3C"),
        key(KeyCode::Right, KeyModifiers::ALT)
    );
    assert_eq!(
        one_key(b"\x1b[1;6D"),
        key(KeyCode::Left, KeyModifiers::CTRL.union(KeyModifiers::SHIFT))
    );
}

#[test]
fn ss3_sequences_decode_into_terminal_events() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"\x1bOA"), key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOB"), key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOP"), key(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOS"), key(KeyCode::F(4), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOH"), key(KeyCode::Home, KeyModifiers::NONE));
}

#[test]
fn tilde_codes_decode_to_named_keys() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_key(b"\x1b[2~"),
        key(KeyCode::Insert, KeyModifiers::NONE)
    );
    assert_eq!(
        one_key(b"\x1b[3~"),
        key(KeyCode::Delete, KeyModifiers::NONE)
    );
    assert_eq!(
        one_key(b"\x1b[5~"),
        key(KeyCode::PageUp, KeyModifiers::NONE)
    );
    assert_eq!(
        one_key(b"\x1b[6~"),
        key(KeyCode::PageDown, KeyModifiers::NONE)
    );
    assert_eq!(one_key(b"\x1b[1~"), key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[4~"), key(KeyCode::End, KeyModifiers::NONE));
}

#[test]
fn tilde_codes_cover_full_function_key_range() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"\x1b[11~"), key(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[15~"), key(KeyCode::F(5), KeyModifiers::NONE));
    assert_eq!(
        one_key(b"\x1b[21~"),
        key(KeyCode::F(10), KeyModifiers::NONE)
    );
    assert_eq!(
        one_key(b"\x1b[23~"),
        key(KeyCode::F(11), KeyModifiers::NONE)
    );
    assert_eq!(
        one_key(b"\x1b[24~"),
        key(KeyCode::F(12), KeyModifiers::NONE)
    );
    assert_eq!(
        one_key(b"\x1b[34~"),
        key(KeyCode::F(20), KeyModifiers::NONE)
    );
}

#[test]
fn csi_function_keys_p_to_s_decode() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"\x1b[P"), key(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[Q"), key(KeyCode::F(2), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[R"), key(KeyCode::F(3), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[S"), key(KeyCode::F(4), KeyModifiers::NONE));
    assert_eq!(
        one_key(b"\x1b[1;2P"),
        key(KeyCode::F(1), KeyModifiers::SHIFT)
    );
}

#[test]
fn modified_tilde_codes_decode_modifier() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_key(b"\x1b[3;5~"),
        key(KeyCode::Delete, KeyModifiers::CTRL)
    );
    assert_eq!(
        one_key(b"\x1b[5;2~"),
        key(KeyCode::PageUp, KeyModifiers::SHIFT)
    );
}

#[test]
fn backtab_decodes_to_shift_tab() {
    // arrange
    // act
    // assert
    assert_eq!(one_key(b"\x1b[Z"), key(KeyCode::Tab, KeyModifiers::SHIFT));
}

// ---------------------------------------------------------------------------
// P1 — Kitty / CSI u enhanced keyboard protocol
// ---------------------------------------------------------------------------

#[test]
fn csi_u_decodes_modified_characters() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_key(b"\x1b[97;5u"),
        key(KeyCode::Char('a'), KeyModifiers::CTRL)
    );
    assert_eq!(
        one_key(b"\x1b[13;2u"),
        key(KeyCode::Enter, KeyModifiers::SHIFT)
    );
    assert_eq!(one_key(b"\x1b[9;3u"), key(KeyCode::Tab, KeyModifiers::ALT));
    assert_eq!(
        one_key(b"\x1b[27;5u"),
        key(KeyCode::Esc, KeyModifiers::CTRL)
    );
}

#[test]
fn csi_27_legacy_modifier_form_decodes() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_key(b"\x1b[27;5;97~"),
        key(KeyCode::Char('a'), KeyModifiers::CTRL)
    );
    assert_eq!(
        one_key(b"\x1b[27;2;13~"),
        key(KeyCode::Enter, KeyModifiers::SHIFT)
    );
}

#[test]
fn escape_printable_decodes_to_alt_char() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_key(b"\x1bx"),
        key(KeyCode::Char('x'), KeyModifiers::ALT)
    );
    assert_eq!(
        one_key(b"\x1bb"),
        key(KeyCode::Char('b'), KeyModifiers::ALT)
    );
}

// ---------------------------------------------------------------------------
// P1 / P3 — SGR mouse decoding (mode 1006)
// ---------------------------------------------------------------------------

fn mouse_kind(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent::new(kind, column, row)
}

#[test]
fn sgr_mouse_press_and_release_decode() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_mouse(b"\x1b[<0;10;20M"),
        mouse_kind(MouseEventKind::Down(MouseButton::Left), 9, 19)
    );
    assert_eq!(
        one_mouse(b"\x1b[<0;10;20m"),
        mouse_kind(MouseEventKind::Up(MouseButton::Left), 9, 19)
    );
    assert_eq!(
        one_mouse(b"\x1b[<2;5;5M"),
        mouse_kind(MouseEventKind::Down(MouseButton::Right), 4, 4)
    );
    assert_eq!(
        one_mouse(b"\x1b[<1;1;1M"),
        mouse_kind(MouseEventKind::Down(MouseButton::Middle), 0, 0)
    );
}

#[test]
fn sgr_mouse_motion_and_drag_decode() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_mouse(b"\x1b[<32;5;5M"),
        mouse_kind(MouseEventKind::Drag(MouseButton::Left), 4, 4)
    );
    assert_eq!(
        one_mouse(b"\x1b[<35;5;5M"),
        mouse_kind(MouseEventKind::Moved, 4, 4)
    );
}

#[test]
fn sgr_mouse_wheel_decodes_direction() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_mouse(b"\x1b[<64;3;3M"),
        mouse_kind(MouseEventKind::Scroll(MouseScrollDirection::Up), 2, 2)
    );
    assert_eq!(
        one_mouse(b"\x1b[<65;3;3M"),
        mouse_kind(MouseEventKind::Scroll(MouseScrollDirection::Down), 2, 2)
    );
    assert_eq!(
        one_mouse(b"\x1b[<66;3;3M"),
        mouse_kind(MouseEventKind::Scroll(MouseScrollDirection::Left), 2, 2)
    );
    assert_eq!(
        one_mouse(b"\x1b[<67;3;3M"),
        mouse_kind(MouseEventKind::Scroll(MouseScrollDirection::Right), 2, 2)
    );
}

#[test]
fn sgr_mouse_modifier_bits_do_not_change_button_kind() {
    // arrange
    // act
    // assert
    assert_eq!(
        one_mouse(b"\x1b[<16;5;5M"),
        mouse_kind(MouseEventKind::Down(MouseButton::Left), 4, 4)
    );
}

// ---------------------------------------------------------------------------
// P1 / P3 — legacy X10 mouse decoding (default encoding)
// ---------------------------------------------------------------------------

#[test]
fn legacy_x10_mouse_left_press_decodes() {
    // arrange
    // act
    let bytes = vec![0x1B, b'[', b'M', 32, 33, 33];
    // assert
    assert_eq!(
        one_mouse(&bytes),
        mouse_kind(MouseEventKind::Down(MouseButton::Left), 0, 0)
    );
}

#[test]
fn legacy_x10_mouse_right_press_and_release_decode() {
    // arrange
    // act
    let right = vec![0x1B, b'[', b'M', 34, 42, 52];
    // assert
    assert_eq!(
        one_mouse(&right),
        mouse_kind(MouseEventKind::Down(MouseButton::Right), 9, 19)
    );
    let release = vec![0x1B, b'[', b'M', 35, 33, 33];
    assert_eq!(
        one_mouse(&release),
        mouse_kind(MouseEventKind::Up(MouseButton::Left), 0, 0)
    );
}

#[test]
fn legacy_x10_mouse_wheel_up_decodes() {
    // arrange
    // act
    let wheel = vec![0x1B, b'[', b'M', 96, 33, 33];
    // assert
    assert_eq!(
        one_mouse(&wheel),
        mouse_kind(MouseEventKind::Scroll(MouseScrollDirection::Up), 0, 0)
    );
}

// ---------------------------------------------------------------------------
// P1 — focus events and bracketed paste
// ---------------------------------------------------------------------------

#[test]
fn focus_events_decode_into_terminal_events() {
    // arrange
    // act
    // assert
    assert_eq!(
        events(b"\x1b[I"),
        vec![TerminalInputEvent::Focus(FocusEvent::Gained)]
    );
    assert_eq!(
        events(b"\x1b[O"),
        vec![TerminalInputEvent::Focus(FocusEvent::Lost)]
    );
}

#[test]
fn bracketed_paste_decodes_content() {
    // arrange
    // act
    // assert
    assert_eq!(
        events(b"\x1b[200~hello world\x1b[201~"),
        vec![TerminalInputEvent::Paste("hello world".to_string())]
    );
}

#[test]
fn empty_bracketed_paste_decodes() {
    // arrange
    // act
    // assert
    assert_eq!(
        events(b"\x1b[200~\x1b[201~"),
        vec![TerminalInputEvent::Paste(String::new())]
    );
}

#[test]
fn paste_with_embedded_escape_decodes_literally() {
    // arrange
    // act
    let mut bytes = b"\x1b[200~line1\x1b[Aline2\x1b[201~".to_vec();
    bytes.extend_from_slice(b"tail");
    let events = events(&bytes);
    // assert
    assert_eq!(events.len(), 5);
    assert_eq!(
        events[0],
        TerminalInputEvent::Paste("line1\x1b[Aline2".to_string())
    );
}

// ---------------------------------------------------------------------------
// P1 — resize reports
// ---------------------------------------------------------------------------

#[test]
fn resize_is_constructed_out_of_band() {
    // arrange
    // act
    // assert
    assert_eq!(
        Decoder::resize(80, 24),
        TerminalInputEvent::Resize(ResizeEvent::new(80, 24))
    );
    assert_eq!(
        Decoder::resize(120, 40),
        TerminalInputEvent::Resize(ResizeEvent::new(120, 40))
    );
}

// ---------------------------------------------------------------------------
// P2 — streaming decoder retains incomplete sequences across feeds
// ---------------------------------------------------------------------------

#[test]
fn streaming_split_csi_completes_across_feeds() {
    // arrange
    // act
    let mut decoder = Decoder::new();
    // assert
    assert!(decoder.feed(b"\x1b[1").is_empty(), "partial CSI held");
    let events = decoder.feed(b";5A");
    assert_eq!(
        events,
        vec![TerminalInputEvent::Key(key(
            KeyCode::Up,
            KeyModifiers::CTRL
        ))]
    );
}

#[test]
fn streaming_split_sgr_mouse_completes_across_feeds() {
    // arrange
    // act
    let mut decoder = Decoder::new();
    // assert
    assert!(decoder.feed(b"\x1b[<0;").is_empty());
    assert!(decoder.feed(b"10;").is_empty());
    let events = decoder.feed(b"20M");
    assert_eq!(
        events,
        vec![TerminalInputEvent::Mouse(mouse_kind(
            MouseEventKind::Down(MouseButton::Left),
            9,
            19
        ))]
    );
}

#[test]
fn streaming_split_paste_spans_three_feeds() {
    // arrange
    // act
    let mut decoder = Decoder::new();
    // assert
    assert!(decoder.feed(b"\x1b[200~abc").is_empty());
    assert!(decoder.feed(b"def\x1b[2").is_empty(), "partial terminator");
    let events = decoder.feed(b"01~");
    assert_eq!(
        events,
        vec![TerminalInputEvent::Paste("abcdef".to_string())]
    );
}

#[test]
fn streaming_mixed_text_and_arrows_decode_in_order() {
    // arrange
    // act
    let mut bytes = b"hi".to_vec();
    bytes.extend_from_slice(b"\x1b[A");
    bytes.extend_from_slice(b"\x1b[B");
    let events = events(&bytes);
    // assert
    assert_eq!(
        events,
        vec![
            TerminalInputEvent::Key(KeyEvent::char('h')),
            TerminalInputEvent::Key(KeyEvent::char('i')),
            TerminalInputEvent::Key(key(KeyCode::Up, KeyModifiers::NONE)),
            TerminalInputEvent::Key(key(KeyCode::Down, KeyModifiers::NONE)),
        ]
    );
}

// ---------------------------------------------------------------------------
// P6 — rejection: unknown sequences surface as Unknown, never panic
// ---------------------------------------------------------------------------

#[test]
fn unknown_ss3_byte_is_rejected_as_unknown() {
    // arrange
    // act
    // assert
    assert_eq!(
        events(b"\x1bOZ"),
        vec![TerminalInputEvent::Unknown(vec![0x1B, b'O', b'Z'])]
    );
}

#[test]
fn unknown_csi_code_is_rejected_as_unknown() {
    // arrange
    // act
    // assert
    assert_eq!(
        events(b"\x1b[999~"),
        vec![TerminalInputEvent::Unknown(vec![b'9', b'9', b'9', b'~'])]
    );
}

#[test]
fn invalid_utf8_byte_is_rejected_as_unknown() {
    // arrange
    // act
    let events = events(&[0xFF, b'a']);
    // assert
    assert_eq!(
        events,
        vec![
            TerminalInputEvent::Unknown(vec![0xFF]),
            TerminalInputEvent::Key(KeyEvent::char('a'))
        ]
    );
}

// ---------------------------------------------------------------------------
// P3 — terminal brand detection (all 20 brands)
// ---------------------------------------------------------------------------

fn env_term_program(program: &str) -> TerminalEnv {
    TerminalEnv {
        term_program: Some(program.to_string()),
        ..TerminalEnv::default()
    }
}

#[test]
fn empty_environment_detects_unknown() {
    // arrange
    // act
    // assert
    assert_eq!(
        TerminalName::detect(&TerminalEnv::default()),
        TerminalName::Unknown
    );
}

#[test]
fn all_twenty_brands_are_distinct() {
    // arrange
    // act
    let keys: std::collections::HashSet<&str> =
        TerminalName::ALL.iter().map(|brand| brand.key()).collect();
    // assert
    assert_eq!(keys.len(), 20);
    assert_eq!(TerminalName::ALL.len(), 20);
}

include!("support/terminal_input_decode_test_part2_test.rs");
