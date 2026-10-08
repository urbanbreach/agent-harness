//! Plan list overlay backed by `project_plan_list`, with optional content preview.

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use harness_core::plan::{project_plan_list, PlanProjectionEntry, PLAN_DIR};

use super::{AppState, ToastVariant};

/// One row in the plan viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanViewRow {
    pub path: String,
    pub slug: String,
    pub exists: bool,
    pub is_active: bool,
    pub selected: bool,
    pub byte_len: Option<u64>,
}

impl PlanViewRow {
    /// Operator-facing one-line plan row (overlay/list diagnostics).
    pub fn one_line(&self) -> String {
        let exists = if self.exists { "exists" } else { "missing" };
        let active = if self.is_active { "active" } else { "inactive" };
        let bytes = self
            .byte_len
            .map(|n| format!("{n}B"))
            .unwrap_or_else(|| "bytes=?".to_string());
        format!(
            "plan: `{}` slug=`{}` ({exists}, {active}, {bytes})",
            self.path, self.slug
        )
    }
}

/// Operator-facing counts for the plan list overlay (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlanViewSummary {
    pub total: usize,
    pub existing: usize,
    pub missing: usize,
    pub active: usize,
    pub preview_open: bool,
    pub total_bytes: u64,
}

impl PlanViewSummary {
    fn from_entries(entries: &[PlanProjectionEntry], preview_open: bool) -> Self {
        Self {
            total: entries.len(),
            existing: entries.iter().filter(|entry| entry.exists).count(),
            missing: entries.iter().filter(|entry| !entry.exists).count(),
            active: entries.iter().filter(|entry| entry.is_active).count(),
            preview_open,
            total_bytes: entries
                .iter()
                .filter_map(|entry| entry.byte_len)
                .fold(0, u64::saturating_add),
        }
    }

    pub fn one_line(&self) -> String {
        format!(
            "plan view: {} total (existing={}, missing={}, active={}, preview={}, bytes={})",
            self.total,
            self.existing,
            self.missing,
            self.active,
            if self.preview_open { "open" } else { "closed" },
            self.total_bytes
        )
    }

    /// Compact overlay subtitle for the plan list/preview header.
    pub fn overlay_line(&self) -> String {
        if self.preview_open {
            format!(
                "{} existing · {} missing · preview open",
                self.existing, self.missing
            )
        } else {
            format!(
                "{} total · {} existing · {} missing · {} active · {} bytes",
                self.total, self.existing, self.missing, self.active, self.total_bytes
            )
        }
    }

    pub const fn has_plans(&self) -> bool {
        self.existing > 0
    }
}

impl AppState {
    pub(in crate::app) fn open_plan_view(&mut self) {
        if self.replay_mode {
            self.status_banner = Some("plan mode is unavailable during replay".to_string());
            return;
        }

        self.plan_view_visible = true;
        self.prepare_plan_view();
        self.plan_view_selected = 0;
        self.plan_view_preview = None;
        self.status_banner = Some("plan mode entered".to_string());
        self.theme_dialog_visible = false;
        self.error_details_visible = false;
        self.prompt_stash.list_visible = false;
        self.palette_visible = false;
        self.session_history_visible = false;
        self.model_switcher_visible = false;
        self.toggles_menu_visible = false;
        self.lineage_browser_visible = false;
        self.fork_selector_visible = false;
        self.settings_editor_visible = false;
    }

    pub(in crate::app) fn close_plan_view(&mut self) {
        self.plan_view_visible = false;
        self.plan_view_preview = None;
    }

    pub(in crate::app) fn plan_view_move(&mut self, delta: isize) {
        self.plan_entries = self.plan_view_entries();
        if self.plan_entries.is_empty() {
            self.plan_view_selected = 0;
            return;
        }
        let last = self.plan_entries.len() - 1;
        self.plan_view_selected = self
            .plan_view_selected
            .min(last)
            .saturating_add_signed(delta)
            .min(last);
        self.plan_view_preview = None;
    }

