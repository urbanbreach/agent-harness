use crossterm::event::{KeyCode, KeyModifiers, MediaKeyCode, ModifierKeyCode};
use harness_tui::keybindings::KeyBinding;

#[test]
fn frozen_shortcut_labels_match() {
    let mut codes = vec![
        KeyCode::Backspace, KeyCode::Enter, KeyCode::Left, KeyCode::Right,
        KeyCode::Up, KeyCode::Down, KeyCode::Home, KeyCode::End, KeyCode::PageUp,
        KeyCode::PageDown, KeyCode::Tab, KeyCode::BackTab, KeyCode::Delete,
        KeyCode::Insert, KeyCode::Null, KeyCode::Esc, KeyCode::CapsLock,
        KeyCode::ScrollLock, KeyCode::NumLock, KeyCode::PrintScreen, KeyCode::Pause,
        KeyCode::Menu, KeyCode::KeypadBegin,
    ];
    codes.extend((0..=255).map(KeyCode::F));
    codes.extend((0..=127).map(|value| KeyCode::Char(char::from(value))));
    codes.extend("é中🦀\u{301}\u{200d}\u{fe0f}\u{a0}\u{2028}\u{2029}\u{1f1eb}\u{10ffff}".chars().map(KeyCode::Char));
    codes.extend([
        MediaKeyCode::Play, MediaKeyCode::Pause, MediaKeyCode::PlayPause,
        MediaKeyCode::Reverse, MediaKeyCode::Stop, MediaKeyCode::FastForward,
        MediaKeyCode::Rewind, MediaKeyCode::TrackNext, MediaKeyCode::TrackPrevious,
        MediaKeyCode::Record, MediaKeyCode::LowerVolume, MediaKeyCode::RaiseVolume,
        MediaKeyCode::MuteVolume,
    ].map(KeyCode::Media));
    codes.extend([
        ModifierKeyCode::LeftShift, ModifierKeyCode::LeftControl,
        ModifierKeyCode::LeftAlt, ModifierKeyCode::LeftSuper,
        ModifierKeyCode::LeftHyper, ModifierKeyCode::LeftMeta,
        ModifierKeyCode::RightShift, ModifierKeyCode::RightControl,
        ModifierKeyCode::RightAlt, ModifierKeyCode::RightSuper,
        ModifierKeyCode::RightHyper, ModifierKeyCode::RightMeta,
        ModifierKeyCode::IsoLevel3Shift, ModifierKeyCode::IsoLevel5Shift,
    ].map(KeyCode::Modifier));
    let mut cases = 0;
    for code in codes {
        for bits in 0..=63 {
            let binding = KeyBinding::new(code, KeyModifiers::from_bits_retain(bits));
            assert_eq!(binding.to_string(), format_key_binding(&binding), "{binding:?}");
            cases += 1;
        }
    }
    println!("{cases} frozen shortcut labels matched");
}
