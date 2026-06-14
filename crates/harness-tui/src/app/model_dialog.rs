use std::collections::BTreeSet;

use super::model_dialog_state::ModelDialogKind;
use super::{humanize_profile_label, set_pending_live_launch_metadata, AppState, ModelOption};
use crate::app::{ToastVariant, UiIntent};
use crate::text::non_empty_trimmed;

impl AppState {
    pub(in crate::app) fn reset_model_dialog_kind(&mut self) {
        self.model_dialog_state
            .set_active_dialog(ModelDialogKind::Model);
    }

    pub(crate) fn active_model_dialog_kind(&self) -> ModelDialogKind {
        self.model_dialog_state.active_dialog()
    }

    pub(in crate::app) fn open_provider_dialog(&mut self) {
        self.open_model_dialog(ModelDialogKind::Provider);
    }

    pub(in crate::app) fn open_variant_dialog(&mut self) {
        self.open_model_dialog(ModelDialogKind::Variant);
    }

    pub(in crate::app) fn open_agent_dialog(&mut self) {
        self.open_model_dialog(ModelDialogKind::Agent);
    }

    pub(in crate::app) fn open_model_dialog(&mut self, kind: ModelDialogKind) {
        if self.replay_mode {
            self.overlay_state.model_switcher_visible = false;
            return;
        }
        if !self.overlay_state.model_switcher_visible {
            self.palette_focus_return.get_or_insert(self.focus);
        }
        self.overlay_state.palette_visible = false;
        self.overlay_state.session_history_visible = false;
        self.overlay_state.model_switcher_visible = true;
        self.model_dialog_state.set_active_dialog(kind);
        self.palette_input.clear();
        self.palette_cursor = 0;
        self.rebuild_model_options();
        self.update_model_filter();
        self.sync_slash_overlay();
    }

