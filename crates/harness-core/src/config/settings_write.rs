use super::normalize::rename as alias;
use super::ordered::OrderedValue;
use super::*;
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
mod rewind;
pub use rewind::*;

#[derive(Debug, thiserror::Error)]
pub enum SettingWriteError {
    #[error("setting `{0}` is not registered")]
    UnknownSetting(String),
    #[error("setting `{0}` is secret and cannot be edited")]
    SecretSetting(String),
    #[error("setting `{0}` is not editable")]
    NotEditable(String),
    #[error("setting `{0}` has no project write path")]
    UnsupportedWrite(String),
    #[error("setting `{0}` belongs in tui.json")]
    WrongSurface(String),
    #[error("failed to read config {path}: {source}")]
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to write config {path}: {source}")]
    WriteFile {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid config: {0}")]
    Parse(String),
    #[error("config must be an object")]
    InvalidRoot,
    #[error("failed to reload config {path}: {reason}")]
    Reload { path: String, reason: String },
}
impl From<ConfigError> for SettingWriteError {
    fn from(error: ConfigError) -> Self {
        Self::Parse(error.to_string())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingEditorKind {
    Boolean,
    Integer,
    String,
    Choice(&'static [&'static str]),
}
pub fn setting_editor_kind(id: &str) -> Option<SettingEditorKind> {
    match id {
        "runtime.yolo"
        | "subagents.enabled"
        | "features.active_agent_messages"
        | "features.subagent_model_inheritance"
        | "features.subagent_worktree_snapshot" => Some(SettingEditorKind::Boolean),
        "runtime.compaction.reserve_tokens"
        | "runtime.compaction.keep_recent_tokens"
        | "runtime.compaction.fallback_input_tokens"
        | "subagents.max_depth"
        | "subagents.max_concurrent"
        | "subagents.sampling_limit" => Some(SettingEditorKind::Integer),
        "runtime.session_dir" => Some(SettingEditorKind::String),
        "permission.bash" => Some(SettingEditorKind::Choice(&["ask", "allow", "deny"])),
        "subagents.limit_behavior" => Some(SettingEditorKind::Choice(&["queue", "fail"])),
        _ => None,
    }
}
// The unchanged UI has separate toggles for these six settings.
const TOGGLES: &[&str] = &[
    "hashline_edit",
    "runtime.compaction.enabled",
    "runtime.compaction.auto_retry_overflow",
    "runtime.compaction.structured_summary_contract",
    "runtime.compaction.estimated_token_triggers",
    "runtime.deterministic.enabled",
];
pub(super) fn writable(id: &str) -> Result<&'static SettingDefinition, SettingWriteError> {
    let entry =
        setting_definition(id).ok_or_else(|| SettingWriteError::UnknownSetting(id.into()))?;
    if entry.is_secret() {
        return Err(SettingWriteError::SecretSetting(id.into()));
    }
    if entry.surface != SettingSurface::Runtime {
        return Err(SettingWriteError::WrongSurface(id.into()));
    }
    if !entry.is_editable() {
        return Err(SettingWriteError::NotEditable(id.into()));
    }
    if !TOGGLES.contains(&entry.setting_id.0) && setting_editor_kind(entry.setting_id.0).is_none() {
        return Err(SettingWriteError::UnsupportedWrite(id.into()));
    }
    Ok(entry)
}
pub fn read_project_setting_value(
    path: &Path,
    id: &str,
) -> Result<Option<String>, SettingWriteError> {
    let entry = writable(id)?;
    let raw = raw_file(path)?;
    if setting_editor_kind(entry.setting_id.0).is_some() {
        let value = raw.json()?;
        return Ok(value
            .pointer(&format!("/{}", entry.setting_id.0.replace('.', "/")))
            .and_then(|value| match value {
                Value::String(text) => Some(text.clone()),
                Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
                _ => None,
            }));
    }
    effective(&raw, path, entry.setting_id.0).map(Some)
}
pub fn write_project_setting_value(
    path: &Path,
    id: &str,
    input: &str,
) -> Result<String, SettingWriteError> {
    let entry = writable(id)?;
    let id = entry.setting_id.0;
    let kind = setting_editor_kind(id).unwrap_or(SettingEditorKind::Boolean);
    let value = match kind {
        SettingEditorKind::Boolean => Value::Bool(
            input
                .parse()
                .map_err(|_| SettingWriteError::Parse("expected true or false".into()))?,
        ),
        SettingEditorKind::Integer => Value::from(input.parse::<u32>().map_err(|_| {
            SettingWriteError::Parse("Enter a whole number from 0 to 4294967295".into())
        })?),
        SettingEditorKind::Choice(choices) if !choices.contains(&input) => {
            return Err(SettingWriteError::Parse(
                "unsupported setting choice".into(),
            ))
        }
        SettingEditorKind::Choice(_) | SettingEditorKind::String => Value::String(input.into()),
    };
    update(path, id, Some(value))
}
pub fn write_project_setting_bool(
    path: &Path,
    id: &str,
    value: bool,
) -> Result<bool, SettingWriteError> {
    let entry = writable(id)?;
    if !(TOGGLES.contains(&entry.setting_id.0)
        || setting_editor_kind(entry.setting_id.0) == Some(SettingEditorKind::Boolean))
    {
        return Err(SettingWriteError::UnsupportedWrite(id.into()));
    }
    write_project_setting_value(path, id, if value { "true" } else { "false" })
        .map(|value| value == "true")
}
pub fn reset_project_setting_to_default(
    path: &Path,
    id: &str,
) -> Result<String, SettingWriteError> {
    update(path, writable(id)?.setting_id.0, None)
}
fn update(path: &Path, id: &str, value: Option<Value>) -> Result<String, SettingWriteError> {
    let path = absolute(path)?;
    let _lock = lock(&path)?;
    let mut raw = raw_file(&path)?;
    for (old, new) in [
        ("permissions", "permission"),
        ("hashlineEdit", "hashline_edit"),
    ] {
        raw.rename(old, new)?;
    }
    let mut edited = raw.json()?;
    let keys = setting_path(&mut edited, id)?;
    replace(&mut edited, &keys, value)?;
    let raw = raw.replace(edited);
    // Resolve references only in the validation copy. Persist the authored values.
    let result = effective(&raw, &path, id)?;
    save(&path, &raw)?;
    Ok(result)
}
fn effective(raw: &OrderedValue, path: &Path, id: &str) -> Result<String, SettingWriteError> {
    let mut instructions = Vec::new();
    let source = serde_json::to_string(raw).map_err(parse_error)?;
    let layer = loader::parse_layer(
        &source,
        path.parent().unwrap_or(Path::new(".")),
        &mut instructions,
    )?;
    let config = normalize::normalize(layer)?;
    if id == "permission.bash" {
        return Ok(match config.permissions.defaults.shell {
            PermissionMode::Allow => "allow",
            PermissionMode::Ask => "ask",
            PermissionMode::Deny => "deny",
        }
        .into());
    }
    let normalized = serde_json::to_value(config).map_err(parse_error)?;
    let value = normalized
        .pointer(&format!("/{}", id.replace('.', "/")))
        .ok_or_else(|| SettingWriteError::UnsupportedWrite(id.into()))?;
    if value.is_null() {
        if id == "subagents.sampling_limit" {
            let count = normalized
                .pointer("/subagents/max_concurrent")
                .and_then(Value::as_i64)
                .map_or(32, |count| {
                    usize::try_from(count.max(1)).unwrap_or(usize::MAX)
                })
                .min(512);
            return Ok(count.to_string());
        }
        if let Some(default) = setting_definition(id).and_then(|setting| setting.default_value) {
            return Ok(default.into());
        }
    }
    Ok(value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned))
}
fn setting_path(raw: &mut Value, id: &str) -> Result<Vec<String>, SettingWriteError> {
    let root = raw.as_object_mut().ok_or(SettingWriteError::InvalidRoot)?;
    if id == "hashline_edit" {
        alias(root, "hashlineEdit", "hashline_edit")?;
    }
    if id == "permission.bash" {
        alias(root, "permissions", "permission")?;
        let permission = root.entry("permission").or_insert_with(|| json!({}));
        if permission.is_string() {
            *permission = json!({"*":permission.clone()});
        }
        let fields = permission
            .as_object_mut()
            .ok_or(SettingWriteError::InvalidRoot)?;
        if fields.contains_key("defaults") || fields.contains_key("rules") {
            // A scalar choice replaces the shell-specific policy, retaining other tools' rules.
            if let Some(rules) = fields.get_mut("rules").and_then(Value::as_object_mut) {
                rules.remove("shell");
            }
            return Ok(["permission", "defaults", "shell"]
                .map(str::to_owned)
                .into());
        }
        alias(fields, "shell", "bash")?;
    }
    runtime::normalize_aliases(raw)?;
    Ok(id.split('.').map(str::to_owned).collect())
}
fn replace(
    raw: &mut Value,
    keys: &[String],
    value: Option<Value>,
) -> Result<(), SettingWriteError> {
    let Some((key, rest)) = keys.split_first() else {
        return Err(SettingWriteError::InvalidRoot);
    };
    let fields = raw.as_object_mut().ok_or(SettingWriteError::InvalidRoot)?;
    if rest.is_empty() {
        if let Some(value) = value {
            fields.insert(key.clone(), value);
        } else {
            fields.remove(key);
        }
    } else {
        replace(
            fields.entry(key.clone()).or_insert_with(|| json!({})),
            rest,
            value,
        )?;
    }
    Ok(())
}
fn raw_file(path: &Path) -> Result<OrderedValue, SettingWriteError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(OrderedValue::from(json!({})))
        }
        Err(source) => {
            return Err(SettingWriteError::ReadFile {
                path: path.display().to_string(),
                source,
            })
        }
        Ok(metadata) if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 => {
            return Err(SettingWriteError::Parse(
                "config must be a regular file no larger than 4 MiB".into(),
            ))
        }
        Ok(_) => {}
    }
    Ok(OrderedValue::parse(&loader::read_text(path)?)?)
}
fn absolute(path: &Path) -> Result<PathBuf, SettingWriteError> {
    if path.is_absolute() {
        Ok(path.into())
    } else {
        std::env::current_dir()
            .map(|root| root.join(path))
            .map_err(|source| write_error(path, source))
    }
}
fn lock(path: &Path) -> Result<File, SettingWriteError> {
    let parent = path.parent().ok_or(SettingWriteError::InvalidRoot)?;
    crate::store::create_private_dir(parent).map_err(|e| write_error(path, e))?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(parent.join(".harness-settings.lock"))
        .map_err(|e| write_error(path, e))?;
    lock.try_lock()
        .map_err(|e| write_error(path, std::io::Error::other(e)))?;
    Ok(lock)
}
fn save(path: &Path, value: &OrderedValue) -> Result<(), SettingWriteError> {
    let mut source = serde_json::to_string_pretty(value).map_err(parse_error)?;
    source.push('\n');
    crate::store::write_private_atomic(path, source.as_bytes()).map_err(|e| write_error(path, e))
}
fn parse_error(error: impl std::fmt::Display) -> SettingWriteError {
    normalize::parse_error(error).into()
}
fn write_error(path: &Path, source: std::io::Error) -> SettingWriteError {
    SettingWriteError::WriteFile {
        path: path.display().to_string(),
        source,
    }
}

