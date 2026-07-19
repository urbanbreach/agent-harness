// allow: SIZE_OK — extension manifest V1 parser (schema version + descriptor fields + validation)
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::HookLifecycleEvent;

pub const EXTENSION_MANIFEST_V1_SCHEMA_VERSION: &str = "extension.manifest.v1";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExtensionManifestError {
    #[error("failed to parse extension manifest: {0}")]
    Parse(String),
    #[error("unsupported extension manifest schema_version `{found}`; expected `{expected}`")]
    UnsupportedSchemaVersion {
        found: String,
        expected: &'static str,
    },
    #[error("{field} `{value}` is not a stable extension id")]
    InvalidStableId { field: &'static str, value: String },
    #[error("duplicate capability id `{0}`")]
    DuplicateCapabilityId(String),
    #[error(
        "{descriptor_kind} `{descriptor_id}` references unknown capability id `{capability_id}`"
    )]
    UnknownCapabilityRef {
        descriptor_kind: &'static str,
        descriptor_id: String,
        capability_id: String,
    },
    #[error(
        "hook descriptor `{descriptor_id}` references unknown lifecycle event `{lifecycle_event}`"
    )]
    UnknownHookLifecycle {
        descriptor_id: String,
        lifecycle_event: String,
    },
    #[error("{field} must be static replay text and cannot contain interpolation or shell metacharacter tokens")]
    DynamicReplayText { field: &'static str },
    #[error("failed to read extension manifest at {path}: {message}")]
    ManifestRead { path: String, message: String },
    #[error("extension manifest path is not a file: {path}")]
    ManifestNotAFile { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
        let manifest = serde_json::from_str::<Self>(input)
            .map_err(|err| ExtensionManifestError::Parse(err.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ExtensionManifestError> {
        if self.schema_version != EXTENSION_MANIFEST_V1_SCHEMA_VERSION {
            return Err(ExtensionManifestError::UnsupportedSchemaVersion {
                found: self.schema_version.clone(),
                expected: EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            });
        }
        validate_stable_id("id", &self.id)?;

        let mut capability_ids = BTreeSet::new();
        for capability in &self.capabilities {
            validate_stable_id("capabilities.id", &capability.id)?;
            validate_static_text(
                "capabilities.replayLabel",
                capability.replay_label.as_deref(),
            )?;
            if !capability_ids.insert(capability.id.clone()) {
                return Err(ExtensionManifestError::DuplicateCapabilityId(
                    capability.id.clone(),
                ));
            }
        }

        for descriptor in &self.tools {
            validate_descriptor_id("tools.id", &descriptor.id)?;
            validate_capability_ref(
                "tool",
                &descriptor.id,
                &descriptor.capability_id,
                &capability_ids,
            )?;
            validate_static_text("tools.replayLabel", descriptor.replay_label.as_deref())?;
        }
        for descriptor in &self.hooks {
            validate_descriptor_id("hooks.id", &descriptor.id)?;
            validate_capability_ref(
                "hook",
                &descriptor.id,
                &descriptor.capability_id,
                &capability_ids,
            )?;
            if !known_lifecycle_event(&descriptor.lifecycle_event) {
                return Err(ExtensionManifestError::UnknownHookLifecycle {
                    descriptor_id: descriptor.id.clone(),
                    lifecycle_event: descriptor.lifecycle_event.clone(),
                });
            }
            validate_static_text("hooks.replayLabel", descriptor.replay_label.as_deref())?;
        }
        for descriptor in &self.commands {
            validate_descriptor_id("commands.id", &descriptor.id)?;
            validate_capability_ref(
                "command",
                &descriptor.id,
                &descriptor.capability_id,
                &capability_ids,
            )?;
            validate_static_text("commands.replayLabel", descriptor.replay_label.as_deref())?;
        }
        for descriptor in &self.prompts {
            validate_descriptor_id("prompts.id", &descriptor.id)?;
            validate_capability_ref(
                "prompt",
                &descriptor.id,
                &descriptor.capability_id,
                &capability_ids,
            )?;
            validate_static_text("prompts.replayLabel", descriptor.replay_label.as_deref())?;
        }
        for descriptor in &self.mcp_bundles {
            validate_descriptor_id("mcpBundles.id", &descriptor.id)?;
            validate_capability_ref(
                "mcp_bundle",
                &descriptor.id,
                &descriptor.capability_id,
                &capability_ids,
            )?;
            validate_static_text("mcpBundles.replayLabel", descriptor.replay_label.as_deref())?;
        }
        for descriptor in &self.diagnostics {
            validate_descriptor_id("diagnostics.id", &descriptor.id)?;
            validate_capability_ref(
                "diagnostic",
                &descriptor.id,
                &descriptor.capability_id,
                &capability_ids,
            )?;
            validate_static_text(
                "diagnostics.replayLabel",
                descriptor.replay_label.as_deref(),
            )?;
        }
        for descriptor in &self.provider_decorators {
            validate_descriptor_id("providerDecorators.id", &descriptor.id)?;
            validate_capability_ref(
                "provider_decorator",
                &descriptor.id,
                &descriptor.capability_id,
                &capability_ids,
            )?;
            validate_static_text(
                "providerDecorators.replayLabel",
                descriptor.replay_label.as_deref(),
            )?;
        }
        if let Some(replay) = &self.replay {
            validate_static_text("replay.label", Some(&replay.label))?;
            validate_static_text("replay.summaryTemplate", Some(&replay.summary_template))?;
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
            capability_ids: self
                .capabilities
                .iter()
                .map(|capability| capability.id.clone())
                .collect(),
            disabled_capability_ids: self
                .capabilities
                .iter()
                .filter(|capability| !capability.default_enabled)
                .map(|capability| capability.id.clone())
                .collect(),
            tool_descriptor_count: self.tools.len(),
            hook_descriptor_count: self.hooks.len(),
            command_descriptor_count: self.commands.len(),
            prompt_descriptor_count: self.prompts.len(),
            mcp_bundle_descriptor_count: self.mcp_bundles.len(),
            diagnostic_descriptor_count: self.diagnostics.len(),
            provider_decorator_descriptor_count: self.provider_decorators.len(),
            replay_label: self.replay.as_ref().map(|replay| replay.label.clone()),
        }
    }

    /// Operator-facing descriptor counts (diagnostics only; not a plugin runtime).
    pub fn summary(&self) -> ExtensionManifestSummary {
        let enabled_capabilities = self
            .capabilities
            .iter()
            .filter(|capability| capability.default_enabled)
            .count();
        ExtensionManifestSummary {
            extension_id: self.id.clone(),
            display_name: self.display_name.clone(),
            version: self.version.clone(),
            capabilities: self.capabilities.len(),
            enabled_capabilities,
            tools: self.tools.len(),
            hooks: self.hooks.len(),
            commands: self.commands.len(),
            prompts: self.prompts.len(),
            mcp_bundles: self.mcp_bundles.len(),
            diagnostics: self.diagnostics.len(),
            provider_decorators: self.provider_decorators.len(),
            loads_external_code: self.runtime_effects().loads_external_code,
        }
    }
}

/// Operator-facing counts for a parsed extension descriptor (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifestSummary {
    pub extension_id: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub capabilities: usize,
    pub enabled_capabilities: usize,
    pub tools: usize,
    pub hooks: usize,
    pub commands: usize,
    pub prompts: usize,
    pub mcp_bundles: usize,
    pub diagnostics: usize,
    pub provider_decorators: usize,
    /// Always false for V1 descriptor-only manifests.
    pub loads_external_code: bool,
}

impl ExtensionManifestSummary {
    pub fn one_line(&self) -> String {
        format!(
            "extension descriptor: id=`{}` caps={}/{} tools={} hooks={} commands={} prompts={} mcp={} diag={} decorators={} loads_code={}",
            self.extension_id,
            self.enabled_capabilities,
            self.capabilities,
            self.tools,
            self.hooks,
            self.commands,
            self.prompts,
            self.mcp_bundles,
            self.diagnostics,
            self.provider_decorators,
            self.loads_external_code
        )
    }

    pub const fn is_descriptor_only(&self) -> bool {
        !self.loads_external_code
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionCapabilityDescriptor {
    pub id: String,
    #[serde(default = "default_true")]
    pub default_enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionToolDescriptor {
    pub id: String,
    pub capability_id: String,
    pub permission: ExtensionToolPermission,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionToolPermission {
    Bash,
    Edit,
    Question,
    Task,
    Webfetch,
    Websearch,
    Codesearch,
    Lsp,
}

impl ExtensionToolPermission {
    pub fn public_name(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Edit => "edit",
            Self::Question => "question",
            Self::Task => "task",
            Self::Webfetch => "webfetch",
            Self::Websearch => "websearch",
            Self::Codesearch => "codesearch",
            Self::Lsp => "lsp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionHookDescriptor {
    pub id: String,
    pub capability_id: String,
    pub lifecycle_event: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionCommandDescriptor {
    pub id: String,
    pub capability_id: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionPromptDescriptor {
    pub id: String,
    pub capability_id: String,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionMcpBundleDescriptor {
    pub id: String,
    pub capability_id: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub server_ids: Vec<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionDiagnosticDescriptor {
    pub id: String,
    pub capability_id: String,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionProviderDecoratorDescriptor {
    pub id: String,
    pub capability_id: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub provider_scope: Option<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionSeamStatus {
    Native,
    Fallback,
    IntentionallyUnsupported,
    PostV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionReplayDescriptor {
    pub label: String,
    pub summary_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReplayMetadata {
    pub schema_version: String,
    pub extension_id: String,
    pub display_name: Option<String>,
    pub capability_ids: Vec<String>,
    pub disabled_capability_ids: Vec<String>,
    pub tool_descriptor_count: usize,
    pub hook_descriptor_count: usize,
    pub command_descriptor_count: usize,
    pub prompt_descriptor_count: usize,
    pub mcp_bundle_descriptor_count: usize,
    pub diagnostic_descriptor_count: usize,
    pub provider_decorator_descriptor_count: usize,
    pub replay_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifestRuntimeEffects {
    pub registers_tools: bool,
    pub executes_commands: bool,
    pub launches_mcp: bool,
    pub invokes_provider_decorators: bool,
    pub loads_external_code: bool,
    pub mutates_sessions: bool,
}

impl ExtensionManifestRuntimeEffects {
    pub fn descriptor_only() -> Self {
        Self {
            registers_tools: false,
            executes_commands: false,
            launches_mcp: false,
            invokes_provider_decorators: false,
            loads_external_code: false,
            mutates_sessions: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn validate_capability_ref(
    descriptor_kind: &'static str,
    descriptor_id: &str,
    capability_id: &str,
    known_capability_ids: &BTreeSet<String>,
) -> Result<(), ExtensionManifestError> {
    validate_stable_id("capabilityId", capability_id)?;
    if !known_capability_ids.contains(capability_id) {
        return Err(ExtensionManifestError::UnknownCapabilityRef {
            descriptor_kind,
            descriptor_id: descriptor_id.to_string(),
            capability_id: capability_id.to_string(),
        });
    }
    Ok(())
}

fn validate_descriptor_id(field: &'static str, value: &str) -> Result<(), ExtensionManifestError> {
    validate_stable_id(field, value)
}

fn validate_stable_id(field: &'static str, value: &str) -> Result<(), ExtensionManifestError> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(ExtensionManifestError::InvalidStableId {
            field,
            value: value.to_string(),
        });
    };
    if !first.is_ascii_lowercase() {
        return Err(ExtensionManifestError::InvalidStableId {
            field,
            value: value.to_string(),
        });
    }
    let mut previous_was_separator = false;
    for ch in chars {
        let is_separator = matches!(ch, '.' | '_' | ':' | '-');
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            previous_was_separator = false;
            continue;
        }
        if is_separator && !previous_was_separator {
            previous_was_separator = true;
            continue;
        }
        return Err(ExtensionManifestError::InvalidStableId {
            field,
            value: value.to_string(),
        });
    }
    if previous_was_separator {
        return Err(ExtensionManifestError::InvalidStableId {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_static_text(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), ExtensionManifestError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.contains("{{") || value.contains("}}") || value.contains('$') || value.contains('`') {
        return Err(ExtensionManifestError::DynamicReplayText { field });
    }
    Ok(())
}

fn known_lifecycle_event(value: &str) -> bool {
    const EVENTS: &[HookLifecycleEvent] = &[
        HookLifecycleEvent::RunStarted,
        HookLifecycleEvent::RunFinished,
        HookLifecycleEvent::RunFailed,
        HookLifecycleEvent::AgentTurnStarted,
        HookLifecycleEvent::AgentTurnFinished,
        HookLifecycleEvent::ToolCallStarted,
        HookLifecycleEvent::ToolCallFinished,
        HookLifecycleEvent::ProviderRequestStarted,
        HookLifecycleEvent::ProviderRequestFinished,
        HookLifecycleEvent::CompactionRequested,
        HookLifecycleEvent::CompactionWritten,
        HookLifecycleEvent::CompactionApplied,
        HookLifecycleEvent::CompactionFailed,
        HookLifecycleEvent::SubagentSpawned,
        HookLifecycleEvent::SubagentFinished,
        HookLifecycleEvent::PermissionRequested,
        HookLifecycleEvent::PermissionResolved,
    ];
    EVENTS.iter().any(|event| event.as_str() == value)
}

/// Canonical on-disk filename for V1 extension descriptors (shared with plugin lifecycle).
pub const EXTENSION_MANIFEST_FILE_NAME: &str = "extension.manifest.json";

/// Load + validate a single extension.manifest.json path (descriptor-only product surface).
pub fn load_extension_manifest_from_path(
    path: &Path,
) -> Result<ExtensionManifestV1, ExtensionManifestError> {
    if !path.is_file() {
        return Err(ExtensionManifestError::ManifestNotAFile {
            path: path.display().to_string(),
        });
    }
    let raw = fs::read_to_string(path).map_err(|err| ExtensionManifestError::ManifestRead {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;
    ExtensionManifestV1::parse_json(&raw)
}

/// Discover extension descriptors under `scan_root`.
///
/// Looks for `extension.manifest.json` as:
/// - an immediate child file of `scan_root`
/// - an immediate child directory containing the file
///
/// Read-only: never writes. Returns summaries only (no code load / activation).
pub fn discover_extension_manifests(scan_root: &Path) -> Vec<ExtensionManifestSummary> {
    let mut out = Vec::new();
    if !scan_root.is_dir() {
        return out;
    }

    let direct = scan_root.join(EXTENSION_MANIFEST_FILE_NAME);
    if direct.is_file() {
        if let Ok(manifest) = load_extension_manifest_from_path(&direct) {
            out.push(manifest.summary());
        }
    }

    let Ok(entries) = fs::read_dir(scan_root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let candidate = path.join(EXTENSION_MANIFEST_FILE_NAME);
        if !candidate.is_file() {
            continue;
        }
        if let Ok(manifest) = load_extension_manifest_from_path(&candidate) {
            out.push(manifest.summary());
        }
    }

    out.sort_by(|left, right| left.extension_id.cmp(&right.extension_id));
    out
}

/// Operator-facing counts for extension descriptor discovery (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExtensionDiscoverSummary {
    pub discovered: usize,
    pub loads_external_code: bool,
}

impl ExtensionDiscoverSummary {
    pub fn one_line(&self) -> String {
        format!(
            "extension discover: {} descriptor(s) (loads_code={})",
            self.discovered, self.loads_external_code
        )
    }
}

/// Summarize [`discover_extension_manifests`] results for operator surfaces.
pub fn summarize_extension_discover(
    summaries: &[ExtensionManifestSummary],
) -> ExtensionDiscoverSummary {
    let loads_external_code = summaries.iter().any(|summary| summary.loads_external_code);
    ExtensionDiscoverSummary {
        discovered: summaries.len(),
        loads_external_code,
    }
}

/// Result of a single extension.manifest.json load attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ExtensionLoadOutcome {
    Loaded { path: String, extension_id: String },
    Failed { path: String, reason: String },
}

impl ExtensionLoadOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Loaded { path, extension_id } => {
                format!("extension load: ok id=`{extension_id}` path=`{path}` (loads_code=false)")
            }
            Self::Failed { path, reason } => {
                format!("extension load: failed path=`{path}` ({reason})")
            }
        }
    }
}

/// Load a single extension descriptor and return a structured operator-facing outcome.
pub fn load_extension_manifest_outcome(path: impl AsRef<Path>) -> ExtensionLoadOutcome {
    let path_ref = path.as_ref();
    let path_display = path_ref.display().to_string();
    match load_extension_manifest_from_path(path_ref) {
        Ok(manifest) => ExtensionLoadOutcome::Loaded {
            path: path_display,
            extension_id: manifest.id,
        },
        Err(err) => ExtensionLoadOutcome::Failed {
            path: path_display,
            reason: err.to_string(),
        },
    }
}

/// Prefer the first discovered descriptor summary for operator surfaces.
pub fn first_extension_manifest_summary(scan_root: &Path) -> Option<ExtensionManifestSummary> {
    discover_extension_manifests(scan_root).into_iter().next()
}
