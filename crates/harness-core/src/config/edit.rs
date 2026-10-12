use super::*;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditMode {
    #[default]
    Auto,
    Hashline,
    Replace,
    ApplyPatch,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EditConfig {
    pub mode: EditMode,
    #[serde(alias = "modelVariants")]
    pub model_variants: BTreeMap<String, EditMode>,
}

pub(super) fn normalize_compat(value: &mut Value) -> Result<(), ConfigError> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| ConfigError("expected an object".into()))?;
    normalize::rename(object, "hashlineEdit", "hashline_edit")?;
    if let Some(legacy) = object.remove("hashline_edit") {
        if object.contains_key("edit") {
            return Err(ConfigError("use only one of hashline_edit and edit".into()));
        }
        let enabled = legacy
            .as_bool()
            .ok_or_else(|| ConfigError("hashline_edit must be a boolean".into()))?;
        if !enabled {
            object.insert("edit".into(), json!({"mode":"replace"}));
        }
    }
    if let Some(edit) = object.get_mut("edit").and_then(Value::as_object_mut) {
        normalize::rename(edit, "modelVariants", "model_variants")?;
    }
    Ok(())
}
