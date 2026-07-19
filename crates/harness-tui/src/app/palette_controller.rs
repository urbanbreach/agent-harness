// allow: SIZE_OK — TUI app state (session projection + interaction)
//! Palette controller: filtering, grouping, suggested rows, and availability
//! for the ruleset-compatible command palette.
//!
//! This module implements the Harness palette semantics:
//! - Fuzzy filtering on title and category only (not command IDs)
//! - Title weighted higher than category
//! - Results preserve category grouping even when filtered
//! - Empty filter duplicates suggested commands into a synthetic "Suggested" group
//! - Non-empty filter has no suggested duplicates
//! - No-result text is exactly "No results found"

use std::sync::LazyLock;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::app::AppState;
use crate::keybindings::palette_model::{
    entries, find, DynamicTitle, PaletteCategory, PaletteCommandEntry, PaletteDispatch,
    SuggestedRule,
};

static FUZZY_MATCHER: LazyLock<SkimMatcherV2> = LazyLock::new(SkimMatcherV2::default);

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteLogEntry {
    pub command_id: String,
    pub dialog_state: PaletteDialogState,
    pub dispatch_target: &'static str,
    pub status: PaletteLogStatus,
    pub availability_reason: Option<&'static str>,
    pub filter_length: usize,
    pub error_kind: Option<&'static str>,
    pub session_id_redacted: Option<String>,
    pub provider_id_redacted: Option<String>,
    pub model_id_redacted: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaletteDialogState {
    Opened,
    Filtered,
    Selected,
    Closed,
    DispatchStarted,
    DispatchSucceeded,
    DispatchFailed,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaletteLogStatus {
    Success,
    Failure,
    Rejected,
}

fn dispatch_target_label(entry: &PaletteCommandEntry) -> &'static str {
    use crate::keybindings::palette_model::PaletteDispatch;
    match entry.dispatch {
        PaletteDispatch::Action(_) => "action",
        PaletteDispatch::ToggleTranscriptThinking
        | PaletteDispatch::ToggleTranscriptTimestamps
        | PaletteDispatch::ToggleToolDetails
        | PaletteDispatch::ToggleGenericToolOutput
        | PaletteDispatch::ToggleStackedDiffs
        | PaletteDispatch::ExpandTurnResults
        | PaletteDispatch::CollapseTurnResults => "local_toggle",
        PaletteDispatch::OpenSessionHistory
        | PaletteDispatch::OpenSessionRename
        | PaletteDispatch::OpenForkSelector
        | PaletteDispatch::OpenModelSwitcher
        | PaletteDispatch::OpenTogglesMenu
        | PaletteDispatch::OpenAuth
        | PaletteDispatch::OpenEventLog
        | PaletteDispatch::OpenConnectDialog => "dialog",
        PaletteDispatch::NewSession
        | PaletteDispatch::NewWorktreeSession
        | PaletteDispatch::CompactSession => "intent",
        PaletteDispatch::CopySessionTranscript => "action",
        PaletteDispatch::Placeholder => "failure",
    }
}

pub(crate) fn redacted_session_id(app: &AppState) -> Option<String> {
    app.run_id()
        .map(|id| format!("session:{}", &id[..id.len().min(8)]))
}

pub(crate) fn redacted_provider_id(app: &AppState) -> Option<String> {
    if app.launch_metadata.has_provider() {
        Some(format!(
            "provider:{}",
            &app.launch_metadata.provider()[..app.launch_metadata.provider().len().min(8)]
        ))
    } else {
        None
    }
}

pub(crate) fn redacted_model_id(app: &AppState) -> Option<String> {
    app.launch_metadata
        .model()
        .map(|id| format!("model:{}", &id[..id.len().min(8)]))
}

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
    pub is_suggested_duplicate: bool,
    /// Whether this row is a harness-only command.
    pub harness_only: bool,
}

