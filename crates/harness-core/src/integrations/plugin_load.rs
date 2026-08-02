//! Package entry loading for plugin activation (not dynamic .so/wasm exec).
//!
//! Recognized loadable package entries under a package root:
//! - `plugin_entry.json` with declared `entrypoints`
//! - `hooks.json`
//! - `skills/` directory with at least one skill entry
//!
//! Activation writes a load receipt under the package root. Deactivation removes it.
//! No native/shared-library or wasm execution is performed.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// On-disk load receipt written under the package root on successful activate load.
pub const PLUGIN_LOAD_RECEIPT_FILE_NAME: &str = ".harness-plugin-load-receipt.json";

/// Declared package entry file for explicit entrypoints.
pub const PLUGIN_ENTRY_FILE_NAME: &str = "plugin_entry.json";

/// Optional hooks package entry.
pub const PLUGIN_HOOKS_FILE_NAME: &str = "hooks.json";

/// Optional skills package directory.
pub const PLUGIN_SKILLS_DIR_NAME: &str = "skills";

const PLUGIN_ENTRY_SCHEMA_VERSION: &str = "plugin.entry.v1";
const PLUGIN_LOAD_RECEIPT_SCHEMA_VERSION: &str = "plugin.load.receipt.v1";

/// Kind of package entry that was loaded into host state (filesystem-backed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginLoadKind {
    PluginEntryJson,
    HooksJson,
    SkillsDir,
}

impl PluginLoadKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PluginEntryJson => "plugin_entry_json",
            Self::HooksJson => "hooks_json",
            Self::SkillsDir => "skills_dir",
        }
    }
}

