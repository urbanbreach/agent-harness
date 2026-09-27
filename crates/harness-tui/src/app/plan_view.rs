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
        let Some((workspace, entry)) = self.selected_plan() else {
            return;
        };
        if !self.plan_exists(&entry) {
            self.plan_view_preview = None;
            return;
        }
        match fs::read_to_string(workspace.join(&entry.path)) {
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
        let Some((workspace, entry)) = self.selected_plan() else {
            return;
        };
        let path_text = workspace.join(&entry.path).display().to_string();
        self.status_banner = Some(format!("plan path: {path_text}"));
        match crate::clipboard::copy(&path_text) {
            Ok(()) => self.show_toast(format!("copied plan path: {path_text}"), ToastVariant::Info),
            Err(err) => self.show_toast(
                format!("plan path copy failed: {err} (path: {path_text})"),
                ToastVariant::Error,
            ),
        }
    }

    /// Copy the full selected file, falling back to its open preview on a read error.
    pub fn plan_view_copy_selected_body(&mut self) {
        let Some((workspace, entry)) = self.selected_plan() else {
            return;
        };
        if !self.plan_exists(&entry) {
            return;
        }
        let body = match fs::read_to_string(workspace.join(&entry.path))
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
        let Some((workspace, entry)) = self.selected_plan() else {
            return;
        };
        if !self.plan_exists(&entry) {
            return;
        }
        let relative = Path::new(&entry.path);
        if let Err(err) = self
            .plan_validate_path(&entry.path)
            .and_then(|()| validate_plan_path_components(&workspace, relative))
        {
            self.status_banner = Some(format!("plan deletion rejected: {err}"));
            self.show_toast(
                format!("plan deletion rejected for `{}`: {err}", entry.slug),
                ToastVariant::Error,
            );
            return;
        }
        match fs::remove_file(workspace.join(relative)) {
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

    fn selected_plan(&mut self) -> Option<(PathBuf, PlanProjectionEntry)> {
        let workspace = self.plan_workspace();
        self.plan_entries = project_plan_list(&workspace, self.run_id());
        match self.plan_entries.get(self.plan_view_selected).cloned() {
            Some(entry) => Some((workspace, entry)),
            None => {
                self.show_toast(
                    if self.plan_entries.is_empty() {
                        "no plan files yet — write a plan under .omo/plans/"
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

    fn plan_workspace(&self) -> PathBuf {
        self.file_mention_workspace_root
            .clone()
            .or_else(|| (self.file_mention_workspace_root_provider)())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn plan_view_entries(&self) -> Vec<PlanProjectionEntry> {
        project_plan_list(&self.plan_workspace(), self.run_id())
    }

    /// Validate that a plan path is confined to `.agent-harness/plans/*.md`.
    ///
    /// Rejects path traversal, absolute paths, non-`.md` extensions, and paths
    /// outside the plans directory.
    pub fn plan_validate_path(&self, path: &str) -> Result<(), String> {
        let path = Path::new(path);
        if path.is_absolute() {
            return Err("plan path must be relative to the workspace".to_string());
        }

        let mut components = path.components();
        let Some(Component::Normal(root)) = components.next() else {
            return Err("plan path must be under `.agent-harness/plans/`".to_string());
        };
        let Some(Component::Normal(plans)) = components.next() else {
            return Err("plan path must be under `.agent-harness/plans/`".to_string());
        };
        let Some(Component::Normal(filename)) = components.next() else {
            return Err("plan path must name a markdown file".to_string());
        };
        if components.next().is_some()
            || root != OsStr::new(".agent-harness")
            || plans != OsStr::new("plans")
        {
            return Err("plan path must be confined to `.agent-harness/plans/`".to_string());
        }

        let filename = filename
            .to_str()
            .ok_or_else(|| "plan filename must be valid UTF-8".to_string())?;
        let filename_path = Path::new(filename);
        let Some(stem) = filename_path.file_stem() else {
            return Err("plan filename must not be empty".to_string());
        };
        if stem.is_empty() || filename_path.extension() != Some(OsStr::new("md")) {
            return Err("plan path must end with a non-empty `.md` filename".to_string());
        }

        let canonical = Path::new(PLAN_DIR).join(filename);
        if path != canonical {
            return Err("plan path must use its canonical relative form".to_string());
        }
        Ok(())
    }

    /// Whether plan mutations are blocked by replay mode.
    pub fn plan_is_replay_mutation_blocked(&self) -> bool {
        self.replay_mode
    }
}

/// Reject symlink components before creating or replacing the active plan.
/// The coordinator applies the same boundary to agent edits; keeping the TUI
/// writer fail-closed prevents a presentation-layer write from bypassing it.
fn validate_plan_path_components(workspace: &Path, relative: &Path) -> Result<(), String> {
    let mut current = workspace.to_path_buf();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err("plan path contains an invalid component".to_string());
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "plan path contains symlink component `{}`",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(format!(
                    "cannot verify plan path component `{}`: {err}",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}
