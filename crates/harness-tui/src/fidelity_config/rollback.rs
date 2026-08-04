use super::contract::{FidelityConfig, InputMode, NotificationMode};
use crate::capability_matrix::{
    CapabilityMatrix, GraphicsCapability, KeyboardCapability, NotificationCapability,
    TitleCapability,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackDecision {
    KeepEnabled,
    ForceDisable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackToggles {
    pub inline_images: RollbackDecision,
    pub inline_video: RollbackDecision,
    pub terminal_title: RollbackDecision,
    pub native_notifications: RollbackDecision,
    pub modern_keyboard: RollbackDecision,
}

impl RollbackToggles {
    pub fn all_enabled() -> Self {
        Self {
            inline_images: RollbackDecision::KeepEnabled,
            inline_video: RollbackDecision::KeepEnabled,
            terminal_title: RollbackDecision::KeepEnabled,
            native_notifications: RollbackDecision::KeepEnabled,
            modern_keyboard: RollbackDecision::KeepEnabled,
        }
    }

    pub fn all_disabled(reason: &str) -> Self {
        let decision = || RollbackDecision::ForceDisable {
            reason: reason.to_string(),
        };
        Self {
            inline_images: decision(),
            inline_video: decision(),
            terminal_title: decision(),
            native_notifications: decision(),
            modern_keyboard: decision(),
        }
    }

    pub fn disable_risky(reason: &str) -> Self {
        let mut toggles = Self::all_enabled();
        let decision = RollbackDecision::ForceDisable {
            reason: reason.to_string(),
        };
        toggles.inline_video = decision.clone();
        toggles.native_notifications = decision;
        toggles
    }

    pub fn is_enabled(&self, feature: &str) -> bool {
        let decision = match feature {
            "inline_images" => &self.inline_images,
            "inline_video" => &self.inline_video,
            "terminal_title" => &self.terminal_title,
            "native_notifications" => &self.native_notifications,
            "modern_keyboard" => &self.modern_keyboard,
            _ => return false,
        };
        matches!(decision, RollbackDecision::KeepEnabled)
    }

    pub fn merge_with_config(&self, config: &FidelityConfig) -> FidelityConfig {
        let mut merged = config.clone();
        if !self.is_enabled("inline_images") {
            merged.inline_images = false;
        }
        if !self.is_enabled("inline_video") {
            merged.inline_video = false;
        }
        if !self.is_enabled("terminal_title") {
            merged.terminal_title = false;
        }
        if !self.is_enabled("native_notifications") {
            merged.notification = NotificationMode::Bell;
        }
        if !self.is_enabled("modern_keyboard") {
            merged.input_mode = InputMode::Legacy;
        }
        merged
    }

    pub fn from_capability_matrix(capability: &CapabilityMatrix) -> Self {
        let reason = "capability matrix lacks a common feature";
        let mut toggles = Self::all_enabled();
        if capability
            .cells()
            .iter()
            .any(|cell| cell.graphics == GraphicsCapability::None)
        {
            toggles.inline_images = RollbackDecision::ForceDisable {
                reason: reason.to_string(),
            };
            toggles.inline_video = RollbackDecision::ForceDisable {
                reason: reason.to_string(),
            };
        }
        if capability.cells().iter().any(|cell| {
            matches!(
                cell.notification,
                NotificationCapability::Bell | NotificationCapability::None
            )
        }) {
            toggles.native_notifications = RollbackDecision::ForceDisable {
                reason: reason.to_string(),
            };
        }
        if capability
            .cells()
            .iter()
            .any(|cell| cell.title == TitleCapability::Unsupported)
        {
            toggles.terminal_title = RollbackDecision::ForceDisable {
                reason: reason.to_string(),
            };
        }
        if capability
            .cells()
            .iter()
            .any(|cell| cell.keyboard == KeyboardCapability::Minimal)
        {
            toggles.modern_keyboard = RollbackDecision::ForceDisable {
                reason: reason.to_string(),
            };
        }
        toggles
    }
}
