use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use globset::Glob;
use harness_core::tool::{ToolContext, ToolError};
use walkdir::WalkDir;

use crate::fs_walk::{normalize_base_relative_path, normalize_relative_path, should_skip_entry};
use crate::workspace_paths::resolve_existing_path;

pub(super) fn resolve_directory_path(ctx: &ToolContext, input: &str) -> Result<PathBuf, ToolError> {
    let path = resolve_existing_path(ctx, input)?;
    if !path.is_dir() {
        return Err(ToolError::InvalidArguments(format!(
            "path must resolve to a directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

pub(super) struct RenderedTree {
    pub(super) rendered: String,
    pub(super) count: usize,
    pub(super) truncated: bool,
}

pub(super) fn build_recursive_tree(
    ctx: &ToolContext,
    root: &Path,
    ignore: &[String],
    limit: usize,
) -> Result<RenderedTree, ToolError> {
    let ignore_matchers = ignore
        .iter()
        .filter_map(|pattern| Glob::new(pattern).ok().map(|glob| glob.compile_matcher()))
        .collect::<Vec<_>>();
    let mut child_dirs_by_dir = BTreeMap::<String, BTreeSet<String>>::new();
    let mut files_by_dir = BTreeMap::<String, BTreeSet<String>>::new();
    let mut count = 0usize;
    let mut truncated = false;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(&ctx.workspace_root, entry))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let rel = normalize_base_relative_path(root, entry.path());
        if ignore_matchers.iter().any(|matcher| matcher.is_match(&rel)) {
            continue;
        }
        count += 1;
        if count > limit {
            truncated = true;
            break;
        }
        let dir = Path::new(&rel)
            .parent()
            .map(normalize_relative_path)
            .unwrap_or_else(|| ".".to_string());
        register_tree_dirs(&mut child_dirs_by_dir, &dir);
        files_by_dir.entry(dir.clone()).or_default().insert(
            Path::new(&rel)
                .file_name()
                .unwrap_or_else(|| OsStr::new(&rel))
                .to_string_lossy()
                .to_string(),
        );
    }

    fn render_dir(
        dir_path: &str,
        child_dirs_by_dir: &BTreeMap<String, BTreeSet<String>>,
        files_by_dir: &BTreeMap<String, BTreeSet<String>>,
        depth: usize,
    ) -> String {
        let indent = "  ".repeat(depth);
        let mut output = String::new();
        if depth > 0 {
            output.push_str(&format!(
                "{}{}/\n",
                indent,
                Path::new(dir_path)
                    .file_name()
                    .unwrap_or_else(|| OsStr::new(dir_path))
                    .to_string_lossy()
            ));
        }
        let child_indent = "  ".repeat(depth + 1);
        if let Some(children) = child_dirs_by_dir.get(dir_path) {
            for child in children {
                output.push_str(&render_dir(
                    child,
                    child_dirs_by_dir,
                    files_by_dir,
                    depth + 1,
                ));
            }
        }
        if let Some(files) = files_by_dir.get(dir_path) {
            for file in files {
                output.push_str(&format!("{child_indent}{file}\n"));
            }
        }
        output
    }

    let mut rendered = format!("{}/\n", root.display());
    rendered.push_str(&render_dir(".", &child_dirs_by_dir, &files_by_dir, 0));
    Ok(RenderedTree {
        rendered: rendered.trim_end().to_string(),
        count: count.min(limit),
        truncated,
    })
}

fn register_tree_dirs(child_dirs_by_dir: &mut BTreeMap<String, BTreeSet<String>>, dir: &str) {
    child_dirs_by_dir.entry(".".to_string()).or_default();
    if dir == "." {
        return;
    }

    let parts = dir.split('/').collect::<Vec<_>>();
    for idx in 0..parts.len() {
        let parent = if idx == 0 {
            ".".to_string()
        } else {
            parts[..idx].join("/")
        };
        let child = parts[..=idx].join("/");
        child_dirs_by_dir
            .entry(parent)
            .or_default()
            .insert(child.clone());
        child_dirs_by_dir.entry(child).or_default();
    }
}
