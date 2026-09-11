use super::*;
use crate::keybindings;

#[derive(Clone)]
pub(crate) struct SlashCompletionRow {
    pub(crate) index: usize,
    pub(crate) label: String,
    pub(crate) description: String,
}

impl AppState {
    pub(crate) fn slash_completion_column_width(&self, width: u16) -> usize {
        use unicode_width::UnicodeWidthStr;
        let maximum = (0..self.slash_filtered.len())
            .map(|index| self.slash_completion_label(index).width() + 2)
            .max()
            .unwrap_or(0);
        maximum.min(usize::from(width.saturating_sub(2)) / 3).max(1)
    }

    pub(crate) fn slash_completion_rows(&self, width: u16) -> Vec<SlashCompletionRow> {
        let label_width = self.slash_completion_column_width(width);
        let description_width = usize::from(width.saturating_sub(2))
            .saturating_sub(label_width)
            .max(1);
        self.slash_filtered
            .iter()
            .enumerate()
            .flat_map(|(index, _)| {
                let label = self.slash_completion_label(index);
                let mut descriptions = crate::ui::wrap_completion_text(
                    &self.slash_completion_description(index),
                    description_width,
                );
                if descriptions.is_empty() {
                    descriptions.push(String::new());
                }
                descriptions
                    .into_iter()
                    .enumerate()
                    .map(move |(row, description)| SlashCompletionRow {
                        index,
                        label: if row == 0 {
                            label.clone()
                        } else {
                            String::new()
                        },
                        description,
                    })
            })
            .collect()
    }

    pub(crate) fn slash_completion_viewport(&self, area: Rect) -> Vec<(SlashCompletionRow, Rect)> {
        let rows = self.slash_completion_rows(area.width);
        let first = rows
            .iter()
            .position(|row| row.index == self.slash_selected)
            .unwrap_or(0);
        let end = rows
            .iter()
            .rposition(|row| row.index == self.slash_selected)
            .unwrap_or(first)
            + 1;
        let height = usize::from(area.height);
        let start = end.saturating_sub(height).min(first);
        rows.into_iter()
            .skip(start)
            .take(height)
            .enumerate()
            .map(|(offset, row)| {
                (
                    row,
                    Rect::new(
                        area.x,
                        area.y
                            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX)),
                        area.width,
                        1,
                    ),
                )
            })
            .collect()
    }

    pub(crate) fn slash_completion_label(&self, index: usize) -> String {
        if let Some(option) = self.slash_arguments.get(index) {
            if self.slash_model_pending.is_some() {
                option
                    .reasoning_effort()
                    .or_else(|| option.variant())
                    .unwrap_or("default")
                    .to_string()
            } else {
                option.selector_title().to_string()
            }
        } else {
            self.slash_filtered
                .get(index)
                .map_or_else(String::new, |command| format!("/{command}"))
        }
    }

    pub(crate) fn slash_completion_description(&self, index: usize) -> String {
        if let Some(option) = self.slash_arguments.get(index) {
            if self.slash_model_pending.is_some() {
                "Reasoning effort · Enter to select".to_string()
            } else {
                option
                    .description
                    .clone()
                    .unwrap_or_else(|| option.selector_category().to_string())
            }
        } else {
            self.slash_filtered
                .get(index)
                .map_or_else(String::new, |command| {
                    match self.slash_argument_required(command) {
                        Some(true) => "argument required · Enter to run".to_string(),
                        Some(false) => "argument optional · Enter to run".to_string(),
                        None => keybindings::slash_command_description(command).to_string(),
                    }
                })
        }
    }

    pub(in crate::app) fn sync_slash_model_completions(&mut self) {
        let args = self
            .active_slash_parts_full()
            .and_then(|(_, args)| args)
            .unwrap_or("")
            .to_string();
        let pending = self
            .slash_model_pending
            .as_ref()
            .filter(|option| args.starts_with(&format!("{}/{} ", option.provider, option.model)))
            .cloned();
        self.slash_model_pending = pending.clone();
        self.slash_arguments = if let Some(option) = pending {
            let prefix = format!("{}/{} ", option.provider, option.model);
            let query = args
                .strip_prefix(&prefix)
                .unwrap_or("")
                .trim()
                .to_lowercase();
            let mut variants = self
                .launch_metadata
                .available_models()
                .iter()
                .filter(|candidate| {
                    candidate.provider == option.provider && candidate.model == option.model
                })
                .filter(|candidate| {
                    candidate
                        .reasoning_effort()
                        .or_else(|| candidate.variant())
                        .is_some()
                })
                .filter(|candidate| {
                    candidate
                        .reasoning_effort()
                        .or_else(|| candidate.variant())
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
                })
                .cloned()
                .collect::<Vec<_>>();
            variants.sort_by(|a, b| {
                a.reasoning_effort
                    .cmp(&b.reasoning_effort)
                    .then(a.variant.cmp(&b.variant))
            });
            variants.dedup_by(|a, b| {
                a.reasoning_effort == b.reasoning_effort && a.variant == b.variant
            });
            variants
        } else {
            self.rebuild_model_options();
            let query = args.trim().to_lowercase();
            self.model_options
                .iter()
                .filter(|option| {
                    format!(
                        "{} {}/{}",
                        option.selector_title(),
                        option.provider,
                        option.model
                    )
                    .to_lowercase()
                    .contains(&query)
                })
                .cloned()
                .collect()
        };
        self.slash_filtered = (0..self.slash_arguments.len())
            .map(|index| format!("model:{index}"))
            .collect();
        self.slash_selected = 0;
        self.slash_hovered = None;
        self.slash_pointer_down = None;
    }

    pub(in crate::app) fn accept_slash_model(&mut self, execute: bool) -> bool {
        let Some(mut option) = self.slash_arguments.get(self.slash_selected).cloned() else {
            return false;
        };
        option.profile = self.active_profile().to_string();
        let has_efforts = self
            .launch_metadata
            .available_models()
            .iter()
            .any(|candidate| {
                candidate.provider == option.provider
                    && candidate.model == option.model
                    && candidate
                        .reasoning_effort()
                        .or_else(|| candidate.variant())
                        .is_some()
            });
        if self.slash_model_pending.is_none() && has_efforts {
            let text = format!("/model {}/{} ", option.provider, option.model);
            self.slash_model_pending = Some(option);
            self.composer.push_undo();
            self.replace_prompt_input(text);
            self.sync_slash_overlay();
        } else if execute {
            self.apply_selected_model_option(option, true);
            let draft = self.slash_draft_snapshot.take().unwrap_or_default();
            self.replace_prompt_input(draft);
            self.clear_slash_menu();
            self.slash_model_pending = None;
        } else {
            let effort = self
                .slash_model_pending
                .as_ref()
                .and_then(|_| option.reasoning_effort().or_else(|| option.variant()));
            let text = format!(
                "/model {}/{}{}",
                option.provider,
                option.model,
                effort.map_or_else(String::new, |effort| format!(" {effort}"))
            );
            self.composer.push_undo();
            self.replace_prompt_input(text);
            self.sync_slash_overlay();
        }
        true
    }
}
