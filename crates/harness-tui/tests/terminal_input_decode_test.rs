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
    assert!(events(b"").is_empty());
}

#[test]
fn printable_ascii_decodes_to_char() {
    assert_eq!(one_key(b"a"), KeyEvent::char('a'));
    assert_eq!(one_key(b"Z"), KeyEvent::char('Z'));
    assert_eq!(one_key(b" "), KeyEvent::char(' '));
    assert_eq!(one_key(b"~"), KeyEvent::char('~'));
}

#[test]
fn multibyte_utf8_decodes_to_char() {
    assert_eq!(one_key("川".as_bytes()), KeyEvent::char('川'));
    assert_eq!(one_key("✅".as_bytes()), KeyEvent::char('✅'));
    assert_eq!(one_key("❯".as_bytes()), KeyEvent::char('❯'));
}

#[test]
fn control_bytes_decode_to_named_keys() {
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
    let mut decoder = Decoder::new();
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
    assert_eq!(one_key(b"\x1b[A"), key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[B"), key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[C"), key(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[D"), key(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[H"), key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1b[F"), key(KeyCode::End, KeyModifiers::NONE));
}

#[test]
fn csi_cursor_with_modifier_decodes_modifier() {
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
fn ss3_sequences_decode() {
    assert_eq!(one_key(b"\x1bOA"), key(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOB"), key(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOP"), key(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOS"), key(KeyCode::F(4), KeyModifiers::NONE));
    assert_eq!(one_key(b"\x1bOH"), key(KeyCode::Home, KeyModifiers::NONE));
}

#[test]
fn tilde_codes_decode_to_named_keys() {
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
    assert_eq!(one_key(b"\x1b[Z"), key(KeyCode::Tab, KeyModifiers::SHIFT));
}

// ---------------------------------------------------------------------------
// P1 — Kitty / CSI u enhanced keyboard protocol
// ---------------------------------------------------------------------------

#[test]
fn csi_u_decodes_modified_characters() {
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
    let bytes = vec![0x1B, b'[', b'M', 32, 33, 33];
    assert_eq!(
        one_mouse(&bytes),
        mouse_kind(MouseEventKind::Down(MouseButton::Left), 0, 0)
    );
}

#[test]
fn legacy_x10_mouse_right_press_and_release_decode() {
    let right = vec![0x1B, b'[', b'M', 34, 42, 52];
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
    let wheel = vec![0x1B, b'[', b'M', 96, 33, 33];
    assert_eq!(
        one_mouse(&wheel),
        mouse_kind(MouseEventKind::Scroll(MouseScrollDirection::Up), 0, 0)
    );
}

// ---------------------------------------------------------------------------
// P1 — focus events and bracketed paste
// ---------------------------------------------------------------------------

#[test]
fn focus_events_decode() {
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
    assert_eq!(
        events(b"\x1b[200~hello world\x1b[201~"),
        vec![TerminalInputEvent::Paste("hello world".to_string())]
    );
}

#[test]
fn empty_bracketed_paste_decodes() {
    assert_eq!(
        events(b"\x1b[200~\x1b[201~"),
        vec![TerminalInputEvent::Paste(String::new())]
    );
}

#[test]
fn paste_with_embedded_escape_decodes_literally() {
    let mut bytes = b"\x1b[200~line1\x1b[Aline2\x1b[201~".to_vec();
    bytes.extend_from_slice(b"tail");
    let events = events(&bytes);
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
    let mut decoder = Decoder::new();
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
    let mut decoder = Decoder::new();
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
    let mut decoder = Decoder::new();
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
    let mut bytes = b"hi".to_vec();
    bytes.extend_from_slice(b"\x1b[A");
    bytes.extend_from_slice(b"\x1b[B");
    let events = events(&bytes);
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
    assert_eq!(
        events(b"\x1bOZ"),
        vec![TerminalInputEvent::Unknown(vec![0x1B, b'O', b'Z'])]
    );
}

#[test]
fn unknown_csi_code_is_rejected_as_unknown() {
    assert_eq!(
        events(b"\x1b[999~"),
        vec![TerminalInputEvent::Unknown(vec![b'9', b'9', b'9', b'~'])]
    );
}

#[test]
fn invalid_utf8_byte_is_rejected_as_unknown() {
    let events = events(&[0xFF, b'a']);
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
    assert_eq!(
        TerminalName::detect(&TerminalEnv::default()),
        TerminalName::Unknown
    );
}

#[test]
fn all_twenty_brands_are_distinct() {
    let keys: std::collections::HashSet<&str> =
        TerminalName::ALL.iter().map(|brand| brand.key()).collect();
    assert_eq!(keys.len(), 20);
    assert_eq!(TerminalName::ALL.len(), 20);
}

#[test]
fn brand_detection_by_term_program() {
    assert_eq!(
        TerminalName::detect(&env_term_program("ghostty")),
        TerminalName::Ghostty
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("WarpTerminal")),
        TerminalName::WarpTerminal
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("vscode")),
        TerminalName::VsCode
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("Zed")),
        TerminalName::Zed
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("WezTerm")),
        TerminalName::WezTerm
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("iTerm.app")),
        TerminalName::Iterm2
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("Apple_Terminal")),
        TerminalName::AppleTerminal
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("rio")),
        TerminalName::Rio
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("kitty")),
        TerminalName::Kitty
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("grok-desktop")),
        TerminalName::GrokDesktop
    );
    assert_eq!(
        TerminalName::detect(&env_term_program("otty")),
        TerminalName::Otty
    );
}

