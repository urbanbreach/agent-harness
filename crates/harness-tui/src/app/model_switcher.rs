use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::model_metadata::{LaunchMetadata, ModelOption};
use super::session_history::{fuzzy_subsequence_score, session_history_profile_label};
use super::{set_pending_live_launch_metadata, AppState, UiIntent};
use crate::text::{has_trimmed_content, non_empty_trimmed};

fn non_empty_str(value: &str) -> Option<&str> {
    has_trimmed_content(value).then_some(value)
}

fn activity_provider_model(activity: &super::ActivityEntry) -> Option<(&str, &str)> {
    Some((
        non_empty_str(&activity.provider_id)?,
        non_empty_str(&activity.model_id)?,
    ))
}

fn model_variant_cycle_cmp(left: &ModelOption, right: &ModelOption) -> std::cmp::Ordering {
    left.provider
        .cmp(&right.provider)
        .then_with(|| left.model.cmp(&right.model))
        .then_with(|| variant_cycle_rank(left).cmp(&variant_cycle_rank(right)))
        .then_with(|| left.variant.cmp(&right.variant))
        .then_with(|| left.profile.cmp(&right.profile))
}

fn model_variant_cycle_none_option(seed: &ModelOption) -> ModelOption {
    let mut option = seed.clone();
    let base_label = option.model_display_label.clone().or_else(|| {
        option
            .display_label
            .as_deref()
            .and_then(|label| label.split_once(" · ").map(|(base, _)| base.trim()))
            .filter(|base| !base.is_empty())
            .map(str::to_string)
    });
    option.variant = None;
    option.variant_display_label = None;
    if option.model_display_label.is_none() {
        option.model_display_label = base_label.clone();
    }
    option.display_label = base_label;
    option.token_window_label = None;
    option.max_input_tokens = None;
    option.max_output_tokens = None;
    option.description = None;
    option.reasoning_effort = None;
    option.text_verbosity = None;
    option.recommended_for = None;
    option
}

fn model_variant_cycle_option_matches_current(
    option: &ModelOption,
    profile_id: &str,
    provider_id: &str,
    model_id: &str,
    variant: Option<&str>,
) -> bool {
    option.profile == profile_id
        && option.provider == provider_id
        && option.model == model_id
        && option.variant() == variant
}

fn variant_cycle_rank(option: &ModelOption) -> u8 {
    option
        .reasoning_effort()
        .or_else(|| option.variant())
        .map(reasoning_variant_rank)
        .unwrap_or(0)
}

