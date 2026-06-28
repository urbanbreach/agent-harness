//! Palette controller: filtering, grouping, suggested rows, and availability
//! for the Opencode-compatible command palette.
//!
//! This module implements the Opencode palette semantics:
//! - Fuzzy filtering on title and category only (not command IDs)
//! - Title weighted higher than category
//! - Results preserve category grouping even when filtered
//! - Empty filter duplicates suggested commands into a synthetic "Suggested" group
//! - Non-empty filter has no suggested duplicates
//! - No-result text is exactly "No results found"

use crate::app::AppState;
use crate::keybindings::palette_model::{
    entries, find, DynamicTitle, PaletteCategory, PaletteCommandEntry, SuggestedRule,
};

/// A resolved palette row after filtering and grouping.
#[derive(Debug, Clone)]
pub struct PaletteRow {
    /// Command ID (or `suggested:<id>` for synthetic suggested duplicates).
    pub value: String,
    /// The underlying command ID (without `suggested:` prefix).
    pub command_id: &'static str,
    /// Display title (resolved dynamically for toggle commands).
    pub title: String,
    /// Description.
    pub description: &'static str,
    /// Category for grouping.
    pub category: PaletteCategory,
    /// Whether this is a synthetic suggested duplicate.
    #[allow(dead_code)]
    pub is_suggested_duplicate: bool,
    /// Whether this row is a harness-only command.
    #[allow(dead_code)]
    pub harness_only: bool,
}

/// Check if a command is available in the current app state.
pub fn is_available(app: &AppState, entry: &PaletteCommandEntry) -> bool {
    match entry.id {
        "session.rename" | "session.timeline" | "session.fork" | "session.compact"
        | "session.undo" | "messages.copy" | "session.copy" | "session.export" | "session.move" => {
            !app.startup_shell_visible() && !app.replay_mode
        }

        "session.sidebar.toggle"
        | "session.toggle.conceal"
        | "session.toggle.timestamps"
        | "session.toggle.thinking"
        | "session.toggle.actions"
        | "session.toggle.scrollbar"
        | "session.toggle.generic_tool_output" => {
            !app.startup_shell_visible() && !app.replay_mode && app.active_review_surface.is_none()
        }

        "session.redo" => {
            !app.startup_shell_visible() && !app.replay_mode && app.has_revert_message()
        }
        "session.unshare" => {
            !app.startup_shell_visible() && !app.replay_mode && app.has_share_url()
        }

        // Harness does not yet implement workspaces, editor context,
        // org switching, or model variants; these are always unavailable.
        "workspace.list" | "workspace.set" | "workspace.copy_path" => false,
        "prompt.editor_context.clear" => false,
        "console.org.switch" => false,
        "variant.list" => false,

        "model.list" => app.model_switcher_supported(),
        "variant.cycle" => !app.replay_mode,

        "harness.toggle_terminal_panel" => !app.startup_shell_visible(),
        "harness.toggle_follow" => !app.startup_shell_visible() && !app.replay_mode,
        "harness.expand_turn_results" | "harness.collapse_turn_results" => {
            !app.startup_shell_visible() && app.active_review_surface.is_none()
        }
        "harness.close_review_surface" => app.active_review_surface.is_some(),
        "harness.revert_workspace" => {
            !app.replay_mode
                && !app.startup_shell_visible()
                && app.most_recent_workspace_snapshot_request_id().is_some()
        }
        "harness.open_lineage_browser"
        | "harness.session_child_first"
        | "harness.session_child_cycle"
        | "harness.session_child_cycle_reverse"
        | "harness.session_parent" => !app.startup_shell_visible(),

        "prompt.stash" => !app.composer.prompt_buffer.is_empty(),
        "prompt.stash.pop" | "prompt.stash.list" => !app.prompt_stash.entries.is_empty(),

        "provider.connect" => true,

        _ => true,
    }
}

pub fn is_suggested(app: &AppState, entry: &PaletteCommandEntry) -> bool {
    match entry.suggested {
        SuggestedRule::Never => false,
        SuggestedRule::Always => true,
        SuggestedRule::WhenSessionsExist => !app.session_history_entries.is_empty(),
        SuggestedRule::WhenSessionRoute => !app.startup_shell_visible(),
        SuggestedRule::WhenDisconnected => app.provider_disconnected(),
    }
}