#[test]
fn brand_detection_by_specific_env_markers() {
    let kitty = TerminalEnv {
        kitty_window_id: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&kitty), TerminalName::Kitty);

    let ghostty = TerminalEnv {
        ghostty_resources_dir: Some("/share".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&ghostty), TerminalName::Ghostty);

    let warp = TerminalEnv {
        warp_session_id: Some("s".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&warp), TerminalName::WarpTerminal);

    let terminator = TerminalEnv {
        terminator_uuid: Some("urn:uuid".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&terminator), TerminalName::Terminator);

    let windows_terminal = TerminalEnv {
        wt_session: Some("{guid}".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalName::detect(&windows_terminal),
        TerminalName::WindowsTerminal
    );

    let jetbrains = TerminalEnv {
        terminal_emulator: Some("JetBrains-JediTerm".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&jetbrains), TerminalName::JetBrains);
}

#[test]
fn brand_detection_by_term_value() {
    let alacritty = TerminalEnv {
        term: Some("alacritty".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&alacritty), TerminalName::Alacritty);

    let foot = TerminalEnv {
        term: Some("foot".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&foot), TerminalName::Foot);

    let vte = TerminalEnv {
        vte_version: Some("7400".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&vte), TerminalName::Vte);
}

#[test]
fn vscode_family_forks_discriminate_over_vscode() {
    let cursor = TerminalEnv {
        term_program: Some("vscode".to_string()),
        cursor_session: Some("sess".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&cursor), TerminalName::Cursor);

    let windsurf = TerminalEnv {
        term_program: Some("vscode".to_string()),
        windsurf: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&windsurf), TerminalName::Windsurf);
}

#[test]
fn lc_terminal_marker_detects_iterm2() {
    let iterm = TerminalEnv {
        lc_terminal: Some("iTerm2".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(TerminalName::detect(&iterm), TerminalName::Iterm2);
}

// ---------------------------------------------------------------------------
// P3 — brand capability conditionals
// ---------------------------------------------------------------------------

#[test]
fn vte_based_conditionals() {
    assert!(TerminalName::Vte.is_vte_based());
    assert!(TerminalName::Terminator.is_vte_based());
    assert!(!TerminalName::Ghostty.is_vte_based());
    assert!(!TerminalName::Kitty.is_vte_based());
}

#[test]
fn vscode_family_conditionals() {
    for brand in [
        TerminalName::VsCode,
        TerminalName::Cursor,
        TerminalName::Windsurf,
    ] {
        assert!(brand.is_vscode_family(), "{brand:?} expected vscode family");
    }
    assert!(!TerminalName::Ghostty.is_vscode_family());
}

#[test]
fn capability_unclassified_conditionals() {
    assert!(TerminalName::Unknown.is_capability_unclassified());
    assert!(TerminalName::Otty.is_capability_unclassified());
    assert!(!TerminalName::Ghostty.is_capability_unclassified());
    assert!(!TerminalName::VsCode.is_capability_unclassified());
}

#[test]
fn osc52_support_is_brand_allowlisted() {
    assert!(TerminalName::Ghostty.supports_osc52_clipboard());
    assert!(TerminalName::Kitty.supports_osc52_clipboard());
    assert!(TerminalName::WezTerm.supports_osc52_clipboard());
    assert!(TerminalName::Cursor.supports_osc52_clipboard());
    assert!(!TerminalName::AppleTerminal.supports_osc52_clipboard());
    assert!(!TerminalName::Vte.supports_osc52_clipboard());
    assert!(!TerminalName::Unknown.supports_osc52_clipboard());
}

#[test]
fn csi_query_interception_conditionals() {
    assert!(TerminalName::JetBrains.intercepts_csi_queries());
    assert!(TerminalName::WarpTerminal.intercepts_csi_queries());
    assert!(!TerminalName::Ghostty.intercepts_csi_queries());
}

#[test]
fn enhanced_keyboard_support_conditionals() {
    assert!(TerminalName::Kitty.supports_enhanced_keyboard());
    assert!(TerminalName::Ghostty.supports_enhanced_keyboard());
    assert!(TerminalName::WindowsTerminal.supports_enhanced_keyboard());
    assert!(!TerminalName::AppleTerminal.supports_enhanced_keyboard());
    assert!(!TerminalName::Vte.supports_enhanced_keyboard());
    assert!(!TerminalName::Unknown.supports_enhanced_keyboard());
}

#[test]
fn shift_enter_diverges_from_generic_enhanced_keyboard() {
    // WindowsTerminal has the enhanced protocol yet still mishandles Shift+Enter.
    assert!(TerminalName::WindowsTerminal.supports_enhanced_keyboard());
    let ctx = TerminalContext {
        brand: TerminalName::WindowsTerminal,
        multiplexer: TerminalMultiplexer::Undetected,
        alt_screen: harness_tui::terminal::lifecycle::AltScreenMode::Auto,
        is_tty: true,
        is_byobu: false,
    };
    assert!(ctx.shift_enter_unavailable());
}

// ---------------------------------------------------------------------------
// P3 — multiplexer detection and conditionals
// ---------------------------------------------------------------------------

#[test]
fn multiplexer_detection() {
    let tmux = TerminalEnv {
        tmux: Some("/tmp/tmux-1000/default,123,0".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&tmux),
        TerminalMultiplexer::Tmux
    );

    let zellij = TerminalEnv {
        zellij: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&zellij),
        TerminalMultiplexer::Zellij
    );

    let screen = TerminalEnv {
        screen_sty: Some("1234.pts-0.host".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&screen),
        TerminalMultiplexer::Screen
    );

    let cmux = TerminalEnv {
        cmux: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&cmux),
        TerminalMultiplexer::Cmux
    );

    assert_eq!(
        TerminalMultiplexer::detect(&TerminalEnv::default()),
        TerminalMultiplexer::Undetected
    );
}

