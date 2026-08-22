//! Settings list overlay backed by `settings_registry()` with project-file edit paths.
//!
//! Writable when a project runtime config path is bound:
//! - `hashline_edit`
//! - `runtime.compaction.enabled`
//! - `runtime.compaction.auto_retry_overflow`
//! - `runtime.compaction.structured_summary_contract`
//! - `runtime.compaction.estimated_token_triggers`
//! - `runtime.deterministic.enabled`
//!
//! Secrets fail closed. Runtime/TUI surfaces stay separate.

use std::path::{Path, PathBuf};

use harness_core::config::{
    reset_project_compaction_auto_retry_overflow, reset_project_compaction_enabled,
    reset_project_compaction_estimated_token_triggers,
    reset_project_compaction_structured_summary_contract, reset_project_deterministic_enabled,
    reset_project_hashline_edit, setting_definition, settings_registry,
    write_project_compaction_auto_retry_overflow, write_project_compaction_enabled,
    write_project_compaction_estimated_token_triggers,
    write_project_compaction_structured_summary_contract, write_project_deterministic_enabled,
    write_project_hashline_edit, SettingDefinition, SettingSensitivity, SettingWriteError,
};

use super::{AppState, ToastVariant};

/// One row in the settings editor (registry-bound; effective value when known).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsEditorRow {
    pub setting_id: String,
    pub sensitivity: String,
    pub surface: String,
    pub effective_value: Option<String>,
    pub editable: bool,
    pub selected: bool,
}

/// Operator-facing counts for the settings editor overlay (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsEditorSummary {
    pub total: usize,
    pub editable: usize,
    pub read_only: usize,
    pub secret: usize,
    pub bound: bool,
    pub writable_paths: usize,
    pub with_effective_value: usize,
}

impl SettingsEditorSummary {
    pub fn one_line(&self) -> String {
        format!(
            "settings editor: {} total (editable={}, read_only={}, secret={}, bound={}, writable_paths={}, with_effective={})",
            self.total,
            self.editable,
            self.read_only,
            self.secret,
            self.bound,
            self.writable_paths,
            self.with_effective_value
        )
    }

    /// Compact overlay subtitle for the settings list header.
    pub fn overlay_line(&self) -> String {
        if self.bound {
            format!(
                "{} total · {}/{} editable · {} secret · bound",
                self.total, self.editable, self.writable_paths, self.secret
            )
        } else {
            format!(
                "{} total · {} write paths · {} secret · unbound",
                self.total, self.writable_paths, self.secret
            )
        }
    }

    pub const fn has_editable(&self) -> bool {
        self.editable > 0
    }
}

const HASHLINE_EDIT_ID: &str = "hashline_edit";
const COMPACTION_ENABLED_ID: &str = "runtime.compaction.enabled";
const COMPACTION_AUTO_RETRY_OVERFLOW_ID: &str = "runtime.compaction.auto_retry_overflow";
const COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID: &str =
    "runtime.compaction.structured_summary_contract";
const COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID: &str = "runtime.compaction.estimated_token_triggers";
const DETERMINISTIC_ENABLED_ID: &str = "runtime.deterministic.enabled";

fn is_writable_setting(setting_id: &str) -> bool {
    matches!(
        setting_id,
        HASHLINE_EDIT_ID
            | COMPACTION_ENABLED_ID
            | COMPACTION_AUTO_RETRY_OVERFLOW_ID
            | COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID
            | COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID
            | DETERMINISTIC_ENABLED_ID
    )
}

impl AppState {
    pub(in crate::app) fn open_settings_editor(&mut self) {
        self.settings_editor_visible = true;
        self.settings_editor_selected = 0;
        self.theme_dialog_visible = false;
        self.error_details_visible = false;
        self.prompt_stash.list_visible = false;
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.toggles_menu_visible = false;
        self.lineage_browser_visible = false;
        self.fork_selector_visible = false;
    }

    pub(in crate::app) fn close_settings_editor(&mut self) {
        self.settings_editor_visible = false;
    }