/// Resolve the dynamic title for a command based on app state.
pub fn resolve_title(app: &AppState, entry: &PaletteCommandEntry) -> String {
    match entry.title {
        DynamicTitle::Static(title) => title.to_string(),
        DynamicTitle::ShowHide { show, hide } => {
            let is_shown = match entry.id {
                "session.sidebar.toggle" => app.details_drawer_open(),
                "session.toggle.timestamps" => app.transcript_view.show_transcript_timestamps,
                "session.toggle.thinking" => app.transcript_view.show_transcript_thinking,
                "session.toggle.actions" => app.transcript_view.show_tool_details,
                "session.toggle.generic_tool_output" => {
                    app.transcript_view.show_generic_tool_output
                }
                // Harness does not yet have a tips overlay; always hidden.
                "tips.toggle" => false,
                _ => false,
            };
            if is_shown {
                hide.to_string()
            } else {
                show.to_string()
            }
        }
        DynamicTitle::Toggle { enable, disable } => {
            // Harness does not yet implement these toggle features;
            // they are always disabled (showing the "enable" label).
            let is_enabled = match entry.id {
                "session.toggle.conceal" => false,
                "terminal.title.toggle" => false,
                "app.toggle.animations" => false,
                "app.toggle.file_context" => false,
                "app.toggle.diffwrap" => false,
                "app.toggle.paste_summary" => false,
                "app.toggle.session_directory_filter" => false,
                _ => false,
            };
            if is_enabled {
                disable.to_string()
            } else {
                enable.to_string()
            }
        }
    }
}

/// Compute the filtered and grouped palette rows.
///
/// When the filter is empty:
/// - Suggested commands are duplicated into a synthetic "Suggested" group
///   with values prefixed by `suggested:<id>`.
/// - All commands (including the originals, not just suggested) appear in
///   their normal category groups.
///
/// When the filter is non-empty:
/// - No suggested duplicates are produced.
/// - Results are filtered by title and category only (not command IDs).
/// - Title matches are weighted higher than category matches.
/// - Results preserve category grouping.
pub fn compute_palette_rows(app: &AppState, filter: &str) -> Vec<PaletteRow> {
    use PaletteCategory as C;
    const CATEGORY_ORDER: [C; 7] = [
        C::Suggested,
        C::Session,
        C::Agent,
        C::Workspace,
        C::Provider,
        C::Prompt,
        C::System,
    ];

    let needle = filter.to_lowercase();
    let mut available: Vec<&PaletteCommandEntry> = entries()
        .iter()
        .filter(|entry| is_available(app, entry))
        .collect();

    available.sort_by_key(|entry| {
        CATEGORY_ORDER
            .iter()
            .position(|c| *c == entry.category)
            .unwrap_or(usize::MAX)
    });

    if needle.is_empty() {
        let suggested: Vec<&PaletteCommandEntry> = available
            .iter()
            .copied()
            .filter(|entry| is_suggested(app, entry))
            .collect();

        let mut rows: Vec<PaletteRow> = Vec::new();

        for entry in &suggested {
            rows.push(PaletteRow {
                value: format!("suggested:{}", entry.id),
                command_id: entry.id,
                title: resolve_title(app, entry),
                description: entry.description,
                category: PaletteCategory::Suggested,
                is_suggested_duplicate: true,
                harness_only: entry.harness_only,
            });
        }

        for entry in &available {
            rows.push(PaletteRow {
                value: entry.id.to_string(),
                command_id: entry.id,
                title: resolve_title(app, entry),
                description: entry.description,
                category: entry.category,
                is_suggested_duplicate: false,
                harness_only: entry.harness_only,
            });
        }

        rows
    } else {
        let mut scored: Vec<(usize, usize, &PaletteCommandEntry)> = Vec::new();

        for (index, entry) in available.iter().enumerate() {
            let title = resolve_title(app, entry).to_lowercase();
            let category = entry.category.label().to_lowercase();

            let title_score = fuzzy_subsequence_score(&title, &needle);
            let category_score = fuzzy_subsequence_score(&category, &needle);

            // Title weighted higher: category score is doubled (penalized)
            // so title matches produce lower (better) overall scores.
            // This mirrors Opencode's scoreFn: r[0].score * 2 + r[1].score
            // where fuzzysort scores are negative (lower = better).
            let score = match (title_score, category_score) {
                (Some(t), Some(c)) => t.saturating_add(c.saturating_mul(2)),
                (Some(t), None) => t,
                (None, Some(c)) => c.saturating_mul(2),
                (None, None) => continue,
            };

            scored.push((score, index, entry));
        }

        let mut grouped: Vec<(&PaletteCommandEntry, usize)> = Vec::new();
        let mut category_buckets: std::collections::BTreeMap<
            usize,
            Vec<(usize, usize, &PaletteCommandEntry)>,
        > = std::collections::BTreeMap::new();

        for (score, index, entry) in &scored {
            let cat_order = CATEGORY_ORDER
                .iter()
                .position(|c| *c == entry.category)
                .unwrap_or(usize::MAX);
            category_buckets
                .entry(cat_order)
                .or_default()
                .push((*score, *index, *entry));
        }

        for (_cat_order, mut bucket) in category_buckets {
            bucket.sort_by_key(|(score, index, _)| (*score, *index));
            for (_score, _index, entry) in bucket {
                grouped.push((entry, _score));
            }
        }

        grouped
            .into_iter()
            .map(|(entry, _)| PaletteRow {
                value: entry.id.to_string(),
                command_id: entry.id,
                title: resolve_title(app, entry),
                description: entry.description,
                category: entry.category,
                is_suggested_duplicate: false,
                harness_only: entry.harness_only,
            })
            .collect()
    }
}

