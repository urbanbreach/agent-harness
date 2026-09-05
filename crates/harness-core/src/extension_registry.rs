//! Durable multi-descriptor extension registry under a workspace.
//!
//! Discovers `extension.manifest.json` descriptors and persists validated entries
//! at `.agent-harness/extension-registry.json`. Descriptor-only: no code load.

mod store;

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::extension_manifest::{
    discover_extension_manifests, load_extension_manifest_from_path, ExtensionDiscoverSummary,
    ExtensionManifestSummary, EXTENSION_MANIFEST_FILE_NAME,
};

use store::{
    load_or_empty, now_unix_ms, resolve_manifest_path, save, workspace_relative,
    ExtensionRegistryDocument,
};

/// Relative durable registry path under a workspace root.
pub const EXTENSION_REGISTRY_REL: &str = ".agent-harness/extension-registry.json";

pub(crate) const STORE_VERSION: u32 = 1;

/// Failures for durable extension registry I/O and path safety.
#[derive(Debug, Error)]
pub enum ExtensionRegistryError {
    #[error("failed to create extension-registry parent directory {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to read extension-registry {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse extension-registry {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("unsupported extension-registry version {version} at {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error("failed to write extension-registry {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace extension-registry {path}: {source}")]
    Replace {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("invalid extension registry path `{path}` (empty or escapes workspace)")]
    InvalidPath { path: String },
    #[error("extension manifest load failed for `{path}`: {detail}")]
    ManifestLoad { path: String, detail: String },
}

/// One durable registry entry (descriptor metadata + workspace-relative path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRegistryEntry {
    pub extension_id: String,
    pub manifest_path: String,
    pub capabilities: usize,
    pub enabled_capabilities: usize,
    pub tools: usize,
    pub hooks: usize,
    pub loads_external_code: bool,
    pub registered_at_unix_ms: u64,
}

impl ExtensionRegistryEntry {
    pub fn one_line(&self) -> String {
        format!(
            "extension registry: id=`{}` path=`{}` caps={}/{} tools={} hooks={} loads_code={}",
            self.extension_id,
            self.manifest_path,
            self.enabled_capabilities,
            self.capabilities,
            self.tools,
            self.hooks,
            self.loads_external_code
        )
    }

    pub fn to_summary(&self) -> ExtensionManifestSummary {
        ExtensionManifestSummary {
            extension_id: self.extension_id.clone(),
            display_name: None,
            version: None,
            capabilities: self.capabilities,
            enabled_capabilities: self.enabled_capabilities,
            tools: self.tools,
            hooks: self.hooks,
            commands: 0,
            prompts: 0,
            mcp_bundles: 0,
            diagnostics: 0,
            provider_decorators: 0,
            loads_external_code: self.loads_external_code,
        }
    }
}

/// Operator-facing durable registry counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExtensionRegistrySummary {
    pub registered: usize,
    pub loads_external_code: bool,
}

impl ExtensionRegistrySummary {
    pub fn one_line(&self) -> String {
        format!(
            "extension registry: {} descriptor(s) (loads_code={})",
            self.registered, self.loads_external_code
        )
    }
}

/// Durable multi-descriptor registry for one workspace.
#[derive(Debug, Clone)]
pub struct ExtensionDescriptorRegistry {
    workspace_root: PathBuf,
    registry_path: PathBuf,
    doc: ExtensionRegistryDocument,
}

