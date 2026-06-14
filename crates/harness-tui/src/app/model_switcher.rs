use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::child_session::provider_label_includes_backend;
use super::model_dialog_state::ModelDialogKind;
use super::model_metadata::{LaunchMetadata, ModelOption};
use super::session_history::{fuzzy_subsequence_score, session_history_profile_label};
use super::AppState;
use crate::text::has_trimmed_content;

fn non_empty_str(value: &str) -> Option<&str> {
    has_trimmed_content(value).then_some(value)
}

fn activity_provider_model(activity: &super::ActivityEntry) -> Option<(&str, &str)> {
    Some((
        non_empty_str(&activity.provider_id)?,
        non_empty_str(&activity.model_id)?,
    ))
}

fn model_selector_fuzzy_score(option: &ModelOption, needle: &str) -> Option<usize> {
    let title_score = fuzzy_subsequence_score(&option.selector_title().to_lowercase(), needle)
        .map(|score| score.saturating_mul(2));
    let category_score =
        fuzzy_subsequence_score(&option.selector_category().to_lowercase(), needle)
            .map(|score| score.saturating_mul(2).saturating_add(1));
    match (title_score, category_score) {
        (Some(title), Some(category)) => Some(title.min(category)),
        (Some(title), None) => Some(title),
        (None, Some(category)) => Some(category),
        (None, None) => None,
    }
}

fn runtime_identity_for_metadata(metadata: &LaunchMetadata) -> String {
    let model_label = metadata
        .display_label()
        .or_else(|| metadata.model())
        .unwrap_or("-");
    format!("{} · {model_label}", metadata.profile())
}

impl AppState {
    pub fn active_profile(&self) -> &str {
        let profile = self.launch_metadata.profile();
        if Self::launch_value_is_unknown(profile) {
            "default"
        } else {
            profile
        }
    }

    pub fn active_provider(&self) -> &str {
        let provider = self.launch_metadata.provider();
        if !Self::launch_value_is_unknown(provider) && provider != "local" {
            provider
        } else {
            self.activities
                .back()
                .and_then(|activity| non_empty_str(&activity.provider_id))
                .filter(|value| !Self::launch_value_is_unknown(value))
                .unwrap_or("local")
        }
    }

    fn current_model_id(&self) -> &str {
        self.launch_metadata
            .model()
            .or_else(|| {
                self.activities
                    .back()
                    .and_then(|activity| non_empty_str(&activity.model_id))
            })
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or("-")
    }

    pub(crate) fn current_model_variant(&self) -> Option<&str> {
        self.launch_metadata.variant()
    }

    pub fn current_model_label(&self) -> &str {
        self.launch_metadata
            .display_label()
            .unwrap_or_else(|| self.current_model_id())
    }

    pub fn current_model_base_label(&self) -> &str {
        self.launch_metadata
            .model_display_label()
            .or_else(|| self.launch_metadata.model())
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or_else(|| self.current_model_id())
    }

    pub fn current_model_reasoning_label(&self) -> Option<&str> {
        self.launch_metadata
            .reasoning_effort()
            .or_else(|| self.launch_metadata.variant_display_label())
            .or_else(|| self.launch_metadata.variant())
            .or_else(|| {
                self.launch_metadata
                    .mode_label()
                    .filter(|label| !label.eq_ignore_ascii_case("live"))
            })
            .filter(|value| !Self::launch_value_is_unknown(value))
    }

    pub(crate) fn current_context_window_tokens(&self) -> Option<u32> {
        self.launch_metadata.context_window_tokens().or_else(|| {
            self.activities
                .back()
                .and_then(|activity| {
                    non_empty_str(&activity.model_id).map(|model_id| {
                        LaunchMetadata::new(
                            self.active_profile(),
                            self.active_provider(),
                            Some(model_id.to_string()),
                        )
                        .context_window_tokens()
                    })
                })
                .flatten()
        })
    }

    pub fn current_source_label(&self) -> Option<String> {
        let provider = self
            .launch_metadata
            .provider_display_label()
            .or_else(|| {
                let provider = self.launch_metadata.provider();
                (!Self::launch_value_is_unknown(provider) && provider != "local")
                    .then_some(provider)
            })
            .or_else(|| {
                let provider = self.active_provider();
                (!Self::launch_value_is_unknown(provider) && provider != "local")
                    .then_some(provider)
            })?;
        let backend = self
            .launch_metadata
            .provider_backend_label()
            .filter(|value| !Self::launch_value_is_unknown(value));
        Some(match backend {
            Some(backend) if !provider_label_includes_backend(provider, backend) => {
                format!("{provider} ({backend})")
            }
            None => provider.to_string(),
            Some(_) => provider.to_string(),
        })
    }

