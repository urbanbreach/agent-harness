//! Light and dark theme family variants resolving design-contract roles to truecolor.

use crate::design_contract::ColorRole;
use crate::design_contract::DESIGN_TOKENS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeFamily {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FamilyColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl FamilyColor {
    pub fn rgb(&self) -> (u8, u8, u8) {
        (self.red, self.green, self.blue)
    }
}

impl ThemeFamily {
    pub fn all() -> [Self; 2] {
        [Self::Dark, Self::Light]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub fn resolve(&self, role: ColorRole) -> FamilyColor {
        let (red, green, blue) = dark_rgb(role);
        if matches!(self, Self::Light) && is_background_role(role) {
            return FamilyColor {
                red: 255 - red,
                green: 255 - green,
                blue: 255 - blue,
            };
        }
        FamilyColor { red, green, blue }
    }

    pub fn resolve_all(&self) -> Vec<(ColorRole, FamilyColor)> {
        ColorRole::ALL
            .iter()
            .copied()
            .map(|role| (role, self.resolve(role)))
            .collect()
    }
}

fn dark_rgb(role: ColorRole) -> (u8, u8, u8) {
    for token in DESIGN_TOKENS.palette.roles.iter() {
        if token.role == role {
            return (token.value.red, token.value.green, token.value.blue);
        }
    }
    (0, 0, 0)
}

fn is_background_role(role: ColorRole) -> bool {
    matches!(
        role,
        ColorRole::Canvas
            | ColorRole::Shell
            | ColorRole::Panel
            | ColorRole::PanelElevated
            | ColorRole::Overlay
            | ColorRole::Card
            | ColorRole::SelectedCard
            | ColorRole::QuestionSurface
            | ColorRole::QuestionSelected
    )
}

impl std::fmt::Display for ThemeFamily {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}