    pub fn bind_settings_project_config(
        &mut self,
        path: impl Into<PathBuf>,
        hashline_edit: bool,
        compaction_enabled: bool,
        compaction_auto_retry_overflow: bool,
        compaction_structured_summary_contract: bool,
        compaction_estimated_token_triggers: bool,
        deterministic_enabled: bool,
    ) {
        self.settings_project_config_path = Some(path.into());
        self.settings_hashline_edit = hashline_edit;
        self.settings_compaction_enabled = compaction_enabled;
        self.settings_compaction_auto_retry_overflow = compaction_auto_retry_overflow;
        self.settings_compaction_structured_summary_contract =
            compaction_structured_summary_contract;
        self.settings_compaction_estimated_token_triggers = compaction_estimated_token_triggers;
        self.settings_deterministic_enabled = deterministic_enabled;
    }

    pub fn settings_project_config_path(&self) -> Option<&Path> {
        self.settings_project_config_path.as_deref()
    }

    pub fn settings_hashline_edit(&self) -> bool {
        self.settings_hashline_edit
    }

    pub fn settings_compaction_enabled(&self) -> bool {
        self.settings_compaction_enabled
    }

    pub fn settings_compaction_auto_retry_overflow(&self) -> bool {
        self.settings_compaction_auto_retry_overflow
    }

    pub fn settings_compaction_structured_summary_contract(&self) -> bool {
        self.settings_compaction_structured_summary_contract
    }

    pub fn settings_compaction_estimated_token_triggers(&self) -> bool {
        self.settings_compaction_estimated_token_triggers
    }

    pub fn settings_deterministic_enabled(&self) -> bool {
        self.settings_deterministic_enabled
    }

    pub(in crate::app) fn settings_editor_move(&mut self, delta: isize) {
        let len = settings_registry().len();
        if len == 0 {
            self.settings_editor_selected = 0;
            return;
        }
        let current = isize::try_from(self.settings_editor_selected.min(len - 1)).unwrap_or(0);
        let next = (current + delta).clamp(0, isize::try_from(len - 1).unwrap_or(0));
        self.settings_editor_selected = usize::try_from(next).unwrap_or(0);
    }

    pub fn settings_editor_rows(&self) -> Vec<SettingsEditorRow> {
        let selected = self.settings_editor_selected;
        let bound = self.settings_project_config_path.is_some();
        settings_registry()
            .iter()
            .enumerate()
            .map(|(index, def)| {
                let id = def.setting_id.as_str();
                SettingsEditorRow {
                    setting_id: id.to_string(),
                    sensitivity: sensitivity_label(def.sensitivity).to_string(),
                    surface: surface_label(def).to_string(),
                    effective_value: self.effective_value_for(id),
                    editable: def.is_editable() && is_writable_setting(id) && bound,
                    selected: index == selected,
                }
            })
            .collect()
    }

    pub fn settings_editor_selected_index(&self) -> usize {
        self.settings_editor_selected
    }

    pub fn settings_editor_is_visible(&self) -> bool {
        self.settings_editor_visible
    }