/// Result of loading package code entries during activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// Fail-closed errors while discovering or loading package entries.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginLoadError {
    #[error("plugin entry invalid at {path}: {message}")]
    EntryInvalid { path: String, message: String },
    #[error("plugin entrypoint missing at {path}")]
    EntrypointMissing { path: String },
    #[error("failed to write plugin load receipt at {path}: {message}")]
    ReceiptWrite { path: String, message: String },
    #[error("failed to read package entry at {path}: {message}")]
    EntryRead { path: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginEntryFile {
    schema_version: String,
    entrypoints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginLoadReceipt {
    schema_version: String,
    plugin_id: String,
    kinds: Vec<PluginLoadKind>,
    entry_paths: Vec<String>,
    entrypoints: Vec<String>,
}

/// Discover and load package entries under `package_root`.
///
/// Returns `Ok(None)` when the package has no recognized loadable entries
/// (descriptor-only packages remain valid).
/// Returns `Err` when an entry is present but invalid/missing declared content.
pub fn load_package_entries(
    package_root: &Path,
    plugin_id: &str,
) -> Result<Option<LoadedCode>, PluginLoadError> {
    let mut kinds = Vec::new();
    let mut entry_paths = Vec::new();
    let mut entrypoints = Vec::new();

    let entry_path = package_root.join(PLUGIN_ENTRY_FILE_NAME);
    if entry_path.exists() {
        if !entry_path.is_file() {
            return Err(PluginLoadError::EntryInvalid {
                path: entry_path.display().to_string(),
                message: "plugin_entry.json must be a file".to_string(),
            });
        }
        let declared = load_plugin_entry_file(&entry_path)?;
        push_unique_kind(&mut kinds, PluginLoadKind::PluginEntryJson);
        push_unique_str(&mut entry_paths, entry_path.display().to_string());
        for relative in &declared.entrypoints {
            let resolved = resolve_declared_entrypoint(package_root, relative)?;
            push_unique_str(&mut entrypoints, relative.clone());
            push_unique_str(&mut entry_paths, resolved.display().to_string());
            record_kind_for_path(package_root, &resolved, &mut kinds);
        }
    }

    let hooks_path = package_root.join(PLUGIN_HOOKS_FILE_NAME);
    if hooks_path.exists() {
        load_hooks_json(&hooks_path)?;
        push_unique_kind(&mut kinds, PluginLoadKind::HooksJson);
        push_unique_str(&mut entry_paths, hooks_path.display().to_string());
        push_unique_str(&mut entrypoints, PLUGIN_HOOKS_FILE_NAME.to_string());
    }

    let skills_path = package_root.join(PLUGIN_SKILLS_DIR_NAME);
    if skills_path.exists() {
        load_skills_dir(&skills_path)?;
        push_unique_kind(&mut kinds, PluginLoadKind::SkillsDir);
        push_unique_str(&mut entry_paths, skills_path.display().to_string());
        push_unique_str(&mut entrypoints, PLUGIN_SKILLS_DIR_NAME.to_string());
    }

    if kinds.is_empty() {
        return Ok(None);
    }

    let receipt_path = package_root.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    write_load_receipt(
        &receipt_path,
        &PluginLoadReceipt {
            schema_version: PLUGIN_LOAD_RECEIPT_SCHEMA_VERSION.to_string(),
            plugin_id: plugin_id.to_string(),
            kinds: kinds.clone(),
            entry_paths: entry_paths.clone(),
            entrypoints: entrypoints.clone(),
        },
    )?;

    Ok(Some(LoadedCode {
        kinds,
        entry_paths,
        entrypoints,
        receipt_path: receipt_path.display().to_string(),
    }))
}

/// Remove the load receipt written by [`load_package_entries`], if present.
pub fn clear_package_load_receipt(package_root: &Path) {
    let receipt_path = package_root.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    let _ = fs::remove_file(receipt_path);
}

fn load_plugin_entry_file(path: &Path) -> Result<PluginEntryFile, PluginLoadError> {
    let raw = read_entry(path)?;
    let parsed: PluginEntryFile =
        serde_json::from_str(&raw).map_err(|err| entry_invalid(path, &err.to_string()))?;
    if parsed.schema_version != PLUGIN_ENTRY_SCHEMA_VERSION {
        return Err(entry_invalid(
            path,
            &format!(
                "unsupported schemaVersion `{}`; expected `{PLUGIN_ENTRY_SCHEMA_VERSION}`",
                parsed.schema_version
            ),
        ));
    }
    if parsed.entrypoints.is_empty() {
        return Err(entry_invalid(path, "entrypoints must be a non-empty array"));
    }
    for entry in &parsed.entrypoints {
        if entry.trim().is_empty() {
            return Err(entry_invalid(
                path,
                "entrypoints must not contain empty strings",
            ));
        }
        if entry.contains("..") || Path::new(entry).is_absolute() {
            return Err(entry_invalid(
                path,
                &format!("entrypoint `{entry}` must be a relative path under package root"),
            ));
        }
    }
    Ok(parsed)
}

fn resolve_declared_entrypoint(
    package_root: &Path,
    relative: &str,
) -> Result<PathBuf, PluginLoadError> {
    let resolved = package_root.join(relative);
    if !resolved.exists() {
        return Err(PluginLoadError::EntrypointMissing {
            path: resolved.display().to_string(),
        });
    }
    Ok(resolved)
}

fn record_kind_for_path(package_root: &Path, path: &Path, kinds: &mut Vec<PluginLoadKind>) {
    if path == package_root.join(PLUGIN_HOOKS_FILE_NAME) {
        push_unique_kind(kinds, PluginLoadKind::HooksJson);
    }
    if path == package_root.join(PLUGIN_SKILLS_DIR_NAME) {
        push_unique_kind(kinds, PluginLoadKind::SkillsDir);
    }
}

fn push_unique_kind(kinds: &mut Vec<PluginLoadKind>, kind: PluginLoadKind) {
    if !kinds.contains(&kind) {
        kinds.push(kind);
    }
}

fn push_unique_str(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|item| item == &value) {
        items.push(value);
    }
}

fn load_hooks_json(path: &Path) -> Result<(), PluginLoadError> {
    if !path.is_file() {
        return Err(entry_invalid(path, "hooks.json must be a file"));
    }
    let raw = read_entry(path)?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| entry_invalid(path, &err.to_string()))?;
    if !value.is_object() && !value.is_array() {
        return Err(entry_invalid(
            path,
            "hooks.json must be a JSON object or array",
        ));
    }
    Ok(())
}

fn load_skills_dir(path: &Path) -> Result<(), PluginLoadError> {
    if !path.is_dir() {
        return Err(entry_invalid(path, "skills must be a directory"));
    }
    let entries = fs::read_dir(path).map_err(|err| PluginLoadError::EntryRead {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    let any = entries
        .flatten()
        .any(|entry| !entry.file_name().to_string_lossy().starts_with('.'));
    if !any {
        return Err(entry_invalid(
            path,
            "skills directory must contain at least one entry",
        ));
    }
    Ok(())
}

fn write_load_receipt(path: &Path, receipt: &PluginLoadReceipt) -> Result<(), PluginLoadError> {
    let body =
        serde_json::to_string_pretty(receipt).map_err(|err| PluginLoadError::ReceiptWrite {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
    fs::write(path, body).map_err(|err| PluginLoadError::ReceiptWrite {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

fn read_entry(path: &Path) -> Result<String, PluginLoadError> {
    fs::read_to_string(path).map_err(|err| PluginLoadError::EntryRead {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

fn entry_invalid(path: &Path, message: &str) -> PluginLoadError {
    PluginLoadError::EntryInvalid {
        path: path.display().to_string(),
        message: message.to_string(),
    }
}