    pub(crate) fn model_dialog_title(&self) -> &'static str {
        match self.active_model_dialog_kind() {
            ModelDialogKind::Model => "Select model",
            ModelDialogKind::Provider => "Select provider",
            ModelDialogKind::Variant => "Select variant",
            ModelDialogKind::Agent => "Select agent",
        }
    }

    pub(crate) fn model_dialog_placeholder(&self) -> &'static str {
        match self.active_model_dialog_kind() {
            ModelDialogKind::Model => "Search",
            ModelDialogKind::Provider => "Search providers",
            ModelDialogKind::Variant => "Search variants",
            ModelDialogKind::Agent => "Search agents",
        }
    }

    pub(crate) fn model_dialog_empty_message(&self) -> &'static str {
        match self.active_model_dialog_kind() {
            ModelDialogKind::Model
                if self.launch_metadata().available_models().is_empty()
                    && self.launch_metadata().model().is_none() =>
            {
                "Connect a provider to list models"
            }
            ModelDialogKind::Model => "No results found",
            ModelDialogKind::Provider => "No providers found",
            ModelDialogKind::Variant => "No variants found",
            ModelDialogKind::Agent => "No agents found",
        }
    }

    pub(crate) fn model_dialog_group_label(&self, option: &ModelOption) -> String {
        match self.active_model_dialog_kind() {
            ModelDialogKind::Model => {
                if self.model_option_is_favorite(option) {
                    "Favorites".to_string()
                } else {
                    option.selector_category().to_string()
                }
            }
            ModelDialogKind::Provider => "Providers".to_string(),
            ModelDialogKind::Variant => "Variants".to_string(),
            ModelDialogKind::Agent => "Agents".to_string(),
        }
    }

    pub(crate) fn model_dialog_option_title(&self, option: &ModelOption) -> String {
        match self.active_model_dialog_kind() {
            ModelDialogKind::Model => option.selector_title().to_string(),
            ModelDialogKind::Provider => option.selector_category().to_string(),
            ModelDialogKind::Variant => option
                .variant_display_label()
                .or_else(|| option.variant())
                .unwrap_or("Default")
                .to_string(),
            ModelDialogKind::Agent => humanize_profile_label(&option.profile),
        }
    }

    pub(crate) fn model_dialog_option_footer(&self, option: &ModelOption) -> String {
        match self.active_model_dialog_kind() {
            ModelDialogKind::Model => option.selector_category().to_string(),
            ModelDialogKind::Provider => option
                .model_display_label()
                .unwrap_or(&option.model)
                .to_string(),
            ModelDialogKind::Variant => option.reasoning_effort().unwrap_or("").to_string(),
            ModelDialogKind::Agent => option.selector_title().to_string(),
        }
    }

    pub(in crate::app) fn collect_model_dialog_options(&self) -> Vec<ModelOption> {
        match self.active_model_dialog_kind() {
            ModelDialogKind::Model => self.collect_model_options().into_iter().collect(),
            ModelDialogKind::Provider => self.collect_provider_dialog_options(),
            ModelDialogKind::Variant => self.collect_variant_dialog_options(),
            ModelDialogKind::Agent => self.collect_agent_dialog_options(),
        }
    }

    pub(in crate::app) fn execute_selected_model_dialog_option(&mut self) {
        let Some(selected_index) = self.model_filtered.get(self.model_selected).copied() else {
            self.close_palette();
            return;
        };
        if self.replay_mode {
            self.close_palette();
            return;
        }
        let Some(mut selected_model) = self.model_options.get(selected_index).cloned() else {
            self.close_palette();
            return;
        };
        if self.active_model_dialog_kind() == ModelDialogKind::Variant
            && selected_model.variant().is_none()
        {
            selected_model = model_variant_cycle_none_option(&selected_model);
        }
        self.apply_selected_model_option(selected_model.clone(), true);
        if matches!(
            self.active_model_dialog_kind(),
            ModelDialogKind::Model | ModelDialogKind::Provider
        ) {
            self.model_dialog_state.record_recent(&selected_model);
        }
        self.close_palette();
    }

    pub(in crate::app) fn cycle_variant(&mut self) {
        if self.replay_mode {
            return;
        }
        let variants = self.collect_variant_dialog_options();
        if variants.len() < 2 {
            return;
        }
        let selected_model = match variants
            .iter()
            .position(|option| self.is_current_model_option(option))
        {
            Some(current_index) => variants[(current_index + 1) % variants.len()].clone(),
            None => variants[0].clone(),
        };
        self.apply_selected_model_option(selected_model, true);
    }

    pub(in crate::app) fn cycle_agent(&mut self, reverse: bool) {
        if self.replay_mode {
            return;
        }
        let options = self.collect_agent_dialog_options();
        if options.len() < 2 {
            return;
        }
        let current_index = options
            .iter()
            .position(|option| option.profile == self.active_profile())
            .unwrap_or(0);
        let next_index = if reverse {
            current_index
                .checked_sub(1)
                .unwrap_or_else(|| options.len().saturating_sub(1))
        } else {
            (current_index + 1) % options.len()
        };
        self.apply_selected_model_option(options[next_index].clone(), true);
    }

    fn collect_provider_dialog_options(&self) -> Vec<ModelOption> {
        let mut seen = BTreeSet::new();
        let mut options = Vec::new();
        for option in self.launch_metadata().available_models() {
            if option.profile != self.active_profile() || !seen.insert(option.provider.clone()) {
                continue;
            }
            options.push(self.model_option_for_active_profile(option));
        }
        options.sort_by(|left, right| {
            left.selector_category()
                .cmp(right.selector_category())
                .then_with(|| left.provider.cmp(&right.provider))
        });
        options
    }

    fn collect_variant_dialog_options(&self) -> Vec<ModelOption> {
        let profile_id = self.launch_metadata().profile().to_string();
        let provider_id = self.launch_metadata().provider().to_string();
        let Some(model_id) = self.launch_metadata().model().map(str::to_owned) else {
            return Vec::new();
        };
        let mut variants = self
            .launch_metadata()
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
            let seed = self
                .launch_metadata()
                .to_model_option()
                .or_else(|| variants.first().cloned());
            if let Some(seed) = seed {
                variants.insert(0, model_variant_cycle_none_option(&seed));
            }
        }
        if let Some(current_option) = self.launch_metadata().to_model_option() {
            let current_variant = current_option.variant();
            if current_option.profile == profile_id
                && current_option.provider == provider_id
                && current_option.model == model_id
                && !variants.iter().any(|option| {
                    option.profile == profile_id
                        && option.provider == provider_id
                        && option.model == model_id
                        && option.variant() == current_variant
                })
            {
                variants.push(current_option);
            }
        }
        variants.sort_by(model_variant_cycle_cmp);
        variants.dedup();
        variants
    }

    fn collect_agent_dialog_options(&self) -> Vec<ModelOption> {
        let mut profiles = self
            .launch_metadata()
            .switchable_profiles()
            .iter()
            .filter_map(|profile| non_empty_trimmed(profile))
            .filter(|profile| self.primary_agent_enabled(profile))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            profiles.extend(
                self.launch_metadata()
                    .available_models()
                    .iter()
                    .filter(|option| self.primary_agent_enabled(&option.profile))
                    .map(|option| option.profile.clone()),
            );
        }
        profiles.sort();
        profiles.dedup();
        profiles
            .into_iter()
            .filter_map(|profile| self.model_option_for_agent_profile(&profile))
            .collect()
    }

    fn model_option_for_agent_profile(&self, profile: &str) -> Option<ModelOption> {
        let available = self.launch_metadata().available_models();
        let provider = self.launch_metadata().provider();
        let model = self.launch_metadata().model();
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
                self.launch_metadata()
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

    pub(in crate::app) fn apply_selected_model_option(
        &mut self,
        selected_model: ModelOption,
        emit_intent: bool,
    ) {
        let launch_metadata = self.build_launch_metadata_for_option(&selected_model);
        self.launch_metadata = launch_metadata.clone();
        if emit_intent {
            set_pending_live_launch_metadata(launch_metadata.clone());
            self.emit_ui_intent(UiIntent::SwitchModel {
                profile: selected_model.profile.clone(),
                launch_metadata,
            });
            self.show_toast(
                format!("Next turn model: {}", selected_model.selector_title()),
                ToastVariant::Info,
            );
        }
    }
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
