use crate::terminal::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyProtocol {
    ControlByte,
    EscapeAlt,
    Csi,
    Ss3,
    KittyCsiU,
    LegacyCsi27,
}

impl KeyProtocol {
    pub const fn all() -> &'static [Self; 6] {
        &KEY_PROTOCOLS
    }
}

pub const KEY_PROTOCOLS: [KeyProtocol; 6] = [
    KeyProtocol::ControlByte,
    KeyProtocol::EscapeAlt,
    KeyProtocol::Csi,
    KeyProtocol::Ss3,
    KeyProtocol::KittyCsiU,
    KeyProtocol::LegacyCsi27,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModifierVariant {
    pub wire: u16,
    pub modifiers: KeyModifiers,
}

impl ModifierVariant {
    pub const fn from_wire(wire: u16) -> Self {
        Self {
            wire,
            modifiers: KeyModifiers::from_xterm_param(wire),
        }
    }
}

pub const MODIFIER_VARIANTS: [ModifierVariant; 16] = [
    ModifierVariant::from_wire(1),
    ModifierVariant::from_wire(2),
    ModifierVariant::from_wire(3),
    ModifierVariant::from_wire(4),
    ModifierVariant::from_wire(5),
    ModifierVariant::from_wire(6),
    ModifierVariant::from_wire(7),
    ModifierVariant::from_wire(8),
    ModifierVariant::from_wire(9),
    ModifierVariant::from_wire(10),
    ModifierVariant::from_wire(11),
    ModifierVariant::from_wire(12),
    ModifierVariant::from_wire(13),
    ModifierVariant::from_wire(14),
    ModifierVariant::from_wire(15),
    ModifierVariant::from_wire(16),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedKey {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl NormalizedKey {
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub fn from_event(event: KeyEvent) -> Self {
        let code = match event.code {
            KeyCode::Char(character)
                if event.modifiers == KeyModifiers::CTRL && character.is_ascii_lowercase() =>
            {
                KeyCode::Char(character.to_ascii_uppercase())
            }
            code => code,
        };
        Self::new(code, event.modifiers)
    }

    pub const fn is_escape(self) -> bool {
        matches!(self.code, KeyCode::Esc)
    }

    pub const fn is_ctrl_c(self) -> bool {
        matches!(self.code, KeyCode::Char('C'))
            && self.modifiers.bits() == KeyModifiers::CTRL.bits()
    }
}

pub fn normalize_key(event: KeyEvent) -> NormalizedKey {
    NormalizedKey::from_event(event)
}
