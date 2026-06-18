//! Pure helper logic shared by the real OpenCode-parity discovery rules.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::coord::formatter::find_up::{find_up, read_text_file};

#[derive(Deserialize)]
pub(crate) struct PackageJson {
    pub(crate) dependencies: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(rename = "devDependencies")]
    pub(crate) dev_dependencies: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
pub(crate) struct ComposerJson {
    pub(crate) require: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(rename = "require-dev")]
    pub(crate) require_dev: Option<BTreeMap<String, serde_json::Value>>,
}

pub(crate) fn has_dep(map: Option<&BTreeMap<String, serde_json::Value>>, name: &str) -> bool {
    map.is_some_and(|deps| deps.contains_key(name))
}

pub(crate) fn first_line(text: &str) -> &str {
    text.lines().next().map_or("", |line| line)
}

pub(crate) async fn nearest_marker_dir(
    names: &[&str],
    start_dir: &Path,
    worktree_root: &Path,
) -> Option<PathBuf> {
    let mut best: Option<(usize, usize, PathBuf, PathBuf)> = None;

    for (index, name) in names.iter().enumerate() {
        for marker in find_up(name, start_dir, worktree_root).await {
            let dir = marker.parent()?.to_path_buf();
            let depth = dir.components().count();
            match best {
                Some((best_depth, best_index, _, _))
                    if (depth, index) >= (best_depth, best_index) => {}
                _ => best = Some((depth, index, dir, marker)),
            }
        }
    }

    best.map(|(_, _, dir, _)| dir)
}

pub(crate) async fn text_marker_contains(
    names: &[&str],
    needle: &str,
    start_dir: &Path,
    worktree_root: &Path,
) -> bool {
    for name in names {
        for marker in find_up(name, start_dir, worktree_root).await {
            if let Some(text) = read_text_file(&marker).await {
                if text.contains(needle) {
                    return true;
                }
            }
        }
    }
    false
}