    /// Open the selected plan file content into the overlay preview (Enter).
    pub fn plan_view_open_selected(&mut self) {
        let Some(entry) = self.selected_plan() else {
            return;
        };
        if !self.plan_exists(&entry) {
            self.plan_view_preview = None;
            return;
        }
        match fs::read_to_string(&entry.path) {
            Ok(mut body) => {
                if let Some((end, _)) = body.char_indices().nth(4_000) {
                    body.truncate(end);
                    body.push_str("\n… (truncated)");
                }
                self.plan_view_preview = Some(body);
            }
            Err(err) => {
                self.plan_view_preview = None;
                self.show_toast(
                    format!("failed to read plan `{}`: {err}", entry.slug),
                    ToastVariant::Error,
                );
            }
        }
    }

    pub fn plan_view_copy_selected_path(&mut self) {
        let Some(entry) = self.selected_plan() else {
            return;
        };
        let path_text = &entry.path;
        self.status_banner = Some(format!("plan path: {path_text}"));
        match crate::clipboard::copy(path_text) {
            Ok(()) => self.show_toast(format!("copied plan path: {path_text}"), ToastVariant::Info),
            Err(err) => self.show_toast(
                format!("plan path copy failed: {err} (path: {path_text})"),
                ToastVariant::Error,
            ),
        }
    }

    /// Copy the full selected file, falling back to its open preview on a read error.
    pub fn plan_view_copy_selected_body(&mut self) {
        let Some(entry) = self.selected_plan() else {
            return;
        };
        if !self.plan_exists(&entry) {
            return;
        }
        let body = match fs::read_to_string(&entry.path)
            .or_else(|err| self.plan_view_preview.clone().ok_or(err))
        {
            Ok(body) => body,
            Err(err) => {
                self.show_toast(
                    format!("failed to read plan `{}`: {err}", entry.slug),
                    ToastVariant::Error,
                );
                return;
            }
        };
        let chars = body.chars().count();
        self.status_banner = Some(format!("plan body: {} ({} chars)", entry.slug, chars));
        match crate::clipboard::copy(&body) {
            Ok(()) => self.show_toast(
                format!("copied plan body: {} ({} chars)", entry.slug, chars),
                ToastVariant::Info,
            ),
            Err(err) => self.show_toast(
                format!(
                    "plan body copy failed: {err} (plan: {}, {} chars)",
                    entry.slug, chars
                ),
                ToastVariant::Error,
            ),
        }
    }

    /// Delete the selected plan file from disk (existing plans only).
    pub fn plan_view_delete_selected(&mut self) {
        if self.plan_is_replay_mutation_blocked() {
            self.status_banner = Some("plan deletion is unavailable during replay".to_string());
            return;
        }
        let Some(entry) = self.selected_plan() else {
            return;
        };
        if !self.plan_exists(&entry) {
            return;
        }
        if let Err(err) = self.plan_validate_path(&entry.path) {
            self.status_banner = Some(format!("plan deletion rejected: {err}"));
            self.show_toast(
                format!("plan deletion rejected for `{}`: {err}", entry.slug),
                ToastVariant::Error,
            );
            return;
        }
        match fs::remove_file(&entry.path) {
            Ok(()) => {
                self.plan_view_preview = None;
                self.plan_entries = self.plan_view_entries();
                self.plan_view_selected = self
                    .plan_view_selected
                    .min(self.plan_entries.len().saturating_sub(1));
                self.status_banner = Some(format!("plan deleted: {}", entry.slug));
                self.show_toast(format!("deleted plan `{}`", entry.slug), ToastVariant::Info);
            }
            Err(err) => self.show_toast(
                format!("failed to delete plan `{}`: {err}", entry.slug),
                ToastVariant::Error,
            ),
        }
    }

    // Projection paths already include the runtime directory, even when it is relative.
    fn selected_plan(&mut self) -> Option<PlanProjectionEntry> {
        let runtime = self.project_runtime_dir()?;
        self.plan_entries = project_plan_list(&runtime, self.run_id());
        match self.plan_entries.get(self.plan_view_selected).cloned() {
            Some(entry) => Some(entry),
            None => {
                self.show_toast(
                    if self.plan_entries.is_empty() {
                        "no plan files yet in the project runtime plans directory"
                    } else {
                        "no plan selected"
                    }
                    .to_string(),
                    ToastVariant::Info,
                );
                None
            }
        }
    }

