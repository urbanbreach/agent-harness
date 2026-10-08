use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const PLAN_DIR: &str = "plans";
/// Resolve a run plan beneath the caller-provided project runtime directory.
pub fn plan_file_path(runtime_dir: &Path, run_id: &str) -> PathBuf {
    let slug = run_id
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    runtime_dir.join(PLAN_DIR).join(format!(
        "{}.md",
        if slug.is_empty() { "plan" } else { &slug }
    ))
}
pub fn plan_file_display_path(runtime_dir: &Path, run_id: &str) -> String {
    plan_file_path(runtime_dir, run_id)
        .to_string_lossy()
        .into_owned()
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanProjectionEntry {
    pub path: String,
    pub slug: String,
    pub exists: bool,
    pub is_active: bool,
    pub byte_len: Option<u64>,
}
impl PlanProjectionEntry {
    pub fn one_line(&self) -> String {
        format!(
            "plan: `{}` slug=`{}` ({}, {}, {})",
            self.path,
            self.slug,
            if self.exists { "exists" } else { "missing" },
            if self.is_active { "active" } else { "inactive" },
            self.byte_len
                .map_or_else(|| "bytes=?".into(), |n| format!("{n}B"))
        )
    }
}
pub fn project_plan_list(root: &Path, active_run_id: Option<&str>) -> Vec<PlanProjectionEntry> {
    let directory = root.join(PLAN_DIR);
    let active = active_run_id.map(|run| plan_file_display_path(root, run));
    let mut entries = Vec::new();
    if crate::store::validate_private_path(&directory).is_ok()
        && fs::symlink_metadata(&directory).is_ok_and(|m| m.is_dir())
    {
        for entry in fs::read_dir(&directory).into_iter().flatten().flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(slug) = name.strip_suffix(".md") else {
                continue;
            };
            let path = directory.join(name).to_string_lossy().into_owned();
            entries.push(PlanProjectionEntry {
                is_active: active.as_ref() == Some(&path),
                path,
                slug: slug.into(),
                exists: true,
                byte_len: entry.metadata().ok().map(|m| m.len()),
            });
        }
    }
    if let Some(path) = active.filter(|_| !entries.iter().any(|e| e.is_active)) {
        let slug = Path::new(&path)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        entries.push(PlanProjectionEntry {
            path,
            slug,
            exists: false,
            is_active: true,
            byte_len: None,
        });
    }
    entries.sort_by(|a, b| {
        b.is_active
            .cmp(&a.is_active)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    entries
}
