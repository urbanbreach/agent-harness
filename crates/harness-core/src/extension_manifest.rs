//! Extension manifests describe capabilities. Loading one never activates code.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};
mod descriptors;
pub use descriptors::*;
pub const EXTENSION_MANIFEST_V1_SCHEMA_VERSION: &str = "extension.manifest.v1";
pub const EXTENSION_MANIFEST_FILE_NAME: &str = "extension.manifest.json";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExtensionManifestError {
    #[error("failed to parse extension manifest: {0}")]
    Parse(String),
    #[error("unsupported extension schema `{found}`; expected `{expected}`")]
    UnsupportedSchemaVersion {
        found: String,
        expected: &'static str,
    },
    #[error("{field} `{value}` is not a stable extension id")]
    InvalidStableId { field: &'static str, value: String },
    #[error("duplicate capability id `{0}`")]
    DuplicateCapabilityId(String),
    #[error("{descriptor_kind} `{descriptor_id}` references unknown capability `{capability_id}`")]
    UnknownCapabilityRef {
        descriptor_kind: &'static str,
        descriptor_id: String,
        capability_id: String,
    },
    #[error("hook `{descriptor_id}` references unknown lifecycle `{lifecycle_event}`")]
    UnknownHookLifecycle {
        descriptor_id: String,
        lifecycle_event: String,
    },
    #[error(
        "{field} must contain bounded static replay text without interpolation or credentials"
    )]
    DynamicReplayText { field: &'static str },
    #[error("cannot read extension manifest at {path}: {message}")]
    ManifestRead { path: String, message: String },
    #[error("extension manifest is not a file: {path}")]
    ManifestNotAFile { path: String },
    #[error("duplicate descriptor id `{0}`")]
    DuplicateDescriptorId(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionManifestV1 {
    pub schema_version: String,
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<ExtensionCapabilityDescriptor>,
    #[serde(default)]
    pub tools: Vec<ExtensionToolDescriptor>,
    #[serde(default)]
    pub hooks: Vec<ExtensionHookDescriptor>,
    #[serde(default)]
    pub commands: Vec<ExtensionCommandDescriptor>,
    #[serde(default)]
    pub prompts: Vec<ExtensionPromptDescriptor>,
    #[serde(default)]
    pub mcp_bundles: Vec<ExtensionMcpBundleDescriptor>,
    #[serde(default)]
    pub diagnostics: Vec<ExtensionDiagnosticDescriptor>,
    #[serde(default)]
    pub provider_decorators: Vec<ExtensionProviderDecoratorDescriptor>,
    #[serde(default)]
    pub replay: Option<ExtensionReplayDescriptor>,
}
impl ExtensionManifestV1 {
    pub fn parse_json(input: &str) -> Result<Self, ExtensionManifestError> {
        if input.len() > 1024 * 1024 {
            return Err(ExtensionManifestError::Parse(
                "manifest exceeds 1 MiB".into(),
            ));
        }
        let manifest: Self = serde_json::from_str(input)
            .map_err(|_| ExtensionManifestError::Parse("invalid descriptor JSON".into()))?;
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn validate(&self) -> Result<(), ExtensionManifestError> {
        if self.schema_version != EXTENSION_MANIFEST_V1_SCHEMA_VERSION {
            return Err(ExtensionManifestError::UnsupportedSchemaVersion {
                found: "unsupported".into(),
                expected: EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            });
        }
        stable_id("id", &self.id)?;
        static_text("displayName", self.display_name.as_deref())?;
        static_text("version", self.version.as_deref())?;
        let count = self.capabilities.len()
            + self.tools.len()
            + self.hooks.len()
            + self.commands.len()
            + self.prompts.len()
            + self.mcp_bundles.len()
            + self.diagnostics.len()
            + self.provider_decorators.len();
        if count > 4096 {
            return Err(ExtensionManifestError::Parse(
                "manifest exceeds 4096 descriptors".into(),
            ));
        }
        let mut known = BTreeSet::new();
        for cap in &self.capabilities {
            stable_id("capability.id", &cap.id)?;
            if !known.insert(cap.id.as_str()) {
                return Err(ExtensionManifestError::DuplicateCapabilityId(
                    cap.id.clone(),
                ));
            }
            static_text("capability.replayLabel", cap.replay_label.as_deref())?;
            static_text("capability.description", cap.description.as_deref())?;
        }
        macro_rules! descriptors {
            ($($field:ident => $kind:literal),+) => { $(
                let mut ids = BTreeSet::new();
                for descriptor in &self.$field {
                    stable_id(concat!($kind, ".id"), &descriptor.id)?;
                    stable_id("capabilityId", &descriptor.capability_id)?;
                    if !ids.insert(&descriptor.id) { return Err(ExtensionManifestError::DuplicateDescriptorId(descriptor.id.clone())); }
                    if !known.contains(descriptor.capability_id.as_str()) { return Err(ExtensionManifestError::UnknownCapabilityRef { descriptor_kind: $kind, descriptor_id: descriptor.id.clone(), capability_id: descriptor.capability_id.clone() }); }
                    static_text(concat!($kind, ".replayLabel"), descriptor.replay_label.as_deref())?;
                }
            )+ };
        }
        descriptors!(tools => "tool", hooks => "hook", commands => "command", prompts => "prompt", mcp_bundles => "mcp", diagnostics => "diagnostic", provider_decorators => "provider");
        for tool in &self.tools {
            static_text("tool.description", tool.description.as_deref())?;
        }
        for hook in &self.hooks {
            if serde_json::from_value::<crate::config::HookLifecycleEvent>(
                hook.lifecycle_event.clone().into(),
            )
            .is_err()
            {
                return Err(ExtensionManifestError::UnknownHookLifecycle {
                    descriptor_id: hook.id.clone(),
                    lifecycle_event: "unknown".into(),
                });
            }
        }
        for bundle in &self.mcp_bundles {
            if bundle.server_ids.len() > 256 {
                return Err(ExtensionManifestError::Parse(
                    "MCP bundle exceeds 256 servers".into(),
                ));
            }
            for id in &bundle.server_ids {
                stable_id("serverId", id)?;
            }
        }
        for provider in &self.provider_decorators {
            if let Some(scope) = &provider.provider_scope {
                stable_id("providerScope", scope)?;
            }
        }
        if let Some(replay) = &self.replay {
            static_text("replay.label", Some(&replay.label))?;
            static_text("replay.summaryTemplate", Some(&replay.summary_template))?;
        }
        Ok(())
    }
    pub fn runtime_effects(&self) -> ExtensionManifestRuntimeEffects {
        ExtensionManifestRuntimeEffects::descriptor_only()
    }
    pub fn replay_metadata(&self) -> ExtensionReplayMetadata {
        ExtensionReplayMetadata {
            schema_version: self.schema_version.clone(),
            extension_id: self.id.clone(),
            display_name: self.display_name.clone(),
            capability_ids: self.capabilities.iter().map(|c| c.id.clone()).collect(),
            disabled_capability_ids: self
                .capabilities
                .iter()
                .filter(|c| !c.default_enabled)
                .map(|c| c.id.clone())
                .collect(),
            tool_descriptor_count: self.tools.len(),
            hook_descriptor_count: self.hooks.len(),
            command_descriptor_count: self.commands.len(),
            prompt_descriptor_count: self.prompts.len(),
            mcp_bundle_descriptor_count: self.mcp_bundles.len(),
            diagnostic_descriptor_count: self.diagnostics.len(),
            provider_decorator_descriptor_count: self.provider_decorators.len(),
            replay_label: self.replay.as_ref().map(|r| r.label.clone()),
        }
    }
    pub fn summary(&self) -> ExtensionManifestSummary {
        ExtensionManifestSummary {
            extension_id: self.id.clone(),
            display_name: self.display_name.clone(),
            version: self.version.clone(),
            capabilities: self.capabilities.len(),
            enabled_capabilities: self
                .capabilities
                .iter()
                .filter(|c| c.default_enabled)
                .count(),
            tools: self.tools.len(),
            hooks: self.hooks.len(),
            commands: self.commands.len(),
            prompts: self.prompts.len(),
            mcp_bundles: self.mcp_bundles.len(),
            diagnostics: self.diagnostics.len(),
            provider_decorators: self.provider_decorators.len(),
            loads_external_code: false,
        }
    }
}
pub(crate) fn stable_id(field: &'static str, value: &str) -> Result<(), ExtensionManifestError> {
    let mut separator = true;
    let valid = value.len() <= 256
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value.bytes().all(|b| {
            if b.is_ascii_lowercase() || b.is_ascii_digit() {
                separator = false;
                true
            } else if b"._:-".contains(&b) && !separator {
                separator = true;
                true
            } else {
                false
            }
        })
        && !separator
        && crate::redact::redact_artifact_text(value) == value;
    if valid {
        Ok(())
    } else {
        Err(ExtensionManifestError::InvalidStableId {
            field,
            value: "<invalid>".into(),
        })
    }
}
fn static_text(field: &'static str, value: Option<&str>) -> Result<(), ExtensionManifestError> {
    if value.is_some_and(|v| {
        v.len() > 4096
            || v.contains(['$', '`'])
            || v.contains("{{")
            || v.contains("}}")
            || v.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
            || crate::redact::redact_artifact_text(v) != v
    }) {
        Err(ExtensionManifestError::DynamicReplayText { field })
    } else {
        Ok(())
    }
}
pub fn load_extension_manifest_from_path(
    path: &Path,
) -> Result<ExtensionManifestV1, ExtensionManifestError> {
    let bytes = crate::store::read_private_bytes(path, 1024 * 1024)
        .map_err(|e| ExtensionManifestError::ManifestRead {
            path: path.display().to_string(),
            message: e.to_string(),
        })?
        .ok_or_else(|| ExtensionManifestError::ManifestNotAFile {
            path: path.display().to_string(),
        })?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ExtensionManifestError::Parse("manifest is not UTF-8".into()))?;
    ExtensionManifestV1::parse_json(text)
}
pub fn discover_extension_manifests(root: &Path) -> Vec<ExtensionManifestSummary> {
    let mut summaries: Vec<_> = manifest_paths(root)
        .filter_map(|p| load_extension_manifest_from_path(&p).ok())
        .map(|m| m.summary())
        .collect();
    summaries.sort_by(|a, b| a.extension_id.cmp(&b.extension_id));
    summaries
}
pub(crate) fn manifest_paths(root: &Path) -> impl Iterator<Item = std::path::PathBuf> {
    std::iter::once(root.join(EXTENSION_MANIFEST_FILE_NAME)).chain(
        std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .map(|entry| entry.path().join(EXTENSION_MANIFEST_FILE_NAME)),
    )
}
pub fn first_extension_manifest_summary(root: &Path) -> Option<ExtensionManifestSummary> {
    discover_extension_manifests(root).into_iter().next()
}
pub fn summarize_extension_discover(
    summaries: &[ExtensionManifestSummary],
) -> ExtensionDiscoverSummary {
    ExtensionDiscoverSummary {
        discovered: summaries.len(),
        loads_external_code: false,
    }
}
pub fn load_extension_manifest_outcome(path: impl AsRef<Path>) -> ExtensionLoadOutcome {
    let path = path.as_ref();
    match load_extension_manifest_from_path(path) {
        Ok(m) => ExtensionLoadOutcome::Loaded {
            path: path.display().to_string(),
            extension_id: m.id,
        },
        Err(e) => ExtensionLoadOutcome::Failed {
            path: path.display().to_string(),
            reason: e.to_string(),
        },
    }
}
