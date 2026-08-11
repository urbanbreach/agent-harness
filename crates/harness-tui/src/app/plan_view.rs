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
        let len = self.plan_view_entries().len();
        if len == 0 {
            self.plan_view_selected = 0;
            return;
        }
        let current = isize::try_from(self.plan_view_selected.min(len - 1)).unwrap_or(0);
        let next = (current + delta).clamp(0, isize::try_from(len - 1).unwrap_or(0));
        self.plan_view_selected = usize::try_from(next).unwrap_or(0);
        self.plan_view_preview = None;
    }

    /// Open the selected plan file content into the overlay preview (Enter).
    pub fn plan_view_open_selected(&mut self) {
        let entries = self.plan_view_entries();
        if entries.is_empty() {
            self.show_toast(
                "no plan files yet — write a plan under .omo/plans/".to_string(),
                ToastVariant::Info,
            );
            return;
        }
        let Some(entry) = entries.get(self.plan_view_selected) else {
            self.show_toast("no plan selected".to_string(), ToastVariant::Info);
            return;
        };
        if !entry.exists {
            self.show_toast(
                format!("plan `{}` does not exist yet", entry.slug),
                ToastVariant::Info,
            );
            self.plan_view_preview = None;
            return;
        }
        let workspace = self
            .file_mention_workspace_root
            .clone()
            .or_else(|| (self.file_mention_workspace_root_provider)())
            .unwrap_or_else(|| PathBuf::from("."));
        let absolute = workspace.join(&entry.path);
        match fs::read_to_string(&absolute) {
            Ok(body) => {
                let preview = if body.chars().count() > 4_000 {
                    let truncated: String = body.chars().take(4_000).collect();
                    format!("{truncated}\n… (truncated)")
                } else {
                    body
                };
                self.plan_view_preview = Some(preview);
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
        let entries = self.plan_view_entries();
        if entries.is_empty() {
            self.show_toast(
                "no plan files yet — write a plan under .omo/plans/".to_string(),
                ToastVariant::Info,
            );
            return;
        }
        let Some(entry) = entries.get(self.plan_view_selected) else {
            self.show_toast("no plan selected".to_string(), ToastVariant::Info);
            return;
        };
        let workspace = self
            .file_mention_workspace_root
            .clone()
            .or_else(|| (self.file_mention_workspace_root_provider)())
            .unwrap_or_else(|| PathBuf::from("."));
        let absolute = workspace.join(&entry.path);
        let path_text = absolute.display().to_string();
        self.status_banner = Some(format!("plan path: {path_text}"));
        match crate::clipboard::copy(&path_text) {
            Ok(()) => self.show_toast(format!("copied plan path: {path_text}"), ToastVariant::Info),
            Err(err) => self.show_toast(
                format!("plan path copy failed: {err} (path: {path_text})"),
                ToastVariant::Error,
            ),
        }
    }

    /// Copy selected plan file body to clipboard (full file when present; preview text if open).
    pub fn plan_view_copy_selected_body(&mut self) {
        let entries = self.plan_view_entries();
        if entries.is_empty() {
            self.show_toast(
                "no plan files yet — write a plan under .omo/plans/".to_string(),
                ToastVariant::Info,
            );
            return;
        }
        let Some(entry) = entries.get(self.plan_view_selected) else {
            self.show_toast("no plan selected".to_string(), ToastVariant::Info);
            return;
        };
        if !entry.exists {
            self.show_toast(
                format!("plan `{}` does not exist yet", entry.slug),
                ToastVariant::Info,
            );
            return;
        }

        let body = if let Some(preview) = self.plan_view_preview.as_ref() {
            // Prefer full file over truncated preview when possible.
            let workspace = self
                .file_mention_workspace_root
                .clone()
                .or_else(|| (self.file_mention_workspace_root_provider)())
                .unwrap_or_else(|| PathBuf::from("."));
            let absolute = workspace.join(&entry.path);
            fs::read_to_string(&absolute).unwrap_or_else(|_| preview.clone())
        } else {
            let workspace = self
                .file_mention_workspace_root
                .clone()
                .or_else(|| (self.file_mention_workspace_root_provider)())
                .unwrap_or_else(|| PathBuf::from("."));
            let absolute = workspace.join(&entry.path);
            match fs::read_to_string(&absolute) {
                Ok(body) => body,
                Err(err) => {
                    self.show_toast(
                        format!("failed to read plan `{}`: {err}", entry.slug),
                        ToastVariant::Error,
                    );
                    return;
                }
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

        let entries = self.plan_view_entries();
        if entries.is_empty() {
            self.show_toast(
                "no plan files yet — write a plan under .omo/plans/".to_string(),
                ToastVariant::Info,
            );
            return;
        }
        let Some(entry) = entries.get(self.plan_view_selected).cloned() else {
            self.show_toast("no plan selected".to_string(), ToastVariant::Info);
            return;
        };
        if !entry.exists {
            self.show_toast(
                format!("plan `{}` does not exist yet", entry.slug),
                ToastVariant::Info,
            );
            return;
        }
        let workspace = self
            .file_mention_workspace_root
            .clone()
            .or_else(|| (self.file_mention_workspace_root_provider)())
            .unwrap_or_else(|| PathBuf::from("."));
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
        let absolute = workspace.join(relative);
        match fs::remove_file(&absolute) {
            Ok(()) => {
                self.plan_view_preview = None;
                let remaining = self.plan_view_entries().len();
                if remaining == 0 {
                    self.plan_view_selected = 0;
                } else if self.plan_view_selected >= remaining {
                    self.plan_view_selected = remaining - 1;
                }
                self.status_banner = Some(format!("plan deleted: {}", entry.slug));
                self.show_toast(format!("deleted plan `{}`", entry.slug), ToastVariant::Info);
            }
            Err(err) => {
                self.show_toast(
                    format!("failed to delete plan `{}`: {err}", entry.slug),
                    ToastVariant::Error,
                );
            }
        }
    }

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

    pub fn plan_view_summary(&self) -> PlanViewSummary {
        let entries = self.plan_view_entries();
        let mut summary = PlanViewSummary {
            total: entries.len(),
            preview_open: self.plan_view_preview.is_some(),
            ..PlanViewSummary::default()
        };
        for entry in &entries {
            if entry.exists {
                summary.existing = summary.existing.saturating_add(1);
            } else {
                summary.missing = summary.missing.saturating_add(1);
            }
            if entry.is_active {
                summary.active = summary.active.saturating_add(1);
            }
            if let Some(bytes) = entry.byte_len {
                summary.total_bytes = summary.total_bytes.saturating_add(bytes);
            }
        }
        summary
    }

    fn plan_view_entries(&self) -> Vec<PlanProjectionEntry> {
        let workspace = self
            .file_mention_workspace_root
            .clone()
            .or_else(|| (self.file_mention_workspace_root_provider)())
            .unwrap_or_else(|| PathBuf::from("."));
        let active_run = self.run_id();
        project_plan_list(&workspace, active_run)
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
