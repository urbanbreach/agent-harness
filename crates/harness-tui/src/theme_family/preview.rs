use std::collections::BTreeMap;

use crate::app::theme_preview::SystemAppearance;

use super::auto::{ThemeChoice, ThemeEnvironment};
use super::fallback::ResolvedTheme;
use super::persist::store_theme_choice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePreviewStatus {
    Committed,
    Previewing,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePreviewState {
    committed: ThemeChoice,
    preview: Option<ThemeChoice>,
    environment: ThemeEnvironment,
    status: ThemePreviewStatus,
}

impl ThemePreviewState {
    pub fn new(committed: ThemeChoice, environment: ThemeEnvironment) -> Self {
        Self {
            committed,
            preview: None,
            environment,
            status: ThemePreviewStatus::Committed,
        }
    }

    pub const fn committed_choice(&self) -> ThemeChoice {
        self.committed
    }

    pub const fn preview_choice(&self) -> Option<ThemeChoice> {
        self.preview
    }

    pub const fn status(&self) -> ThemePreviewStatus {
        self.status
    }

    pub const fn effective_choice(&self) -> ThemeChoice {
        match self.preview {
            Some(choice) => choice,
            None => self.committed,
        }
    }

    pub fn effective_theme(&self) -> ResolvedTheme {
        self.effective_choice().resolve(&self.environment)
    }

    pub const fn preview(&mut self, choice: ThemeChoice) {
        self.preview = Some(choice);
        self.status = ThemePreviewStatus::Previewing;
    }

    pub const fn cancel(&mut self) -> ThemeChoice {
        self.preview = None;
        self.status = ThemePreviewStatus::Cancelled;
        self.committed
    }

    pub const fn commit(&mut self) -> ThemeChoice {
        if let Some(choice) = self.preview {
            self.committed = choice;
            self.preview = None;
        }
        self.status = ThemePreviewStatus::Committed;
        self.committed
    }

    pub fn commit_to_keybindings(&mut self, keybindings: &mut BTreeMap<String, String>) {
        store_theme_choice(keybindings, self.commit());
    }

    pub fn on_system_appearance_change(&mut self, appearance: SystemAppearance) {
        self.environment.appearance = Some(appearance);
    }

    pub fn set_environment(&mut self, environment: ThemeEnvironment) {
        self.environment = environment;
    }

    pub const fn environment(&self) -> &ThemeEnvironment {
        &self.environment
    }
}
