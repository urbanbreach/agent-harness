use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub fn matches(&self, event: &KeyEvent) -> bool {
        self.code == event.code && self.modifiers == event.modifiers
    }
}

impl FromStr for KeyBinding {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let parts = s.split('+').collect::<Vec<_>>();
        if parts.len() > 1 {
            let mut modifiers = KeyModifiers::NONE;
            for modifier in &parts[..parts.len() - 1] {
                match modifier.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                    "shift" => modifiers |= KeyModifiers::SHIFT,
                    "alt" | "option" => modifiers |= KeyModifiers::ALT,
                    other => return Err(format!("unknown modifier: {other}")),
                }
            }
            let key_part = parts[parts.len() - 1];
            let code = parse_key_code(key_part)?;
            return Ok(KeyBinding::new(code, modifiers));
        }

        let code = parse_key_code(s)?;
        Ok(KeyBinding::new(code, KeyModifiers::NONE))
    }
}

pub(super) fn format_key_binding(binding: &KeyBinding) -> String {
    format_key_binding_with_case(binding, ModifierCase::Title)
}

pub(super) fn format_key_binding_harness(binding: &KeyBinding) -> String {
    format_key_binding_with_case(binding, ModifierCase::Lower)
}

#[derive(Clone, Copy)]
enum ModifierCase {
    Title,
    Lower,
}

fn format_key_binding_with_case(binding: &KeyBinding, modifier_case: ModifierCase) -> String {
    let mut parts = Vec::new();

    if binding.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push(match modifier_case {
            ModifierCase::Title => "Ctrl".to_string(),
            ModifierCase::Lower => "ctrl".to_string(),
        });
    }
    if binding.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push(match modifier_case {
            ModifierCase::Title => "Shift".to_string(),
            ModifierCase::Lower => "shift".to_string(),
        });
    }
    if binding.modifiers.contains(KeyModifiers::ALT) {
        parts.push(match modifier_case {
            ModifierCase::Title => "Alt".to_string(),
            ModifierCase::Lower => "alt".to_string(),
        });
    }

    parts.push(format_key_code(binding.code, modifier_case));
    parts.join("+")
}

fn format_key_code(code: KeyCode, modifier_case: ModifierCase) -> String {
    match code {
        KeyCode::Char(' ') => match modifier_case {
            ModifierCase::Title => " ".to_string(),
            ModifierCase::Lower => "space".to_string(),
        },
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => match modifier_case {
            ModifierCase::Title => "Shift-Tab".to_string(),
            ModifierCase::Lower => "shift-tab".to_string(),
        },
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Del".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Insert => "Ins".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => format!("{code:?}"),
    }
}

fn parse_key_code(s: &str) -> Result<KeyCode, String> {
    match s.to_lowercase().as_str() {
        "tab" => Ok(KeyCode::Tab),
        "backtab" | "shift-tab" => Ok(KeyCode::BackTab),
        "enter" => Ok(KeyCode::Enter),
        "esc" | "escape" => Ok(KeyCode::Esc),
        "space" | " " => Ok(KeyCode::Char(' ')),
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        "backspace" => Ok(KeyCode::Backspace),
        "delete" | "del" => Ok(KeyCode::Delete),
        "home" => Ok(KeyCode::Home),
        "end" => Ok(KeyCode::End),
        "pageup" => Ok(KeyCode::PageUp),
        "pagedown" => Ok(KeyCode::PageDown),
        "insert" => Ok(KeyCode::Insert),
        "f1" => Ok(KeyCode::F(1)),
        "f2" => Ok(KeyCode::F(2)),
        "f3" => Ok(KeyCode::F(3)),
        "f4" => Ok(KeyCode::F(4)),
        "f5" => Ok(KeyCode::F(5)),
        "f6" => Ok(KeyCode::F(6)),
        "f7" => Ok(KeyCode::F(7)),
        "f8" => Ok(KeyCode::F(8)),
        "f9" => Ok(KeyCode::F(9)),
        "f10" => Ok(KeyCode::F(10)),
        "f11" => Ok(KeyCode::F(11)),
        "f12" => Ok(KeyCode::F(12)),
        single => {
            let mut chars = single.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(KeyCode::Char(c)),
                _ => Err(format!("unknown key: {single}")),
            }
        }
    }
}