    pub fn current_agent_label(&self) -> Option<String> {
        let profile = self
            .launch_metadata
            .configured_profile()
            .or_else(|| {
                let profile = self.active_profile();
                (!Self::launch_value_is_unknown(profile) && profile != "default").then_some(profile)
            })
            .filter(|value| !Self::launch_value_is_unknown(value))?;
        Some(super::humanize_profile_label(profile))
    }

    pub(in crate::app) fn runtime_context_metadata(&self) -> &LaunchMetadata {
        self.runtime_context_metadata
            .as_ref()
            .unwrap_or(&self.launch_metadata)
    }

    pub(in crate::app) fn runtime_context_profile(&self) -> &str {
        let profile = self.runtime_context_metadata().profile();
        if Self::launch_value_is_unknown(profile) {
            self.active_profile()
        } else {
            profile
        }
    }

    pub(in crate::app) fn runtime_context_provider(&self) -> &str {
        let provider = self.runtime_context_metadata().provider();
        if Self::launch_value_is_unknown(provider) || provider == "local" {
            self.active_provider()
        } else {
            provider
        }
    }

    pub(in crate::app) fn runtime_context_model_label(&self) -> String {
        self.runtime_context_metadata()
            .display_label()
            .or_else(|| self.runtime_context_metadata().model())
            .filter(|value| !Self::launch_value_is_unknown(value))
            .unwrap_or_else(|| self.current_model_label())
            .to_string()
    }

    pub(in crate::app) fn runtime_context_identity(&self) -> String {
        format!(
            "{} · {}",
            self.runtime_context_profile(),
            self.runtime_context_model_label()
        )
    }

    pub(in crate::app) fn runtime_context_label(&self) -> crate::view_model::RuntimeContextLabel {
        if self.startup_shell_visible() {
            crate::view_model::RuntimeContextLabel::Launch
        } else if self.replay_mode {
            crate::view_model::RuntimeContextLabel::RecordedRuntimeReadOnly
        } else if self.continued_live_run() {
            crate::view_model::RuntimeContextLabel::ContinuedRuntime
        } else {
            crate::view_model::RuntimeContextLabel::CurrentRuntime
        }
    }

    pub(in crate::app) fn runtime_provider_context(&self) -> Option<String> {
        let provider = self.runtime_context_provider().trim();
        (!provider.is_empty()).then(|| provider.to_string())
    }

    pub(in crate::app) fn next_turn_identity(&self) -> Option<String> {
        if self.startup_shell_visible() || self.replay_mode {
            return None;
        }

        let current = self.runtime_context_metadata();
        let next = &self.launch_metadata;
        let changed = current.profile() != next.profile()
            || current.provider() != next.provider()
            || current.model() != next.model()
            || current.variant() != next.variant();
        changed.then(|| runtime_identity_for_metadata(next))
    }

    pub(in crate::app) fn model_switcher_supported(&self) -> bool {
        !self.replay_mode
    }