    fn plan_exists(&mut self, entry: &PlanProjectionEntry) -> bool {
        if !entry.exists {
            self.show_toast(
                format!("plan `{}` does not exist yet", entry.slug),
                ToastVariant::Info,
            );
        }
        entry.exists
    }

    /// Read the filesystem once before paint and hit testing share these entries.
    pub(super) fn prepare_plan_view(&mut self) {
        if self.plan_view_visible || self.status_dashboard_is_active() {
            self.plan_entries = self.plan_view_entries();
        } else {
            self.plan_entries.clear();
        }
    }

    /// Query current files, including changes since the last prepared frame.
    pub fn plan_view_rows(&self) -> Vec<PlanViewRow> {
        let selected = self.plan_view_selected;
        self.plan_view_entries()
            .into_iter()
            .enumerate()
            .map(|(index, entry)| PlanViewRow {
                path: entry.path,
                slug: entry.slug,
                exists: entry.exists,
                is_active: entry.is_active,
                selected: index == selected,
                byte_len: entry.byte_len,
            })
            .collect()
    }

    pub fn plan_view_selected_index(&self) -> usize {
        self.plan_view_selected
    }

    pub fn plan_view_is_visible(&self) -> bool {
        self.plan_view_visible
    }

    pub fn plan_view_preview(&self) -> Option<&str> {
        self.plan_view_preview.as_deref()
    }

    /// Query current files for diagnostics; painting uses the prepared snapshot.
    pub fn plan_view_summary(&self) -> PlanViewSummary {
        PlanViewSummary::from_entries(&self.plan_view_entries(), self.plan_view_preview.is_some())
    }

    pub(crate) fn prepared_plan_summary(&self) -> PlanViewSummary {
        PlanViewSummary::from_entries(&self.plan_entries, self.plan_view_preview.is_some())
    }

    /// Inject the same user-level storage root used by the coordinator and CLI.
    pub fn set_storage_data_dir(&mut self, data_dir: PathBuf) {
        self.storage_data_dir = Some(data_dir);
        self.plan_entries.clear();
        self.plan_view_preview = None;
    }

    pub(super) fn project_runtime_dir(&self) -> Option<PathBuf> {
        let root = self.file_mention_workspace_root_opt()?;
        harness_core::storage_paths::ProjectPaths::new(self.storage_data_dir.as_deref()?, &root)
            .ok()
            .map(|paths| paths.runtime_dir())
    }

    fn plan_view_entries(&self) -> Vec<PlanProjectionEntry> {
        self.project_runtime_dir()
            .map_or_else(Vec::new, |root| project_plan_list(&root, self.run_id()))
    }

    /// Accept only direct markdown children of the configured runtime plans directory.
    pub fn plan_validate_path(&self, path: &str) -> Result<(), String> {
        let runtime = self
            .project_runtime_dir()
            .ok_or("project storage is unavailable")?;
        let directory = runtime.join(PLAN_DIR);
        let path = Path::new(path);
        let filename = path
            .strip_prefix(&directory)
            .map_err(|_| "plan path is outside the project plans directory")?;
        let mut components = filename.components();
        if !matches!(components.next(), Some(Component::Normal(_)))
            || components.next().is_some()
            || filename.file_stem().is_none_or(|stem| stem.is_empty())
            || filename.extension() != Some(OsStr::new("md"))
            || directory.join(filename) != path
        {
            return Err("plan path must name a direct markdown file".into());
        }
        for component in path.ancestors().filter(|part| !part.as_os_str().is_empty()) {
            match fs::symlink_metadata(component) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err("plan path contains a symlink component".into());
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    }

    /// Whether plan mutations are blocked by replay mode.
    pub fn plan_is_replay_mutation_blocked(&self) -> bool {
        self.replay_mode
    }
}