    pub fn settings_editor_selected_id(&self) -> Option<&'static str> {
        settings_registry()
            .get(self.settings_editor_selected)
            .map(|entry| entry.setting_id.as_str())
    }

    pub fn settings_editor_summary(&self) -> SettingsEditorSummary {
        let rows = self.settings_editor_rows();
        let mut summary = SettingsEditorSummary {
            total: rows.len(),
            bound: self.settings_project_config_path.is_some(),
            ..SettingsEditorSummary::default()
        };
        for row in &rows {
            if row.editable {
                summary.editable = summary.editable.saturating_add(1);
            } else {
                summary.read_only = summary.read_only.saturating_add(1);
            }
            if row.sensitivity == "secret" {
                summary.secret = summary.secret.saturating_add(1);
            }
            if is_writable_setting(&row.setting_id) {
                summary.writable_paths = summary.writable_paths.saturating_add(1);
            }
            if row.effective_value.is_some() {
                summary.with_effective_value = summary.with_effective_value.saturating_add(1);
            }
        }
        summary
    }

    pub fn settings_editor_activate_selected(&mut self) {
        let Some(setting_id) = self.settings_editor_selected_id() else {
            return;
        };
        if let Some(def) = setting_definition(setting_id) {
            if def.is_secret() || matches!(def.sensitivity, SettingSensitivity::Secret) {
                self.show_toast(
                    format!("secret setting `{setting_id}` cannot be edited"),
                    ToastVariant::Error,
                );
                return;
            }
        }
        if !is_writable_setting(setting_id) {
            self.show_toast(
                format!("setting `{setting_id}` is not editable in this editor yet"),
                ToastVariant::Info,
            );
            return;
        }
        let Some(path) = self.settings_project_config_path.clone() else {
            self.show_toast(
                "no project config bound for settings write",
                ToastVariant::Error,
            );
            return;
        };
        match setting_id {
            HASHLINE_EDIT_ID => {
                let next = !self.settings_hashline_edit;
                match write_project_hashline_edit(&path, next) {
                    Ok(effective) => {
                        self.settings_hashline_edit = effective;
                        self.show_toast(
                            format!("hashline_edit set to {effective}"),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            COMPACTION_ENABLED_ID => {
                let next = !self.settings_compaction_enabled;
                match write_project_compaction_enabled(&path, next) {
                    Ok(effective) => {
                        self.settings_compaction_enabled = effective;
                        self.show_toast(
                            format!("runtime.compaction.enabled set to {effective}"),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            COMPACTION_AUTO_RETRY_OVERFLOW_ID => {
                let next = !self.settings_compaction_auto_retry_overflow;
                match write_project_compaction_auto_retry_overflow(&path, next) {
                    Ok(effective) => {
                        self.settings_compaction_auto_retry_overflow = effective;
                        self.show_toast(
                            format!("runtime.compaction.auto_retry_overflow set to {effective}"),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID => {
                let next = !self.settings_compaction_structured_summary_contract;
                match write_project_compaction_structured_summary_contract(&path, next) {
                    Ok(effective) => {
                        self.settings_compaction_structured_summary_contract = effective;
                        self.show_toast(
                            format!(
                                "runtime.compaction.structured_summary_contract set to {effective}"
                            ),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID => {
                let next = !self.settings_compaction_estimated_token_triggers;
                match write_project_compaction_estimated_token_triggers(&path, next) {
                    Ok(effective) => {
                        self.settings_compaction_estimated_token_triggers = effective;
                        self.show_toast(
                            format!(
                                "runtime.compaction.estimated_token_triggers set to {effective}"
                            ),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            DETERMINISTIC_ENABLED_ID => {
                let next = !self.settings_deterministic_enabled;
                match write_project_deterministic_enabled(&path, next) {
                    Ok(effective) => {
                        self.settings_deterministic_enabled = effective;
                        self.show_toast(
                            format!("runtime.deterministic.enabled set to {effective}"),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            _ => {}
        }
    }

    pub fn settings_editor_reset_selected(&mut self) {
        let Some(setting_id) = self.settings_editor_selected_id() else {
            return;
        };
        if let Some(def) = setting_definition(setting_id) {
            if def.is_secret() {
                self.show_toast(
                    format!("secret setting `{setting_id}` cannot be reset"),
                    ToastVariant::Error,
                );
                return;
            }
        }
        if !is_writable_setting(setting_id) {
            self.show_toast(
                format!("reset not supported for `{setting_id}` yet"),
                ToastVariant::Info,
            );
            return;
        }
        let Some(path) = self.settings_project_config_path.clone() else {
            self.show_toast(
                "no project config bound for settings reset",
                ToastVariant::Error,
            );
            return;
        };
        match setting_id {
            HASHLINE_EDIT_ID => match reset_project_hashline_edit(&path) {
                Ok(effective) => {
                    self.settings_hashline_edit = effective;
                    self.show_toast(
                        format!("hashline_edit reset to default ({effective})"),
                        ToastVariant::Info,
                    );
                }
                Err(err) => self.report_settings_write_error(err),
            },
            COMPACTION_ENABLED_ID => match reset_project_compaction_enabled(&path) {
                Ok(effective) => {
                    self.settings_compaction_enabled = effective;
                    self.show_toast(
                        format!("runtime.compaction.enabled reset to default ({effective})"),
                        ToastVariant::Info,
                    );
                }
                Err(err) => self.report_settings_write_error(err),
            },
            COMPACTION_AUTO_RETRY_OVERFLOW_ID => {
                match reset_project_compaction_auto_retry_overflow(&path) {
                    Ok(effective) => {
                        self.settings_compaction_auto_retry_overflow = effective;
                        self.show_toast(
                            format!(
                                "runtime.compaction.auto_retry_overflow reset to default ({effective})"
                            ),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID => {
                match reset_project_compaction_structured_summary_contract(&path) {
                    Ok(effective) => {
                        self.settings_compaction_structured_summary_contract = effective;
                        self.show_toast(
                            format!(
                                "runtime.compaction.structured_summary_contract reset to default ({effective})"
                            ),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID => {
                match reset_project_compaction_estimated_token_triggers(&path) {
                    Ok(effective) => {
                        self.settings_compaction_estimated_token_triggers = effective;
                        self.show_toast(
                            format!(
                                "runtime.compaction.estimated_token_triggers reset to default ({effective})"
                            ),
                            ToastVariant::Info,
                        );
                    }
                    Err(err) => self.report_settings_write_error(err),
                }
            }
            DETERMINISTIC_ENABLED_ID => match reset_project_deterministic_enabled(&path) {
                Ok(effective) => {
                    self.settings_deterministic_enabled = effective;
                    self.show_toast(
                        format!("runtime.deterministic.enabled reset to default ({effective})"),
                        ToastVariant::Info,
                    );
                }
                Err(err) => self.report_settings_write_error(err),
            },
            _ => {}
        }
    }

    fn effective_value_for(&self, setting_id: &str) -> Option<String> {
        self.settings_project_config_path.as_ref()?;
        match setting_id {
            HASHLINE_EDIT_ID => Some(bool_label(self.settings_hashline_edit)),
            COMPACTION_ENABLED_ID => Some(bool_label(self.settings_compaction_enabled)),
            COMPACTION_AUTO_RETRY_OVERFLOW_ID => {
                Some(bool_label(self.settings_compaction_auto_retry_overflow))
            }
            COMPACTION_STRUCTURED_SUMMARY_CONTRACT_ID => Some(bool_label(
                self.settings_compaction_structured_summary_contract,
            )),
            COMPACTION_ESTIMATED_TOKEN_TRIGGERS_ID => Some(bool_label(
                self.settings_compaction_estimated_token_triggers,
            )),
            DETERMINISTIC_ENABLED_ID => Some(bool_label(self.settings_deterministic_enabled)),
            _ => None,
        }
    }

    fn report_settings_write_error(&mut self, err: SettingWriteError) {
        let message = match err {
            SettingWriteError::SecretSetting(id) => {
                format!("secret setting `{id}` cannot be edited")
            }
            other => format!("settings write failed: {other}"),
        };
        self.show_toast(message, ToastVariant::Error);
    }
}

fn bool_label(value: bool) -> String {
    if value {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

fn sensitivity_label(sensitivity: SettingSensitivity) -> &'static str {
    match sensitivity {
        SettingSensitivity::Public => "public",
        SettingSensitivity::Redacted => "redacted",
        SettingSensitivity::Secret => "secret",
    }
}

fn surface_label(def: &SettingDefinition) -> &'static str {
    match def.surface {
        harness_core::config::SettingSurface::Runtime => "runtime",
        harness_core::config::SettingSurface::Tui => "tui",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Relocated from overlay_picker_settings_permission_consistency_test.rs: the
    // `is_writable_setting` helper is module-private; exercise it here without
    // widening visibility.
    #[test]
    fn settings_editor_writable_settings_identified() {
        // arrange
        // act
        // assert
        assert!(
            is_writable_setting("hashline_edit"),
            "hashline_edit must be writable"
        );
        assert!(
            is_writable_setting("runtime.compaction.enabled"),
            "runtime.compaction.enabled must be writable"
        );
        assert!(
            !is_writable_setting("model"),
            "model must not be writable in settings editor"
        );
    }
}