    pub(in crate::app) fn handle_model_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_palette();
                true
            }
            KeyCode::Enter => {
                self.execute_selected_model();
                true
            }
            KeyCode::PageUp => {
                self.move_model_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.move_model_selection(10);
                true
            }
            KeyCode::Home => {
                self.model_selected = 0;
                true
            }
            KeyCode::End => {
                self.model_selected = self.model_filtered.len().saturating_sub(1);
                true
            }
            KeyCode::Up => {
                self.move_model_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_model_selection(1);
                true
            }
            KeyCode::Backspace => {
                self.overlay_backspace(Self::update_model_filter);
                true
            }
            KeyCode::Delete => {
                self.overlay_delete(Self::update_model_filter);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.move_model_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.move_model_selection(1);
                true
            }
            KeyCode::Char('f') if ctrl_only => {
                self.toggle_selected_model_favorite();
                true
            }
            KeyCode::Char('a') if ctrl_only => {
                self.open_provider_dialog();
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return false;
                }
                self.overlay_insert_char(c, Self::update_model_filter);
                true
            }
            _ => false,
        }
    }

    fn move_model_selection(&mut self, delta: isize) {
        let len = self.model_filtered.len();
        if len == 0 {
            self.model_selected = 0;
            return;
        }

        if delta == -1 {
            self.model_selected = if self.model_selected == 0 {
                len - 1
            } else {
                self.model_selected - 1
            };
            return;
        }

        if delta == 1 {
            self.model_selected = (self.model_selected + 1) % len;
            return;
        }

        let current = self.model_selected.min(len.saturating_sub(1)) as isize;
        let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
        self.model_selected = usize::try_from(next).unwrap_or(0);
    }
    pub fn launch_metadata(&self) -> &LaunchMetadata {
        &self.launch_metadata
    }

    pub fn apply_auth_provider_catalog_refresh(&mut self, launch_metadata: LaunchMetadata) {
        self.set_launch_metadata(launch_metadata);
        if !self.launch_metadata.available_models().is_empty() {
            self.status_banner = Some("Provider connected; choose a model".to_string());
            self.open_model_switcher();
        }
    }

    pub fn set_launch_metadata(&mut self, launch_metadata: LaunchMetadata) {
        let refresh_runtime_context = self.startup_mode
            || self.replay_mode
            || self.runtime_context_metadata.is_none()
            || (self.events.is_empty() && self.activities.is_empty());
        self.launch_metadata = launch_metadata.clone();
        if let Some(option) = self.launch_metadata.to_model_option() {
            self.model_dialog_state.record_recent(&option);
        }
        self.seed_toggles_from_launch_metadata();
        if refresh_runtime_context {
            self.runtime_context_metadata = Some(launch_metadata);
        }
    }
    pub(in crate::app) fn build_launch_metadata_for_option(
        &self,
        selected_model: &ModelOption,
    ) -> LaunchMetadata {
        let mut launch_metadata = LaunchMetadata::from_model_option(selected_model)
            .with_available_models(self.launch_metadata.available_models().to_vec())
            .with_switchable_profiles(self.launch_metadata.switchable_profiles().to_vec());
        if let Some(mode_label) = self.launch_metadata.mode_label().map(str::to_owned) {
            launch_metadata = launch_metadata.with_mode_label(mode_label);
        }
        launch_metadata
    }

    pub(crate) fn model_option_is_favorite(&self, option: &ModelOption) -> bool {
        self.model_dialog_state.is_favorite(option)
    }

    fn toggle_selected_model_favorite(&mut self) {
        if self.active_model_dialog_kind() != ModelDialogKind::Model {
            return;
        }
        let Some(selected_index) = self.model_filtered.get(self.model_selected).copied() else {
            return;
        };
        let Some(selected_model) = self.model_options.get(selected_index).cloned() else {
            return;
        };
        self.model_dialog_state.toggle_favorite(&selected_model);
        self.update_model_filter();
    }

    pub(in crate::app) fn cycle_recent_model(&mut self, reverse: bool) {
        if self.replay_mode {
            return;
        }
        self.rebuild_model_options();
        let recents = self.model_dialog_state.recent_options(&self.model_options);
        if recents.len() < 2 {
            return;
        }
        let current_index = recents
            .iter()
            .position(|option| self.is_current_model_option(option))
            .unwrap_or(0);
        let next_index = if reverse {
            current_index
                .checked_sub(1)
                .unwrap_or_else(|| recents.len().saturating_sub(1))
        } else {
            (current_index + 1) % recents.len()
        };
        self.apply_selected_model_option(recents[next_index].clone(), true);
    }

    pub(crate) fn is_current_model_option(&self, option: &ModelOption) -> bool {
        option.profile == self.active_profile()
            && option.provider == self.active_provider()
            && option.model == self.current_model_id()
            && option.variant() == self.current_model_variant()
    }

    pub(crate) fn model_switcher_visual_row_count(&self) -> usize {
        if self.model_filtered.is_empty() {
            return 1;
        }
        if !self.palette_input.trim().is_empty() {
            return self.model_filtered.len();
        }

        let mut rows = 0usize;
        let mut groups = 0usize;
        let mut previous_category: Option<String> = None;
        for option_index in &self.model_filtered {
            let Some(option) = self.model_options.get(*option_index) else {
                continue;
            };
            let category = self.model_dialog_group_label(option);
            if previous_category.as_deref() != Some(category.as_str()) {
                rows = rows.saturating_add(if groups == 0 { 1 } else { 2 });
                groups = groups.saturating_add(1);
                previous_category = Some(category);
            }
            rows = rows.saturating_add(1);
        }
        rows.max(1)
    }

    pub(in crate::app) fn rebuild_model_options(&mut self) {
        self.model_options = self.collect_model_dialog_options();
    }

    pub(in crate::app) fn collect_model_options(&self) -> BTreeSet<ModelOption> {
        let mut options = BTreeSet::new();

        options.extend(
            self.launch_metadata
                .available_models()
                .iter()
                .map(|option| self.model_selector_option_for_active_profile(option)),
        );

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            options.insert(self.model_selector_option_for_active_profile(&current_option));
        }

        if options.is_empty() {
            for activity in &self.activities {
                if let Some((provider, model)) = activity_provider_model(activity) {
                    let option = ModelOption {
                        profile: self.launch_metadata.profile().to_string(),
                        provider: provider.to_string(),
                        provider_display_label: None,
                        provider_backend_label: None,
                        model: model.to_string(),
                        model_display_label: None,
                        variant: None,
                        variant_display_label: None,
                        display_label: None,
                        token_window_label: None,
                        context_window_tokens: None,
                        max_input_tokens: None,
                        max_output_tokens: None,
                        description: None,
                        profile_description: None,
                        reasoning_effort: None,
                        text_verbosity: None,
                        recommended_for: None,
                    };
                    options.insert(self.model_selector_option_for_active_profile(&option));
                }
            }

            for entry in &self.session_history_entries {
                let Some(provider_model) = entry.catalog.provider_model.as_deref() else {
                    continue;
                };
                let Some((provider, model)) = provider_model.split_once('/') else {
                    continue;
                };
                let option = ModelOption {
                    profile: session_history_profile_label(entry).to_string(),
                    provider: provider.to_string(),
                    provider_display_label: None,
                    provider_backend_label: None,
                    model: model.to_string(),
                    model_display_label: None,
                    variant: None,
                    variant_display_label: None,
                    display_label: None,
                    token_window_label: None,
                    context_window_tokens: None,
                    max_input_tokens: None,
                    max_output_tokens: None,
                    description: None,
                    profile_description: None,
                    reasoning_effort: None,
                    text_verbosity: None,
                    recommended_for: None,
                };
                options.insert(self.model_selector_option_for_active_profile(&option));
            }
        }

        options
    }

    pub(in crate::app) fn model_option_for_active_profile(
        &self,
        option: &ModelOption,
    ) -> ModelOption {
        let mut option = option.clone();
        option.profile = self.active_profile().to_string();
        option.profile_description = self
            .launch_metadata
            .profile_description()
            .map(str::to_string);
        option
    }

    fn model_selector_option_for_active_profile(&self, option: &ModelOption) -> ModelOption {
        let mut option = self.model_option_for_active_profile(option);
        option.variant = None;
        option.variant_display_label = None;
        option.display_label = option.model_display_label.clone();
        option.reasoning_effort = None;
        option.text_verbosity = None;
        option.recommended_for = None;
        option
    }

    pub(super) fn update_model_filter(&mut self) {
        let input = self.palette_input.trim().to_lowercase();
        if input.is_empty() {
            let mut filtered = (0..self.model_options.len()).collect::<Vec<_>>();
            filtered.sort_by(|left, right| {
                let left_option = &self.model_options[*left];
                let right_option = &self.model_options[*right];
                self.model_dialog_group_label(left_option)
                    .cmp(&self.model_dialog_group_label(right_option))
                    .then_with(|| {
                        left_option
                            .selector_title()
                            .cmp(right_option.selector_title())
                    })
                    .then_with(|| left_option.model.cmp(&right_option.model))
                    .then_with(|| left_option.variant.cmp(&right_option.variant))
                    .then_with(|| left_option.profile.cmp(&right_option.profile))
            });
            self.model_selected = filtered
                .iter()
                .position(|index| self.is_current_model_option(&self.model_options[*index]))
                .unwrap_or(0);
            self.model_filtered = filtered;
            return;
        }

        let mut filtered = self
            .model_options
            .iter()
            .enumerate()
            .filter_map(|(index, option)| {
                model_selector_fuzzy_score(option, &input).map(|score| (index, score))
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|(left, left_score), (right, right_score)| {
            let left_option = &self.model_options[*left];
            let right_option = &self.model_options[*right];
            left_score
                .cmp(right_score)
                .then_with(|| {
                    left_option
                        .selector_category()
                        .cmp(right_option.selector_category())
                })
                .then_with(|| {
                    left_option
                        .selector_title()
                        .cmp(right_option.selector_title())
                })
                .then_with(|| left_option.model.cmp(&right_option.model))
        });
        self.model_filtered = filtered.into_iter().map(|(index, _)| index).collect();
        self.model_selected = 0;
    }

    pub(super) fn open_model_switcher(&mut self) {
        if !self.model_switcher_supported() {
            self.overlay_state.model_switcher_visible = false;
            return;
        }
        self.open_model_dialog(ModelDialogKind::Model);
    }

    pub(super) fn execute_selected_model(&mut self) {
        self.execute_selected_model_dialog_option();
    }
}
