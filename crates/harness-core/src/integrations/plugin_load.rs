//! Validates data entrypoints and records explicit activation. It never executes package code.
use crate::store;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};
pub const PLUGIN_LOAD_RECEIPT_FILE_NAME: &str = ".harness-plugin-load-receipt.json";
pub const PLUGIN_ENTRY_FILE_NAME: &str = "plugin_entry.json";
pub const PLUGIN_HOOKS_FILE_NAME: &str = "hooks.json";
pub const PLUGIN_SKILLS_DIR_NAME: &str = "skills";
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginLoadKind {
    PluginEntryJson,
    HooksJson,
    SkillsDir,
}
impl PluginLoadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PluginEntryJson => "plugin_entry_json",
            Self::HooksJson => "hooks_json",
            Self::SkillsDir => "skills_dir",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadedCode {
    pub kinds: Vec<PluginLoadKind>,
    pub entry_paths: Vec<String>,
    pub entrypoints: Vec<String>,
    pub receipt_path: String,
}
impl LoadedCode {
    pub fn loads_code(&self) -> bool {
        !self.kinds.is_empty()
    }
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PluginLoadError {
    #[error("plugin entry invalid at {path}: {message}")]
    EntryInvalid { path: String, message: String },
    #[error("plugin entrypoint missing at {path}")]
    EntrypointMissing { path: String },
    #[error("cannot write plugin load receipt at {path}: {message}")]
    ReceiptWrite { path: String, message: String },
    #[error("cannot read plugin entry at {path}: {message}")]
    EntryRead { path: String, message: String },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    schema_version: String,
    entrypoints: Vec<String>,
}
pub fn load_package_entries(root: &Path, id: &str) -> Result<Option<LoadedCode>, PluginLoadError> {
    let loaded = inspect_package(root)?;
    if let Some(loaded) = &loaded {
        write_receipt(root, id, loaded)?;
    }
    Ok(loaded)
}
pub fn clear_package_load_receipt(root: &Path) {
    let _ = replace_receipt(&root.join(PLUGIN_LOAD_RECEIPT_FILE_NAME), None);
}
pub(super) fn inspect_package(root: &Path) -> Result<Option<LoadedCode>, PluginLoadError> {
    let mut kinds = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut entries = BTreeSet::new();
    let path = root.join(PLUGIN_ENTRY_FILE_NAME);
    if let Some(bytes) = read(&path)? {
        let entry: Entry =
            serde_json::from_slice(&bytes).map_err(|_| invalid(&path, "invalid entry JSON"))?;
        if entry.schema_version != "plugin.entry.v1"
            || entry.entrypoints.is_empty()
            || entry.entrypoints.len() > 256
        {
            return Err(invalid(&path, "unsupported schema or entrypoint count"));
        }
        kinds.insert(PluginLoadKind::PluginEntryJson);
        paths.insert(path.display().to_string());
        for entry in entry.entrypoints {
            let path = local_entry(root, &entry)?;
            if path.is_dir() {
                if entry != PLUGIN_SKILLS_DIR_NAME {
                    return Err(invalid(
                        &path,
                        "only the skills directory is a directory entrypoint",
                    ));
                }
                skills(&path)?;
                kinds.insert(PluginLoadKind::SkillsDir);
            } else {
                let bytes = read(&path)?.ok_or_else(|| PluginLoadError::EntrypointMissing {
                    path: path.display().to_string(),
                })?;
                if entry == PLUGIN_HOOKS_FILE_NAME {
                    hooks(&path, &bytes)?;
                    kinds.insert(PluginLoadKind::HooksJson);
                }
            }
            entries.insert(entry);
            paths.insert(path.display().to_string());
        }
    }
    let hooks_path = root.join(PLUGIN_HOOKS_FILE_NAME);
    if let Some(bytes) = read(&hooks_path)? {
        hooks(&hooks_path, &bytes)?;
        kinds.insert(PluginLoadKind::HooksJson);
        paths.insert(hooks_path.display().to_string());
        entries.insert(PLUGIN_HOOKS_FILE_NAME.into());
    }
    let skills_path = root.join(PLUGIN_SKILLS_DIR_NAME);
    match fs::symlink_metadata(&skills_path) {
        Ok(_) => {
            skills(&skills_path)?;
            kinds.insert(PluginLoadKind::SkillsDir);
            paths.insert(skills_path.display().to_string());
            entries.insert(PLUGIN_SKILLS_DIR_NAME.into());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(invalid(&skills_path, "cannot inspect skills directory")),
    }
    if kinds.is_empty() {
        return Ok(None);
    }
    Ok(Some(LoadedCode {
        kinds: kinds.into_iter().collect(),
        entry_paths: paths.into_iter().collect(),
        entrypoints: entries.into_iter().collect(),
        receipt_path: root
            .join(PLUGIN_LOAD_RECEIPT_FILE_NAME)
            .display()
            .to_string(),
    }))
}
pub(super) fn receipt_bytes(id: &str, loaded: &LoadedCode) -> Result<Vec<u8>, PluginLoadError> {
    serde_json::to_vec(&serde_json::json!({"schemaVersion":"plugin.load.receipt.v1", "pluginId":id, "kinds":loaded.kinds, "entryPaths":loaded.entry_paths, "entrypoints":loaded.entrypoints})).map_err(|_| invalid(Path::new(&loaded.receipt_path), "cannot encode receipt"))
}
fn write_receipt(root: &Path, id: &str, loaded: &LoadedCode) -> Result<(), PluginLoadError> {
    replace_receipt(
        &root.join(PLUGIN_LOAD_RECEIPT_FILE_NAME),
        Some(&receipt_bytes(id, loaded)?),
    )
}
pub(super) fn replace_receipt(path: &Path, bytes: Option<&[u8]>) -> Result<(), PluginLoadError> {
    store::validate_private_path(path).map_err(|e| invalid(path, &e.to_string()))?;
    let result = match bytes {
        Some(bytes) => store::write_private_atomic(path, bytes),
        None => match fs::remove_file(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        },
    };
    result.map_err(|e| PluginLoadError::ReceiptWrite {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, PluginLoadError> {
    store::read_private_bytes(path, 1024 * 1024).map_err(|e| PluginLoadError::EntryRead {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}
fn hooks(path: &Path, bytes: &[u8]) -> Result<(), PluginLoadError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| invalid(path, "invalid hooks JSON"))?;
    if !value.is_object() && !value.is_array() {
        return Err(invalid(path, "hooks must be a JSON object or array"));
    }
    Ok(())
}
fn skills(path: &Path) -> Result<(), PluginLoadError> {
    store::validate_private_path(path).map_err(|e| invalid(path, &e.to_string()))?;
    if !path.is_dir() {
        return Err(invalid(path, "skills must be a directory"));
    }
    let walker = walkdir::WalkDir::new(path).follow_links(false);
    let mut count = 0;
    let mut bytes = 0;
    for (visited, entry) in walker.into_iter().enumerate() {
        let entry = entry.map_err(|_| invalid(path, "cannot enumerate skills"))?;
        if visited >= 4096 || entry.depth() > 64 {
            return Err(invalid(
                path,
                "skills exceed the directory count or depth limit",
            ));
        }
        if entry.file_type().is_symlink() {
            return Err(invalid(path, "skill paths cannot be symlinks"));
        }
        if !entry.file_type().is_file() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        count += 1;
        bytes += read(entry.path())?.map_or(0, |b| b.len());
        if count > 256 || bytes > 4 * 1024 * 1024 {
            return Err(invalid(path, "skills exceed 256 files or 4 MiB"));
        }
    }
    if count == 0 {
        return Err(invalid(path, "skills directory contains no files"));
    }
    Ok(())
}
pub(super) fn local_entry(root: &Path, name: &str) -> Result<PathBuf, PluginLoadError> {
    if name.is_empty()
        || name.len() > 4096
        || name.chars().any(char::is_control)
        || name.contains('\\')
        || !Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(invalid(
            root,
            "entrypoint must be a relative path under the package",
        ));
    }
    let path = root.join(name);
    store::validate_private_path(&path)
        .map_err(|_| invalid(root, "entry paths cannot be symlinks"))?;
    Ok(path)
}
fn invalid(path: &Path, message: &str) -> PluginLoadError {
    PluginLoadError::EntryInvalid {
        path: path.display().to_string(),
        message: message.into(),
    }
}
