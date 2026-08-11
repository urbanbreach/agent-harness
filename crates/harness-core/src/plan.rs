use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const PLAN_DIR: &str = ".agent-harness/plans";

pub fn plan_file_relative_path(run_id: &str) -> PathBuf {
    Path::new(PLAN_DIR).join(format!("{}.md", sanitize_plan_slug(run_id)))
}

pub fn plan_file_display_path(run_id: &str) -> String {
    plan_file_relative_path(run_id)
        .to_string_lossy()
        .to_string()
}

/// One plan file entry for TUI/CLI projection (read-only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanProjectionEntry {
    pub path: String,
    pub slug: String,
    pub exists: bool,
    pub is_active: bool,
    pub byte_len: Option<u64>,
}

impl PlanProjectionEntry {
    /// Operator-facing one-line plan entry (read-only projection; not plan-edit product).
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

/// Project plan files under `.agent-harness/plans/` for optional active run.
pub fn project_plan_list(
    workspace_root: &Path,
    active_run_id: Option<&str>,
) -> Vec<PlanProjectionEntry> {
    let active_slug = active_run_id.map(sanitize_plan_slug);
    let plans_dir = workspace_root.join(PLAN_DIR);
    let mut entries = Vec::new();

    if let Ok(read_dir) = fs::read_dir(&plans_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".md") {
                continue;
            }
            let slug = name.trim_end_matches(".md").to_string();
            let relative = Path::new(PLAN_DIR).join(name);
            let byte_len = entry.metadata().ok().map(|m| m.len());
            let is_active = active_slug.as_ref().is_some_and(|s| s == &slug);
            entries.push(PlanProjectionEntry {
                path: relative.to_string_lossy().replace('\\', "/"),
                slug,
                exists: true,
                is_active,
                byte_len,
            });
        }
    }

    if let Some(run_id) = active_run_id {
        let slug = sanitize_plan_slug(run_id);
        if !entries.iter().any(|e| e.slug == slug) {
            let relative = plan_file_display_path(run_id);
            let absolute = workspace_root.join(&relative);
            let exists = absolute.is_file();
            let byte_len = exists
                .then(|| fs::metadata(&absolute).ok().map(|m| m.len()))
                .flatten();
            entries.push(PlanProjectionEntry {
                path: relative,
                slug,
                exists,
                is_active: true,
                byte_len,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.is_active
            .cmp(&a.is_active)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    entries
}

fn sanitize_plan_slug(value: &str) -> String {
    let slug = value.trim().chars().fold(String::new(), |mut slug, ch| {
        let normalized = if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            ch
        } else {
            '-'
        };
        if normalized != '-' || !slug.ends_with('-') {
            slug.push(normalized);
        }
        slug
    });
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "plan".to_string()
    } else {
        slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_file_path_is_workspace_relative_and_sanitized() {
        // arrange
        // act
        // assert
        assert_eq!(
            plan_file_display_path("run/with spaces"),
            ".agent-harness/plans/run-with-spaces.md"
        );
        assert_eq!(
            plan_file_display_path(" run//with   spaces "),
            ".agent-harness/plans/run-with-spaces.md"
        );
        assert_eq!(
            plan_file_display_path("---"),
            ".agent-harness/plans/plan.md"
        );
    }

    #[test]
    fn project_plan_list_includes_existing_and_active_missing() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path();
        let plans = root.join(PLAN_DIR);
        fs::create_dir_all(&plans).expect("plans dir");
        fs::write(plans.join("older.md"), "# old\n").expect("write");

        let list = project_plan_list(root, Some("active-run"));
        assert!(list.iter().any(|e| e.slug == "older" && e.exists));
        let active = list
            .iter()
            .find(|e| e.slug == "active-run")
            .expect("active placeholder");
        assert!(active.is_active);
        assert!(!active.exists);
        assert!(list[0].is_active);
    }
}
