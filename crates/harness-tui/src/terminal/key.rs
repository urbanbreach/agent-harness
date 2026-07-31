//! ANSI / xterm / Kitty keyboard sequence decoding.
//!
//! Pure functions that turn already-framed terminal tokens (a C0 control byte,
//! an `ESC O` SS3 byte, or a `CSI ... final` parameter list) into a
//! [`KeyEvent`]. Escape framing is owned by [`super::decode`]; this module owns
//! the key semantics only.

use super::event::{KeyCode, KeyEvent, KeyModifiers};

/// Decode a C0 control byte (0x00..=0x1F and DEL 0x7F) into a key event.
///
/// Printable Ctrl-letter combinations follow the classic mapping (`0x01` →
/// Ctrl+A … `0x1A` → Ctrl+Z); `TAB`, `LF`, and `CR` collapse onto their
/// dedicated key codes.
pub fn key_from_control_byte(byte: u8) -> KeyEvent {
    match byte {
        0x00 => KeyEvent::plain(KeyCode::Null),
        0x08 => KeyEvent::plain(KeyCode::Backspace),
        0x09 => KeyEvent::plain(KeyCode::Tab),
        0x0A => KeyEvent::plain(KeyCode::Enter),
        0x0D => KeyEvent::plain(KeyCode::Enter),
        0x1B => KeyEvent::plain(KeyCode::Esc),
        0x7F => KeyEvent::plain(KeyCode::Backspace),
        // Ctrl+\ (0x1C) … Ctrl+_ (0x1F): the letter is byte + 0x40.
        0x1C..=0x1F => ctrl_letter(byte),
        // Ctrl+A .. Ctrl+Z (already excluding TAB/LF/CR/ESC handled above).
        other => ctrl_letter(other),
    }
}

fn ctrl_letter(byte: u8) -> KeyEvent {
    let letter = char::from(byte + 0x40);
    KeyEvent::new(KeyCode::Char(letter), KeyModifiers::CTRL)
}

/// Decode an `ESC O <byte>` (SS3) payload, e.g. `SS3 A` → Up, `SS3 P` → F1.
pub fn key_from_ss3(byte: u8) -> Option<KeyEvent> {
    Some(match byte {
        b'A' => KeyEvent::plain(KeyCode::Up),
        b'B' => KeyEvent::plain(KeyCode::Down),
        b'C' => KeyEvent::plain(KeyCode::Right),
        b'D' => KeyEvent::plain(KeyCode::Left),
        b'H' => KeyEvent::plain(KeyCode::Home),
        b'F' => KeyEvent::plain(KeyCode::End),
        b'M' => KeyEvent::plain(KeyCode::Enter),
        b'P' => KeyEvent::plain(KeyCode::F(1)),
        b'Q' => KeyEvent::plain(KeyCode::F(2)),
        b'R' => KeyEvent::plain(KeyCode::F(3)),
        b'S' => KeyEvent::plain(KeyCode::F(4)),
        _ => return None,
    })
}

/// Decode a `CSI <params> <final>` payload into a key event.
///
/// Returns `None` when the final byte is not a key (e.g. mouse SGR `M`/`m`,
/// focus `I`/`O`, paste `~` handled upstream).
pub fn key_from_csi(params: &[u16], final_byte: u8) -> Option<KeyEvent> {
    match final_byte {
        b'A' => Some(KeyEvent::new(KeyCode::Up, modifier(params))),
        b'B' => Some(KeyEvent::new(KeyCode::Down, modifier(params))),
        b'C' => Some(KeyEvent::new(KeyCode::Right, modifier(params))),
        b'D' => Some(KeyEvent::new(KeyCode::Left, modifier(params))),
        b'H' => Some(KeyEvent::new(KeyCode::Home, modifier(params))),
        b'F' => Some(KeyEvent::new(KeyCode::End, modifier(params))),
        b'P' => Some(KeyEvent::new(KeyCode::F(1), modifier(params))),
        b'Q' => Some(KeyEvent::new(KeyCode::F(2), modifier(params))),
        b'R' => Some(KeyEvent::new(KeyCode::F(3), modifier(params))),
        b'S' => Some(KeyEvent::new(KeyCode::F(4), modifier(params))),
        // Shift+Tab / BackTab. Inherently shifted; keep any extra modifier.
        b'Z' => Some(KeyEvent::new(
            KeyCode::Tab,
            modifier(params).union(KeyModifiers::SHIFT),
        )),
        b'u' => decode_csi_u(params),
        b'~' => decode_tilde(params),
        _ => None,
    }
}

fn modifier(params: &[u16]) -> KeyModifiers {
    KeyModifiers::from_xterm_param(params.get(1).copied().unwrap_or(1))
}

/// Decode the Kitty/fixterms `CSI <codepoint> ; <mod> u` payload.
fn decode_csi_u(params: &[u16]) -> Option<KeyEvent> {
    let codepoint = u32::from(*params.first()?);
    let modifiers = modifier(params);
    let code = codepoint_to_code(codepoint)?;
    Some(KeyEvent::new(code, modifiers))
}

/// Decode a `CSI <code> [; <mod>] ~` payload using the vt220 tilde table.
fn decode_tilde(params: &[u16]) -> Option<KeyEvent> {
    let code = *params.first()?;
    let modifiers = modifier(params);
    // Legacy "CSI 27 ; mod ; codepoint ~" carries the key as a codepoint.
    if code == 27 {
        let cp = u32::from(*params.get(2)?);
        let code = codepoint_to_code(cp)?;
        return Some(KeyEvent::new(code, modifier_from(params.get(1))));
    }
    let mapped = key_code_for_special_code(u32::from(code))?;
    Some(KeyEvent::new(mapped, modifiers))
}

fn modifier_from(param: Option<&u16>) -> KeyModifiers {
    KeyModifiers::from_xterm_param(param.copied().unwrap_or(1))
}

fn codepoint_to_code(codepoint: u32) -> Option<KeyCode> {
    Some(match codepoint {
        13 => KeyCode::Enter,
        27 => KeyCode::Esc,
        9 => KeyCode::Tab,
        8 | 127 => KeyCode::Backspace,
        cp => KeyCode::Char(char::from_u32(cp)?),
    })
}

fn key_code_for_special_code(code: u32) -> Option<KeyCode> {
    Some(match code {
        1 | 7 => KeyCode::Home,
        2 => KeyCode::Insert,
        3 => KeyCode::Delete,
        4 | 8 => KeyCode::End,
        5 => KeyCode::PageUp,
        6 => KeyCode::PageDown,
        11 => KeyCode::F(1),
        12 => KeyCode::F(2),
        13 => KeyCode::F(3),
        14 => KeyCode::F(4),
        15 => KeyCode::F(5),
        17 => KeyCode::F(6),
        18 => KeyCode::F(7),
        19 => KeyCode::F(8),
        20 => KeyCode::F(9),
        21 => KeyCode::F(10),
        23 => KeyCode::F(11),
        24 => KeyCode::F(12),
        25 => KeyCode::F(13),
        26 => KeyCode::F(14),
        28 => KeyCode::F(15),
        29 => KeyCode::F(16),
        31 => KeyCode::F(17),
        32 => KeyCode::F(18),
        33 => KeyCode::F(19),
        34 => KeyCode::F(20),
        _ => return None,
    })
}