fn reasoning_variant_rank(label: &str) -> u8 {
    match label.trim().to_ascii_lowercase().as_str() {
        "none" => 1,
        "minimal" => 2,
        "low" => 3,
        "medium" => 4,
        "high" => 5,
        "xhigh" | "x-high" | "extra-high" => 6,
        "max" => 7,
        _ => u8::MAX,
    }
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

fn model_option_favorite_key(option: &ModelOption) -> String {
    option.model.clone()
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
        Some(provider.to_string())
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
                self.toggle_model_favorite();
                true
            }
            KeyCode::F(2) => {
                self.cycle_recent_model(false);
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
        self.seed_toggles_from_launch_metadata();
        self.seed_toggles_from_formatter_config();
        if refresh_runtime_context {
            self.runtime_context_metadata = Some(launch_metadata);
        }
    }
    fn build_launch_metadata_for_option(&self, selected_model: &ModelOption) -> LaunchMetadata {
        let mut launch_metadata = LaunchMetadata::from_model_option(selected_model)
            .with_available_models(self.launch_metadata.available_models().to_vec())
            .with_switchable_profiles(self.launch_metadata.switchable_profiles().to_vec());
        if let Some(mode_label) = self.launch_metadata.mode_label().map(str::to_owned) {
            launch_metadata = launch_metadata.with_mode_label(mode_label);
        }
        launch_metadata
    }

    fn apply_selected_model_option(&mut self, selected_model: ModelOption, emit_intent: bool) {
        let launch_metadata = self.build_launch_metadata_for_option(&selected_model);
        self.launch_metadata = launch_metadata.clone();

        if emit_intent {
            let favorite_key = model_option_favorite_key(&selected_model);
            super::model_favorites::push_model_recent(&mut self.model_recents, &favorite_key);
            self.persist_model_recents();
            set_pending_live_launch_metadata(launch_metadata.clone());
            self.emit_ui_intent(UiIntent::SwitchModel {
                profile: selected_model.profile,
                launch_metadata,
            });
        }
    }

    pub(super) fn cycle_variant(&mut self) {
        if self.replay_mode {
            return;
        }

        let profile_id = self.launch_metadata.profile().to_string();
        let Some(model_id) = self.launch_metadata.model().map(str::to_owned) else {
            return;
        };
        let provider_id = self.launch_metadata.provider().to_string();
        let mut variants = self
            .launch_metadata
            .available_models()
            .iter()
            .filter(|option| {
                option.profile == profile_id
                    && option.provider == provider_id
                    && option.model == model_id
            })
            .cloned()
            .collect::<Vec<_>>();

        let explicit_variants_exist = variants.iter().any(|option| option.variant().is_some());
        let base_variant_exists = variants.iter().any(|option| option.variant().is_none());
        if explicit_variants_exist && !base_variant_exists {
            let none_seed = self
                .launch_metadata
                .to_model_option()
                .or_else(|| variants.first().cloned());
            if let Some(none_seed) = none_seed {
                variants.push(model_variant_cycle_none_option(&none_seed));
            }
        }

        if let Some(current_option) = self.launch_metadata.to_model_option() {
            let current_variant = current_option.variant();
            if current_option.profile == profile_id
                && current_option.provider == provider_id
                && current_option.model == model_id
                && !variants.iter().any(|option| {
                    model_variant_cycle_option_matches_current(
                        option,
                        &profile_id,
                        &provider_id,
                        &model_id,
                        current_variant,
                    )
                })
            {
                variants.push(current_option);
            }
        }

        variants.sort_by(model_variant_cycle_cmp);
        variants.dedup();
        if variants.is_empty() {
            return;
        }

        let selected_model = match variants.iter().position(|option| {
            model_variant_cycle_option_matches_current(
                option,
                &profile_id,
                &provider_id,
                &model_id,
                self.current_model_variant(),
            )
        }) {
            Some(_) if variants.len() < 2 => return,
            Some(current_index) => {
                let next_index = (current_index + 1) % variants.len();
                variants[next_index].clone()
            }
            None => variants[0].clone(),
        };
        self.apply_selected_model_option(selected_model, !self.replay_mode);
    }

    pub(super) fn cycle_agent(&mut self, reverse: bool) {
        if self.replay_mode {
            return;
        }

        let profiles = self.switchable_agent_profiles();
        if profiles.len() < 2 {
            return;
        }

        let current_profile = self.active_profile();
        let current_index = profiles
            .iter()
            .position(|profile| profile == current_profile)
            .unwrap_or(0);
        let next_index = if reverse {
            current_index
                .checked_sub(1)
                .unwrap_or_else(|| profiles.len().saturating_sub(1))
        } else {
            (current_index + 1) % profiles.len()
        };

        let Some(selected_model) = self.model_option_for_agent_profile(&profiles[next_index])
        else {
            return;
        };
        self.apply_selected_model_option(selected_model, true);
    }

    fn switchable_agent_profiles(&self) -> Vec<String> {
        let mut profiles = self
            .launch_metadata
            .switchable_profiles()
            .iter()
            .filter_map(|profile| non_empty_trimmed(profile))
            .filter(|profile| self.primary_agent_enabled(profile))
            .map(str::to_string)
            .collect::<Vec<_>>();

        if profiles.is_empty() {
            for candidate in ["build", "plan"] {
                if self
                    .launch_metadata
                    .available_models()
                    .iter()
                    .any(|option| option.profile == candidate)
                    && self.primary_agent_enabled(candidate)
                {
                    profiles.push(candidate.to_string());
                }
            }
        }

        if profiles.is_empty() {
            profiles.push(self.active_profile().to_string());
        }

        let mut deduped = Vec::new();
        for profile in profiles {
            if !deduped.contains(&profile) {
                deduped.push(profile);
            }
        }
        deduped
    }

    fn model_option_for_agent_profile(&self, profile: &str) -> Option<ModelOption> {
        let available = self.launch_metadata.available_models();
        let provider = self.launch_metadata.provider();
        let model = self.launch_metadata.model();
        let variant = self.current_model_variant();

        available
            .iter()
            .find(|option| {
                option.profile == profile
                    && option.provider == provider
                    && Some(option.model.as_str()) == model
                    && option.variant() == variant
            })
            .cloned()
            .or_else(|| {
                available
                    .iter()
                    .find(|option| {
                        option.profile == profile
                            && option.provider == provider
                            && Some(option.model.as_str()) == model
                    })
                    .cloned()
            })
            .or_else(|| {
                self.launch_metadata
                    .to_model_option()
                    .map(|option| option.with_profile(profile.to_string()))
            })
            .or_else(|| {
                available
                    .iter()
                    .find(|option| option.profile == profile)
                    .cloned()
            })
    }
    pub(crate) fn is_current_model_option(&self, option: &ModelOption) -> bool {
        option.profile == self.active_profile()
            && option.provider == self.active_provider()
            && option.model == self.current_model_id()
            && option
                .variant()
                .is_none_or(|variant| Some(variant) == self.current_model_variant())
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
        let mut previous_category: Option<&str> = None;
        for option_index in &self.model_filtered {
            let Some(option) = self.model_options.get(*option_index) else {
                continue;
            };
            let category = option.selector_category();
            if previous_category != Some(category) {
                rows = rows.saturating_add(if groups == 0 { 1 } else { 2 });
                groups = groups.saturating_add(1);
                previous_category = Some(category);
            }
            rows = rows.saturating_add(1);
        }
        rows.max(1)
    }

    pub(in crate::app) fn rebuild_model_options(&mut self) {
        self.model_options = self.collect_model_options().into_iter().collect();
    }

    fn collect_model_options(&self) -> BTreeSet<ModelOption> {
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
                        thinking: None,
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
                    thinking: None,
                    recommended_for: None,
                };
                options.insert(self.model_selector_option_for_active_profile(&option));
            }
        }

        options
    }

    fn model_option_for_active_profile(&self, option: &ModelOption) -> ModelOption {
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
                let left_fav = self
                    .model_favorites
                    .contains(&model_option_favorite_key(left_option));
                let right_fav = self
                    .model_favorites
                    .contains(&model_option_favorite_key(right_option));
                right_fav
                    .cmp(&left_fav)
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
            let left_fav = self
                .model_favorites
                .contains(&model_option_favorite_key(left_option));
            let right_fav = self
                .model_favorites
                .contains(&model_option_favorite_key(right_option));
            right_fav
                .cmp(&left_fav)
                .then_with(|| left_score.cmp(right_score))
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
            self.model_switcher_visible = false;
            return;
        }
        if !self.model_switcher_visible {
            self.palette_focus_return.get_or_insert(self.focus);
        }
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = true;
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.rebuild_model_options();
        self.update_model_filter();
        self.sync_slash_overlay();
    }

    pub(super) fn execute_selected_model(&mut self) {
        let Some(selected_index) = self.model_filtered.get(self.model_selected).copied() else {
            self.close_palette();
            return;
        };

        if self.replay_mode {
            self.close_palette();
            return;
        }

        let Some(selected_model) = self.model_options.get(selected_index).cloned() else {
            self.close_palette();
            return;
        };

        self.apply_selected_model_option(selected_model, true);
        self.close_palette();
    }

    fn toggle_model_favorite(&mut self) {
        let Some(selected_index) = self.model_filtered.get(self.model_selected).copied() else {
            return;
        };
        let Some(option) = self.model_options.get(selected_index) else {
            return;
        };
        let key = model_option_favorite_key(option);
        if !self.model_favorites.insert(key.clone()) {
            self.model_favorites.remove(&key);
        }
        self.persist_model_favorites();
        self.update_model_filter();
    }

    fn cycle_recent_model(&mut self, reverse: bool) {
        if self.model_recents.is_empty() {
            return;
        }
        let current_key = self.current_model_id().to_string();
        let len = self.model_recents.len();
        let current_index = self
            .model_recents
            .iter()
            .position(|recent| recent == &current_key);
        let next_index = match current_index {
            Some(idx) => {
                if reverse {
                    idx.saturating_sub(1)
                } else {
                    (idx + 1) % len
                }
            }
            None => 0,
        };
        let Some(target_key) = self.model_recents.get(next_index) else {
            return;
        };
        let target_option = self
            .model_options
            .iter()
            .find(|option| option.model == *target_key)
            .cloned();
        if let Some(option) = target_option {
            self.apply_selected_model_option(option, !self.replay_mode);
        }
    }

    fn persist_model_favorites(&mut self) {
        let Some(path) = self.model_favorites_path.as_deref() else {
            return;
        };
        if let Err(err) = super::model_favorites::save_model_favorites(path, &self.model_favorites)
        {
            self.status_banner = Some(err);
        }
    }

    fn persist_model_recents(&mut self) {
        let Some(path) = self.model_recents_path.as_deref() else {
            return;
        };
        if let Err(err) = super::model_favorites::save_model_recents(path, &self.model_recents) {
            self.status_banner = Some(err);
        }
    }
}