#[test]
fn byobu_detection_defaults_to_tmux_backend() {
    let byobu = TerminalEnv {
        byobu: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&byobu),
        TerminalMultiplexer::Tmux
    );

    let byobu_screen = TerminalEnv {
        byobu: Some("1".to_string()),
        byobu_backend: Some("screen".to_string()),
        ..TerminalEnv::default()
    };
    assert_eq!(
        TerminalMultiplexer::detect(&byobu_screen),
        TerminalMultiplexer::Screen
    );
}

#[test]
fn multiplexer_csi_interception() {
    assert!(TerminalMultiplexer::Tmux.intercepts_csi_queries());
    assert!(TerminalMultiplexer::Screen.intercepts_csi_queries());
    assert!(TerminalMultiplexer::Zellij.intercepts_csi_queries());
    assert!(!TerminalMultiplexer::Undetected.intercepts_csi_queries());
}

// ---------------------------------------------------------------------------
// P3 / P6 — capability fallback / graceful degradation
// ---------------------------------------------------------------------------

fn ghostty_env() -> TerminalEnv {
    env_term_program("ghostty")
}

fn apple_env() -> TerminalEnv {
    env_term_program("Apple_Terminal")
}

#[test]
fn modern_terminal_resolves_full_capabilities() {
    let caps = terminal_capability_fallback(&ghostty_env(), ColorMode::Truecolor, true);
    let expected = TerminalCapabilityLeaf {
        color_mode: ColorMode::Truecolor,
        keyboard_mode: harness_tui::terminal::KeyboardMode::Enhanced,
        mouse_capture: true,
        bracketed_paste: true,
        osc52_clipboard: true,
        alternate_screen: true,
        focus_reporting: true,
    };
    assert_eq!(caps, expected);
}

#[test]
fn apple_terminal_degrades_mouse_and_keyboard() {
    let ctx = TerminalContext::probe(&apple_env(), true);
    assert_eq!(ctx.brand, TerminalName::AppleTerminal);
    let caps = ctx.resolve(ColorMode::Ansi256);
    assert!(
        !caps.mouse_capture,
        "mouse leaks as raw text on Apple Terminal"
    );
    assert!(
        !caps.osc52_clipboard,
        "Apple Terminal is not OSC52-allowlisted"
    );
    assert_eq!(
        caps.keyboard_mode,
        harness_tui::terminal::KeyboardMode::Legacy
    );
    assert!(
        caps.bracketed_paste,
        "bracketed paste still available on a TTY"
    );
}

