//! Per-model compaction threshold editor, opened from the model switcher.
//!
//! The input grammar is the typed parser `policy::parse_override`: `80%`, `f150000`,
//! `200K` (a base window), an empty line resets. Validation, planning and the preview come
//! from the pure helpers in `harness_compaction::policy`; this module owns the interaction and
//! the write to `runtime.compaction.model_thresholds`.

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_compaction::policy::{
    format_override, match_model_key, parse_override, plan_override, preview, OverrideWarning,
    PreviewOrigin, ThresholdOverride, ThresholdSettings,
};
use harness_core::config::{read_project_setting_value, write_project_setting_value};
use serde_json::{Map, Value};
use unicode_segmentation::UnicodeSegmentation;

use super::AppState;
use crate::app::ToastVariant;
use crate::composer_editing::{ComposerEditor, DeleteKind};

const THRESHOLDS_ID: &str = "runtime.compaction.model_thresholds";
const THRESHOLDS_ENABLED_ID: &str = "runtime.compaction.model_thresholds_enabled";
const PERCENT_ID: &str = "runtime.compaction.threshold_percent";
const TOKENS_ID: &str = "runtime.compaction.threshold_tokens";
const BASE_WINDOW_ID: &str = "runtime.compaction.base_window_tokens";
const RESERVE_ID: &str = "runtime.compaction.reserve_tokens";

/// Open editor state for one model.
pub(crate) struct ThresholdEditor {
    provider: String,
    model: String,
    window: Option<u32>,
    editor: ComposerEditor,
    /// The authored `model_thresholds` map at the time the editor opened.
    project: Map<String, Value>,
    /// Failure of the last save attempt.
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThresholdLineTone {
    Plain,
    Warning,
    Error,
}

/// Everything the overlay shows for the open editor.
pub(crate) struct ThresholdEditorView {
    pub(crate) title: String,
    pub(crate) input: String,
    /// Byte offset of the cursor in `input`.
    pub(crate) cursor: usize,
    pub(crate) selected: bool,
    pub(crate) lines: Vec<(ThresholdLineTone, String)>,
}

impl ThresholdEditor {
    fn key(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }

    fn overrides(&self) -> BTreeMap<String, ThresholdOverride> {
        self.project
            .iter()
            .filter_map(|(key, value)| Some((key.clone(), override_from_value(value)?)))
            .collect()
    }

    /// The map editing plans against: saving drops the colon spelling of this model, so it
    /// no longer counts as something that "still applies".
    fn plan_models(&self) -> BTreeMap<String, ThresholdOverride> {
        let alias = format!("{}:{}", self.provider, self.model);
        let mut models = self.overrides();
        models.remove(&alias);
        models
    }

    fn handle_key(&mut self, key: &KeyEvent) {
        self.error = None;
        match key.code {
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.editor.select_all();
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let _ = self.editor.insert_text(&c.to_string());
            }
            KeyCode::Backspace => {
                let _ = self.editor.backspace();
            }
            KeyCode::Delete => {
                let _ = self.editor.delete(DeleteKind::CharacterForward);
            }
            KeyCode::Left => self.editor.move_left(),
            KeyCode::Right => self.editor.move_right(),
            KeyCode::Home => self.editor.move_line_start(),
            KeyCode::End => self.editor.move_line_end(),
            _ => {}
        }
    }
}

/// A persisted entry as the typed override it means. Bare numbers are base windows.
fn override_from_value(value: &Value) -> Option<ThresholdOverride> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|tokens| u32::try_from(tokens).ok())
            .filter(|tokens| *tokens > 0)
            .map(ThresholdOverride::BaseWindow),
        Value::String(text) => parse_override(text, true).ok(),
        Value::Object(fields) if fields.len() == 1 => {
            let tokens = |value: &Value| {
                value
                    .as_u64()
                    .and_then(|tokens| u32::try_from(tokens).ok())
                    .filter(|tokens| *tokens > 0)
            };
            match fields.iter().next()? {
                (name, value) if name == "tokens" => tokens(value).map(ThresholdOverride::Fixed),
                (name, value) if name == "base" => tokens(value).map(ThresholdOverride::BaseWindow),
                _ => None,
            }
        }
        _ => None,
    }
}

fn warning_text(warning: OverrideWarning) -> &'static str {
    match warning {
        OverrideWarning::BaseNotBelowWindow => {
            "The base window is not below the model window, so it changes nothing."
        }
    }
}