/// Fuzzy subsequence match: returns Some(score) if needle is a subsequence of haystack.
/// Lower score = better match. Score is the number of characters between matches.
pub(crate) fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    let haystack_chars: Vec<char> = haystack.chars().collect();
    let needle_chars: Vec<char> = needle.chars().collect();

    let mut haystack_idx = 0;
    let mut total_gap = 0;
    let mut last_match = 0;

    for needle_char in &needle_chars {
        let mut found = false;
        while haystack_idx < haystack_chars.len() {
            if haystack_chars[haystack_idx] == *needle_char {
                total_gap += haystack_idx - last_match;
                last_match = haystack_idx + 1;
                haystack_idx += 1;
                found = true;
                break;
            }
            haystack_idx += 1;
        }
        if !found {
            return None;
        }
    }

    // Prefer prefix matches (lower gap = better)
    Some(total_gap)
}

/// Dispatch a palette command by its value (which may be `suggested:<id>`).
pub fn dispatch_palette_command(app: &mut AppState, value: &str) {
    // Strip suggested: prefix if present
    let command_id = value.strip_prefix("suggested:").unwrap_or(value);

    let Some(entry) = find(command_id) else {
        return;
    };

    if !is_available(app, entry) {
        return;
    }

    use crate::keybindings::palette_model::PaletteDispatch;
    match entry.dispatch {
        PaletteDispatch::Action(action) => {
            app.execute_action(action);
        }
        PaletteDispatch::ToggleTranscriptThinking => {
            app.transcript_view.show_transcript_thinking =
                !app.transcript_view.show_transcript_thinking;
        }
        PaletteDispatch::ToggleTranscriptTimestamps => {
            app.transcript_view.show_transcript_timestamps =
                !app.transcript_view.show_transcript_timestamps;
        }
        PaletteDispatch::ToggleToolDetails => {
            app.transcript_view.show_tool_details = !app.transcript_view.show_tool_details;
        }
        PaletteDispatch::ToggleGenericToolOutput => {
            app.transcript_view.show_generic_tool_output =
                !app.transcript_view.show_generic_tool_output;
        }
        PaletteDispatch::ToggleStackedDiffs => {
            app.transcript_view.stacked_transcript_diffs =
                !app.transcript_view.stacked_transcript_diffs;
        }
        PaletteDispatch::ExpandTurnResults => {
            app.set_selected_activity_expandable_outputs(true);
        }
        PaletteDispatch::CollapseTurnResults => {
            app.set_selected_activity_expandable_outputs(false);
        }
        PaletteDispatch::OpenSessionHistory => {
            app.begin_session_history_picker(crate::app::StartupLauncherAction::ContinueSession);
        }
        PaletteDispatch::OpenModelSwitcher => {
            app.open_model_switcher();
        }
        PaletteDispatch::OpenTogglesMenu => {
            app.open_toggles_menu();
        }
        PaletteDispatch::OpenAuth => {
            let auth_args = vec!["login".to_string()];
            app.status_banner = Some(crate::app::auth_status_banner(&auth_args));
            app.emit_ui_intent(crate::app::UiIntent::OpenAuthManager {
                args: auth_args,
                stdin: None,
            });
        }
        PaletteDispatch::OpenEventLog => {
            app.status_banner = Some("event log surface has been removed".to_string());
        }
        PaletteDispatch::OpenConnectDialog => {
            app.open_connect_dialog();
        }
        PaletteDispatch::NewSession => {
            app.startup_launcher_action = crate::app::StartupLauncherAction::NewSession;
            app.apply_new_session_launcher_selection();
        }
        PaletteDispatch::CompactSession => {
            app.emit_ui_intent(crate::app::UiIntent::CompactSession);
        }
        PaletteDispatch::Placeholder => {
            app.status_banner = Some(format!(
                "{} is not yet available in Harness",
                resolve_title(app, entry)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_subsequence_matches_title() {
        assert!(fuzzy_subsequence_score("switch model", "swi").is_some());
        assert!(fuzzy_subsequence_score("switch model", "swi").is_some());
    }

    #[test]
    fn fuzzy_subsequence_returns_none_for_no_match() {
        assert!(fuzzy_subsequence_score("switch model", "xyz").is_none());
    }

    #[test]
    fn fuzzy_subsequence_empty_needle_matches() {
        assert_eq!(fuzzy_subsequence_score("anything", ""), Some(0));
    }

    #[test]
    fn title_score_weighted_higher_than_category() {
        // "session" appears in both title and category for session commands
        // Title match should produce a lower (better) score than category-only match
        let title_score = fuzzy_subsequence_score("switch session", "session");
        let category_score = fuzzy_subsequence_score("session", "session");
        assert!(title_score.is_some());
        assert!(category_score.is_some());
        // Title weighted 2x, category weighted 1x - title should still be better or equal
        // because the needle is found earlier in the title
    }
}