impl ExtensionDescriptorRegistry {
    pub fn open(workspace_root: impl Into<PathBuf>) -> Result<Self, ExtensionRegistryError> {
        let workspace_root = workspace_root.into();
        let registry_path = workspace_root.join(EXTENSION_REGISTRY_REL);
        let doc = load_or_empty(&registry_path)?;
        Ok(Self {
            workspace_root,
            registry_path,
            doc,
        })
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn registry_path(&self) -> &Path {
        &self.registry_path
    }

    pub fn summary(&self) -> ExtensionRegistrySummary {
        ExtensionRegistrySummary {
            registered: self.doc.entries.len(),
            loads_external_code: self
                .doc
                .entries
                .values()
                .any(|entry| entry.loads_external_code),
        }
    }

    pub fn list(&self) -> Vec<&ExtensionRegistryEntry> {
        self.doc.entries.values().collect()
    }

    pub fn get(&self, extension_id: &str) -> Option<&ExtensionRegistryEntry> {
        self.doc.entries.get(extension_id)
    }

    /// Discover descriptors under `scan_root` and register each (upsert by id).
    pub fn discover_and_register(
        &mut self,
        scan_root: &Path,
    ) -> Result<ExtensionDiscoverSummary, ExtensionRegistryError> {
        let discovered = discover_extension_manifests(scan_root);
        let mut pending = self.doc.clone();
        let now = now_unix_ms();
        for summary in &discovered {
            let manifest_path = resolve_manifest_path(scan_root, summary)?;
            let relative = workspace_relative(&self.workspace_root, &manifest_path)?;
            load_extension_manifest_from_path(&manifest_path).map_err(|err| {
                ExtensionRegistryError::ManifestLoad {
                    path: manifest_path.display().to_string(),
                    detail: err.to_string(),
                }
            })?;
            pending.entries.insert(
                summary.extension_id.clone(),
                ExtensionRegistryEntry {
                    extension_id: summary.extension_id.clone(),
                    manifest_path: relative,
                    capabilities: summary.capabilities,
                    enabled_capabilities: summary.enabled_capabilities,
                    tools: summary.tools,
                    hooks: summary.hooks,
                    loads_external_code: summary.loads_external_code,
                    registered_at_unix_ms: now,
                },
            );
        }
        save(&self.registry_path, &pending)?;
        self.doc = pending;
        Ok(ExtensionDiscoverSummary {
            discovered: discovered.len(),
            loads_external_code: discovered.iter().any(|s| s.loads_external_code),
        })
    }

    /// Register one validated manifest path (workspace-relative after resolve).
    pub fn register_manifest_path(
        &mut self,
        manifest_path: impl AsRef<Path>,
    ) -> Result<ExtensionRegistryEntry, ExtensionRegistryError> {
        let absolute = if manifest_path.as_ref().is_absolute() {
            manifest_path.as_ref().to_path_buf()
        } else {
            self.workspace_root.join(manifest_path.as_ref())
        };
        let relative = workspace_relative(&self.workspace_root, &absolute)?;
        let manifest = load_extension_manifest_from_path(&absolute).map_err(|err| {
            ExtensionRegistryError::ManifestLoad {
                path: absolute.display().to_string(),
                detail: err.to_string(),
            }
        })?;
        let summary = manifest.summary();
        let entry = ExtensionRegistryEntry {
            extension_id: summary.extension_id.clone(),
            manifest_path: relative,
            capabilities: summary.capabilities,
            enabled_capabilities: summary.enabled_capabilities,
            tools: summary.tools,
            hooks: summary.hooks,
            loads_external_code: summary.loads_external_code,
            registered_at_unix_ms: now_unix_ms(),
        };
        self.doc
            .entries
            .insert(entry.extension_id.clone(), entry.clone());
        save(&self.registry_path, &self.doc)?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
    use std::fs;

    fn write_manifest(dir: &Path, id: &str, tools: bool) {
        fs::create_dir_all(dir).expect("mkdir");
        let tools_json = if tools {
            r#","tools":[{"id":"probe.tool","capabilityId":"probe.cap","permission":"bash"}]"#
        } else {
            ""
        };
        let body = format!(
            r#"{{"schemaVersion":"{schema}","id":"{id}","displayName":"Probe","version":"0.0.1","capabilities":[{{"id":"probe.cap","defaultEnabled":true}}]{tools}}}"#,
            schema = EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            id = id,
            tools = tools_json,
        );
        fs::write(dir.join(EXTENSION_MANIFEST_FILE_NAME), body).expect("write");
    }

    #[test]
    fn discover_register_persists_and_reloads() {
        // Given
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path();
        write_manifest(&root.join("ext-a"), "harness.probe.extension", true);
        write_manifest(&root.join("ext-b"), "harness.probe.extension.alt", false);
        write_manifest(&root.join("ext-c"), "harness.probe.extension.tools", true);

        // When
        let mut registry = ExtensionDescriptorRegistry::open(root).expect("open");
        let discover = registry.discover_and_register(root).expect("discover");

        // Then
        assert!(discover.discovered >= 3);
        assert!(registry.registry_path().is_file());
        assert!(registry.summary().registered >= 3);
        assert!(registry.get("harness.probe.extension").is_some());

        let reloaded = ExtensionDescriptorRegistry::open(root).expect("reload");
        assert!(reloaded.summary().registered >= 3);
        assert_eq!(
            reloaded.get("harness.probe.extension").map(|e| e.tools),
            Some(1)
        );
        assert!(!reloaded.summary().loads_external_code);
    }

    #[test]
    fn register_manifest_path_fail_closed_on_missing() {
        // Given
        let temp = tempfile::tempdir().expect("temp");
        let mut registry = ExtensionDescriptorRegistry::open(temp.path()).expect("open");

        // When / Then
        let err = registry
            .register_manifest_path("missing/extension.manifest.json")
            .expect_err("missing");
        assert!(matches!(err, ExtensionRegistryError::ManifestLoad { .. }));
        assert_eq!(registry.summary().registered, 0);
    }

    #[test]
    fn register_manifest_path_fail_closed_on_invalid_json() {
        // arrange — a manifest file that is not a valid descriptor
        let temp = tempfile::tempdir().expect("temp");
        let ext_dir = temp.path().join("ext-broken");
        fs::create_dir_all(&ext_dir).expect("mkdir");
        fs::write(
            ext_dir.join(EXTENSION_MANIFEST_FILE_NAME),
            "{ not a valid descriptor",
        )
        .expect("write");
        let mut registry = ExtensionDescriptorRegistry::open(temp.path()).expect("open");

        // act
        let err = registry
            .register_manifest_path(ext_dir.join(EXTENSION_MANIFEST_FILE_NAME))
            .expect_err("invalid manifest");

        // assert — fails closed with no registry side effect
        assert!(matches!(err, ExtensionRegistryError::ManifestLoad { .. }));
        assert_eq!(registry.summary().registered, 0);
    }

    #[test]
    fn path_escape_attempts_are_rejected() {
        // Given
        let temp = tempfile::tempdir().expect("temp");
        let outside = tempfile::tempdir().expect("outside");
        write_manifest(outside.path(), "harness.escape.probe", false);
        let mut registry = ExtensionDescriptorRegistry::open(temp.path()).expect("open");

        // When / Then
        let err = registry
            .register_manifest_path(outside.path().join(EXTENSION_MANIFEST_FILE_NAME))
            .expect_err("escape");
        assert!(matches!(err, ExtensionRegistryError::InvalidPath { .. }));
    }
}