macro_rules! toggles {
    ($($id:literal => $read:ident, $write:ident, $reset:ident; )*) => {$(
        pub fn $read(path: &Path) -> Result<bool, SettingWriteError> { read_project_setting_value(path, $id).map(|value| value.as_deref() == Some("true")) }
        pub fn $write(path: &Path, value: bool) -> Result<bool, SettingWriteError> { write_project_setting_bool(path, $id, value) }
        pub fn $reset(path: &Path) -> Result<bool, SettingWriteError> { reset_project_setting_to_default(path, $id).map(|value| value == "true") }
    )*};
}
toggles! {
    "hashline_edit" => read_effective_hashline_edit, write_project_hashline_edit, reset_project_hashline_edit;
    "runtime.compaction.enabled" => read_effective_compaction_enabled, write_project_compaction_enabled, reset_project_compaction_enabled;
    "runtime.compaction.auto_retry_overflow" => read_effective_compaction_auto_retry_overflow, write_project_compaction_auto_retry_overflow, reset_project_compaction_auto_retry_overflow;
    "runtime.compaction.structured_summary_contract" => read_effective_compaction_structured_summary_contract, write_project_compaction_structured_summary_contract, reset_project_compaction_structured_summary_contract;
    "runtime.compaction.estimated_token_triggers" => read_effective_compaction_estimated_token_triggers, write_project_compaction_estimated_token_triggers, reset_project_compaction_estimated_token_triggers;
    "runtime.deterministic.enabled" => read_effective_deterministic_enabled, write_project_deterministic_enabled, reset_project_deterministic_enabled;
}