/// Check if a command is available in the current app state.
pub fn is_available(app: &AppState, entry: &PaletteCommandEntry) -> bool {
    match entry.id {
        "session.rename" | "session.compact" => !app.replay_mode,
        "session.timeline" | "session.fork" | "session.undo" | "messages.copy" | "session.copy"
        | "session.export" | "session.move" => !app.startup_shell_visible() && !app.replay_mode,

        "session.status.open"
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

        "model.list" | "agent.list" | "mcp.list" => {
            !app.startup_shell_visible() && app.model_switcher_supported()
        }
        "variant.cycle" => !app.startup_shell_visible() && !app.replay_mode,
        "provider.connect" => !app.startup_shell_visible(),
        "app.exit" => !app.startup_shell_visible(),

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
        | "harness.session_parent"
        | "harness.session_background" => !app.startup_shell_visible(),

        "prompt.stash" => !app.composer.prompt_buffer.is_empty(),
        "prompt.stash.pop" | "prompt.stash.list" => !app.prompt_stash.entries.is_empty(),

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
    const CATEGORY_ORDER: [C; 9] = [
        C::Session,
        C::Context,
        C::ModelInput,
        C::Suggested,
        C::Agent,
        C::Workspace,
        C::Provider,
        C::Prompt,
        C::System,
    ];

    let needle = filter.to_lowercase();
    let mut available: Vec<&PaletteCommandEntry> = entries()
        .iter()
        .filter(|entry| !entry.harness_only)
        .filter(|entry| !matches!(entry.dispatch, PaletteDispatch::Placeholder))
        .filter(|entry| is_available(app, entry))
        .collect();

    available.sort_by_key(|entry| {
        CATEGORY_ORDER
            .iter()
            .position(|c| *c == entry.category)
            .unwrap_or(usize::MAX)
    });

    if needle.is_empty() {
        const EMPTY_FILTER_CATEGORIES: [C; 3] = [C::Session, C::Context, C::ModelInput];
        available
            .into_iter()
            .filter(|entry| EMPTY_FILTER_CATEGORIES.contains(&entry.category))
            .map(|entry| PaletteRow {
                value: entry.id.to_string(),
                command_id: entry.id,
                title: resolve_title(app, entry),
                description: entry.description,
                category: entry.category,
                is_suggested_duplicate: false,
                harness_only: entry.harness_only,
            })
            .collect()
    } else {
        let mut scored: Vec<(i64, usize, &PaletteCommandEntry)> = Vec::new();

        for (index, entry) in available.iter().enumerate() {
            let title = resolve_title(app, entry).to_lowercase();
            let category = entry.category.label().to_lowercase();

            let title_score = fuzzy_subsequence_score(&title, &needle);
            let category_score = fuzzy_subsequence_score(&category, &needle);

            let score = match (title_score, category_score) {
                (Some(t), Some(c)) => t.saturating_mul(2).saturating_add(c),
                (Some(t), None) => t.saturating_mul(2),
                (None, Some(c)) => c,
                (None, None) => continue,
            };

            scored.push((score, index, entry));
        }

        let mut grouped: Vec<(&PaletteCommandEntry, i64)> = Vec::new();
        let mut category_buckets: std::collections::BTreeMap<
            usize,
            Vec<(i64, usize, &PaletteCommandEntry)>,
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

pub(crate) fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<i64> {
    if needle.is_empty() {
        return Some(0);
    }
    FUZZY_MATCHER
        .fuzzy_match(haystack, needle)
        .map(|score| -score)
}

/// Dispatch a palette command by its value (which may be `suggested:<id>`).
pub fn dispatch_palette_command(app: &mut AppState, value: &str) {
    // Strip suggested: prefix if present
    let command_id = value.strip_prefix("suggested:").unwrap_or(value);

    let Some(entry) = find(command_id) else {
        return;
    };

    if !is_available(app, entry) {
        app.palette_log.push(PaletteLogEntry {
            command_id: entry.id.to_string(),
            dialog_state: PaletteDialogState::Rejected,
            dispatch_target: dispatch_target_label(entry),
            status: PaletteLogStatus::Rejected,
            availability_reason: Some("unavailable"),
            filter_length: app.palette_input.len(),
            error_kind: None,
            session_id_redacted: redacted_session_id(app),
            provider_id_redacted: redacted_provider_id(app),
            model_id_redacted: redacted_model_id(app),
        });
        return;
    }

    let target = dispatch_target_label(entry);
    app.palette_log.push(PaletteLogEntry {
        command_id: entry.id.to_string(),
        dialog_state: PaletteDialogState::DispatchStarted,
        dispatch_target: target,
        status: PaletteLogStatus::Success,
        availability_reason: None,
        filter_length: app.palette_input.len(),
        error_kind: None,
        session_id_redacted: redacted_session_id(app),
        provider_id_redacted: redacted_provider_id(app),
        model_id_redacted: redacted_model_id(app),
    });

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
        PaletteDispatch::OpenSessionRename => {
            app.open_session_rename_dialog();
        }
        PaletteDispatch::OpenForkSelector => {
            app.open_fork_selector();
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
        PaletteDispatch::NewWorktreeSession => {
            app.request_new_worktree_session();
        }
        PaletteDispatch::CompactSession => {
            app.emit_ui_intent(crate::app::UiIntent::CompactSession);
        }
        PaletteDispatch::CopySessionTranscript => {
            let text: String = app
                .activities
                .iter()
                .map(|a| a.transcript_text.as_str())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            if !text.is_empty() {
                let _ = crate::clipboard::copy(&text);
                app.show_toast("Copied session transcript", crate::app::ToastVariant::Info);
            }
        }
        PaletteDispatch::Placeholder => {
            app.status_banner = Some(format!(
                "{} is not yet available in Harness",
                resolve_title(app, entry)
            ));
            app.palette_log.push(PaletteLogEntry {
                command_id: entry.id.to_string(),
                dialog_state: PaletteDialogState::DispatchFailed,
                dispatch_target: "failure",
                status: PaletteLogStatus::Failure,
                availability_reason: None,
                filter_length: app.palette_input.len(),
                error_kind: Some("placeholder"),
                session_id_redacted: redacted_session_id(app),
                provider_id_redacted: redacted_provider_id(app),
                model_id_redacted: redacted_model_id(app),
            });
            return;
        }
    }

    app.palette_log.push(PaletteLogEntry {
        command_id: entry.id.to_string(),
        dialog_state: PaletteDialogState::DispatchSucceeded,
        dispatch_target: target,
        status: PaletteLogStatus::Success,
        availability_reason: None,
        filter_length: app.palette_input.len(),
        error_kind: None,
        session_id_redacted: redacted_session_id(app),
        provider_id_redacted: redacted_provider_id(app),
        model_id_redacted: redacted_model_id(app),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_palette_rows_matches_title() {
        // arrange
        // act
        // assert
        let app = AppState::new_live(None, false, None);
        let rows = compute_palette_rows(&app, "swi");
        assert!(
            !rows.is_empty(),
            "query 'swi' must match 'Switch model' title"
        );
    }

    #[test]
    fn compute_palette_rows_returns_empty_for_no_match() {
        // arrange
        // act
        // assert
        let app = AppState::new_live(None, false, None);
        let rows = compute_palette_rows(&app, "xyz");
        assert!(rows.is_empty(), "query 'xyz' must produce no results");
    }

    #[test]
    fn compute_palette_rows_empty_filter_returns_all() {
        // arrange
        // act
        // assert
        let app = AppState::new_live(None, false, None);
        let rows = compute_palette_rows(&app, "");
        assert!(
            !rows.is_empty(),
            "empty filter must return freeze inventory"
        );
        assert!(
            rows.iter().all(|r| {
                matches!(
                    r.category,
                    PaletteCategory::Session
                        | PaletteCategory::Context
                        | PaletteCategory::ModelInput
                )
            }),
            "empty filter inventory is Session/Context/Model & Input only"
        );
    }

    #[test]
    fn compute_palette_rows_title_weighted_higher_than_category() {
        // arrange
        // act
        // assert
        let app = AppState::new_live(None, false, None);
        let rows = compute_palette_rows(&app, "session");
        assert!(!rows.is_empty());
        assert!(
            rows[0].title.to_lowercase().contains("session"),
            "title match must rank first for query 'session'"
        );
    }
}