#[test]
fn mouse_leak_brands_disable_mouse_capture() {
    for brand in [
        TerminalName::Unknown,
        TerminalName::Otty,
        TerminalName::JetBrains,
    ] {
        let ctx = TerminalContext {
            brand,
            multiplexer: TerminalMultiplexer::Undetected,
            alt_screen: harness_tui::terminal::lifecycle::AltScreenMode::Auto,
            is_tty: true,
            is_byobu: false,
        };
        assert!(
            ctx.mouse_reporting_leaks_as_raw_text(),
            "{brand:?} should leak"
        );
        assert!(
            !ctx.resolve(ColorMode::Ansi256).mouse_capture,
            "{brand:?} must disable capture"
        );
    }
}

#[test]
fn multiplexer_disables_focus_reporting_and_auto_alt_screen() {
    let env = TerminalEnv {
        term_program: Some("ghostty".to_string()),
        tmux: Some("/tmp/tmux-1000/default".to_string()),
        ..TerminalEnv::default()
    };
    let ctx = TerminalContext::probe(&env, true);
    assert!(ctx.repaints_pane_out_of_band(), "tmux repaints out of band");
    let caps = ctx.resolve(ColorMode::Truecolor);
    assert!(
        !caps.focus_reporting,
        "focus reporting degraded under multiplexer"
    );
    assert!(
        !caps.alternate_screen,
        "Auto alt-screen disabled under multiplexer"
    );
}

#[test]
fn alt_screen_always_engages_even_under_multiplexer() {
    let env = TerminalEnv {
        term_program: Some("ghostty".to_string()),
        tmux: Some("/tmp/tmux".to_string()),
        ..TerminalEnv::default()
    };
    let mut ctx = TerminalContext::probe(&env, true);
    ctx.alt_screen = harness_tui::terminal::lifecycle::AltScreenMode::Always;
    assert!(ctx.resolve(ColorMode::Truecolor).alternate_screen);

    ctx.alt_screen = harness_tui::terminal::lifecycle::AltScreenMode::Never;
    assert!(!ctx.resolve(ColorMode::Truecolor).alternate_screen);
}

#[test]
fn non_tty_disables_interactive_features() {
    let caps = terminal_capability_fallback(&ghostty_env(), ColorMode::Truecolor, false);
    assert!(!caps.mouse_capture);
    assert!(!caps.bracketed_paste);
    assert!(!caps.osc52_clipboard);
    assert!(!caps.alternate_screen);
    assert!(!caps.focus_reporting);
}

#[test]
fn ctrl_dot_unreliable_for_vscode_family_and_apple() {
    for brand in [
        TerminalName::VsCode,
        TerminalName::Cursor,
        TerminalName::Windsurf,
        TerminalName::AppleTerminal,
    ] {
        let ctx = TerminalContext::probe(
            &TerminalEnv {
                term_program: None,
                ..TerminalEnv::default()
            },
            true,
        );
        let ctx = TerminalContext { brand, ..ctx };
        assert!(
            ctx.ctrl_dot_unreliable(),
            "{brand:?} expected unreliable Ctrl+."
        );
    }
    let ghostty = TerminalContext::probe(&ghostty_env(), true);
    assert!(!ghostty.ctrl_dot_unreliable());
}

#[test]
fn csi_queries_unavailable_under_interceptor_or_multiplexer() {
    let jetbrains = TerminalContext::probe(
        &TerminalEnv {
            terminal_emulator: Some("JetBrains-JediTerm".to_string()),
            ..TerminalEnv::default()
        },
        true,
    );
    assert!(!jetbrains.csi_queries_available());

    let tmux = TerminalContext::probe(
        &TerminalEnv {
            term_program: Some("ghostty".to_string()),
            tmux: Some("/tmp/tmux".to_string()),
            ..TerminalEnv::default()
        },
        true,
    );
    assert!(!tmux.csi_queries_available());

    let ghostty = TerminalContext::probe(&ghostty_env(), true);
    assert!(ghostty.csi_queries_available());
}

#[test]
fn ime_as_bracketed_paste_conditional_is_brand_specific() {
    assert!(TerminalName::Vte.delivers_ime_as_bracketed_paste());
    assert!(TerminalName::AppleTerminal.delivers_ime_as_bracketed_paste());
    assert!(TerminalName::JetBrains.delivers_ime_as_bracketed_paste());
    assert!(!TerminalName::Ghostty.delivers_ime_as_bracketed_paste());
    assert!(!TerminalName::Kitty.delivers_ime_as_bracketed_paste());
}

#[test]
fn probe_records_byobu_flag() {
    let env = TerminalEnv {
        byobu: Some("1".to_string()),
        ..TerminalEnv::default()
    };
    let ctx = TerminalContext::probe(&env, true);
    assert!(ctx.byobu());
    assert!(ctx.is_tmux_backed(), "byobu defaults to a tmux backend");
}