impl AppState {
    pub(in crate::app) fn threshold_editor_open(&self) -> bool {
        self.threshold_editor.is_some()
    }

    fn threshold_settings(&self) -> ThresholdSettings {
        let tokens = |id: &str| {
            self.compaction_setting(id)
                .and_then(|value| value.trim().parse::<u32>().ok())
                .filter(|value| *value > 0)
        };
        ThresholdSettings {
            threshold_percent: self
                .compaction_setting(PERCENT_ID)
                .and_then(|value| value.trim().parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value > 0.0),
            threshold_tokens: tokens(TOKENS_ID),
            base_window_tokens: tokens(BASE_WINDOW_ID),
            reserve_tokens: tokens(RESERVE_ID),
        }
    }

    fn model_thresholds_enabled(&self) -> bool {
        self.compaction_setting(THRESHOLDS_ENABLED_ID) != Some("false")
    }

    /// Open the editor for the model highlighted in the switcher.
    pub(in crate::app) fn open_threshold_editor(&mut self) {
        let Some(option) = self
            .model_filtered
            .get(self.model_selected)
            .and_then(|index| self.model_options.get(*index))
            .cloned()
        else {
            return;
        };
        let option = if option.provider == self.launch_metadata.provider()
            && Some(option.model.as_str()) == self.launch_metadata.model()
        {
            self.launch_metadata.to_model_option().unwrap_or(option)
        } else {
            option
        };
        let Some(path) = self.settings_project_config_path.clone() else {
            self.show_toast(
                "no project config bound for settings write",
                ToastVariant::Error,
            );
            return;
        };
        let project = match read_project_setting_value(&path, THRESHOLDS_ID) {
            Ok(None) => Map::new(),
            Ok(Some(text)) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(map)) => map,
                _ => {
                    self.show_toast(
                        "model thresholds in the project config are not an object",
                        ToastVariant::Error,
                    );
                    return;
                }
            },
            Err(error) => {
                self.show_toast(
                    format!("settings read failed: {error}"),
                    ToastVariant::Error,
                );
                return;
            }
        };
        let mut editor = ThresholdEditor {
            provider: option.provider.clone(),
            model: option.model.clone(),
            window: option.model_limits.context_window.tokens.filter(|w| *w > 0),
            editor: ComposerEditor::default(),
            project,
            error: None,
        };
        // Prefill only an entry that names this model; a wildcard stays inherited.
        let overrides = editor.overrides();
        let exact = [
            editor.key(),
            format!("{}:{}", editor.provider, editor.model),
        ]
        .into_iter()
        .find_map(|key| overrides.get(&key).copied());
        if let Some(existing) = exact {
            let _ = editor.editor.insert_text(&format_override(&existing));
        }
        self.threshold_editor = Some(editor);
        self.modal_interaction.invalidate();
    }

    pub(in crate::app) fn handle_threshold_editor_key(&mut self, key: &KeyEvent) -> bool {
        let Some(mut editor) = self.threshold_editor.take() else {
            return false;
        };
        match key.code {
            KeyCode::Esc => {
                self.modal_interaction.invalidate();
                return true;
            }
            KeyCode::Enter => {
                if self.save_threshold_override(&mut editor) {
                    self.modal_interaction.invalidate();
                    return true;
                }
            }
            _ => editor.handle_key(key),
        }
        self.threshold_editor = Some(editor);
        true
    }

    pub(in crate::app) fn handle_threshold_editor_paste(&mut self, text: &str) {
        if let Some(editor) = self.threshold_editor.as_mut() {
            editor.error = None;
            let text: String = text.chars().filter(|c| !c.is_control()).collect();
            let _ = editor.editor.insert_text(&text);
            self.modal_interaction.invalidate();
        }
    }

    /// Plan and persist the typed value. Returns `true` when the editor should close.
    fn save_threshold_override(&mut self, editor: &mut ThresholdEditor) -> bool {
        let Some(path) = self.settings_project_config_path.clone() else {
            editor.error = Some("No project configuration is bound".into());
            return false;
        };
        let key = editor.key();
        let plan = match plan_override(
            &editor.plan_models(),
            &key,
            &editor.editor.text(),
            editor.window,
        ) {
            Ok(plan) => plan,
            Err(message) => {
                editor.error = Some(message);
                return false;
            }
        };
        let mut map = editor.project.clone();
        // The colon spelling of the same model would keep applying after a reset.
        map.remove(&format!("{}:{}", editor.provider, editor.model));
        match &plan.write {
            Some(value) => {
                map.insert(plan.key.clone(), value.clone());
            }
            None => {
                map.remove(&plan.key);
            }
        }
        match write_project_setting_value(&path, THRESHOLDS_ID, &Value::Object(map).to_string()) {
            Ok(stored) => {
                self.store_setting_value(THRESHOLDS_ID, stored);
                let (message, variant) = match (&plan.write, plan.inherits.as_deref()) {
                    (Some(_), _) => match plan.warnings.first() {
                        Some(warning) => (
                            format!("Threshold saved for {key}. {}", warning_text(*warning)),
                            ToastVariant::Warning,
                        ),
                        None => (format!("Threshold saved for {key}"), ToastVariant::Info),
                    },
                    (None, Some(inherited)) => (
                        format!("Threshold reset for {key}; {inherited} still applies"),
                        ToastVariant::Info,
                    ),
                    (None, None) => (format!("Threshold reset for {key}"), ToastVariant::Info),
                };
                self.show_toast(message, variant);
                true
            }
            Err(error) => {
                editor.error = Some(error.to_string());
                false
            }
        }
    }

    /// Lines and input for the open editor; `None` when it is closed.
    pub(crate) fn threshold_editor_view(&self) -> Option<ThresholdEditorView> {
        let editor = self.threshold_editor.as_ref()?;
        let key = editor.key();
        let text = editor.editor.text();
        let input = text.trim();
        let overrides = editor.plan_models();
        let mut lines = Vec::new();

        lines.push((
            ThresholdLineTone::Plain,
            match editor.window {
                Some(window) => format!(
                    "Model window: {} tokens",
                    format_override(&ThresholdOverride::BaseWindow(window))
                ),
                None => "Model window unknown; limits cannot be checked".into(),
            },
        ));

        let plan = plan_override(&overrides, &key, input, editor.window);
        match &plan {
            Err(message) => lines.push((ThresholdLineTone::Error, message.clone())),
            Ok(plan) => {
                for warning in &plan.warnings {
                    lines.push((ThresholdLineTone::Warning, warning_text(*warning).into()));
                }
                if input.is_empty() {
                    lines.push((
                        ThresholdLineTone::Plain,
                        match plan.inherits.as_deref() {
                            Some(inherited) => {
                                format!("Enter resets this model; {inherited} still applies")
                            }
                            None => "Enter removes this model's override".into(),
                        },
                    ));
                }
            }
        }

        if !self.model_thresholds_enabled() {
            lines.push((
                ThresholdLineTone::Warning,
                "Per-model thresholds are turned off, so overrides do not apply".into(),
            ));
        } else if plan.is_ok() {
            let effective = if input.is_empty() {
                // Without its own entry the model falls back to a wildcard, if any.
                let inherited: BTreeMap<_, _> = overrides
                    .iter()
                    .filter(|(name, _)| **name != key)
                    .map(|(name, value)| (name.clone(), *value))
                    .collect();
                match_model_key(&inherited, &editor.provider, &editor.model).copied()
            } else {
                parse_override(input, true).ok()
            };
            if let Some(shown) = preview(
                editor.window,
                effective.as_ref(),
                &self.threshold_settings(),
            ) {
                let origin = match shown.origin {
                    PreviewOrigin::ConfiguredFixed => "configured fixed",
                    PreviewOrigin::ScaledFromBase => "scaled from base",
                };
                lines.push((
                    ThresholdLineTone::Plain,
                    format!(
                        "Compacts at {} tokens ({origin})",
                        format_override(&ThresholdOverride::BaseWindow(shown.tokens))
                    ),
                ));
            }
        }

        if let Some(error) = editor.error.as_ref() {
            lines.push((ThresholdLineTone::Error, error.clone()));
        }
        lines.push((
            ThresholdLineTone::Plain,
            "80% of the window · f150000 fixed tokens · 200K base window · empty resets".into(),
        ));

        let cursor = text
            .graphemes(true)
            .take(editor.editor.cursor().insertion_index())
            .map(str::len)
            .sum();
        Some(ThresholdEditorView {
            title: format!("Compaction threshold for {key}"),
            input: text,
            cursor,
            selected: editor.editor.selection().is_some(),
            lines,
        })
    }
}
