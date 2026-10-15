//! Compaction maps keyed by model selector or agent name.
//!
//! A `null` map is empty. A `null` entry deserializes to no entry, which removes the key that
//! an earlier configuration layer set, because layers merge before they deserialize.
use super::threshold::Scope;
use super::*;
use serde::de::Error;
use serde_json::Value;

/// `provider/model` or `provider:model`, optionally ending in a single `*`, without whitespace.
fn valid_selector(key: &str) -> bool {
    let qualified = key.split_once(':').or_else(|| key.split_once('/'));
    qualified.is_some_and(|(provider, model)| {
        !provider.is_empty()
            && !model.is_empty()
            && !provider.contains('*')
            && !model.trim_end_matches('*').contains('*')
    }) && key.trim() == key
        && !key.chars().any(char::is_whitespace)
        && key.bytes().filter(|b| *b == b'*').count() <= 1
}

fn entries<T>(
    value: Value,
    selectors: bool,
    parse: impl Fn(&Value) -> Result<Option<T>, String>,
) -> Result<BTreeMap<String, T>, String> {
    let fields = match value {
        Value::Null => return Ok(BTreeMap::new()),
        Value::Object(fields) => fields,
        _ => return Err("must be an object".into()),
    };
    let mut map = BTreeMap::new();
    for (key, value) in fields {
        if selectors && !valid_selector(&key) {
            return Err(format!(
                "`{key}`: keys require provider:model or provider/model with an optional terminal *"
            ));
        }
        if let Some(entry) = parse(&value).map_err(|error| format!("`{key}`: {error}"))? {
            map.insert(key, entry);
        }
    }
    Ok(map)
}

pub(super) fn model_thresholds<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, CompactionThreshold>, D::Error> {
    entries(Value::deserialize(deserializer)?, true, |value| {
        CompactionThreshold::parse(value, Scope::Model)
    })
    .map_err(D::Error::custom)
}

pub(super) fn agent_thresholds<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, CompactionThreshold>, D::Error> {
    entries(Value::deserialize(deserializer)?, false, |value| {
        CompactionThreshold::parse(value, Scope::Agent)
    })
    .map_err(D::Error::custom)
}

pub(super) fn remote_models<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, RemoteModelSettings>, D::Error> {
    entries(Value::deserialize(deserializer)?, true, |value| {
        if value.is_null() {
            return Ok(None);
        }
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .map_err(D::Error::custom)
}

pub(super) fn summary_models<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    entries(
        Value::deserialize(deserializer)?,
        true,
        |value| match value {
            Value::Null => Ok(None),
            Value::String(model) if !model.trim().is_empty() => Ok(Some(model.trim().to_owned())),
            _ => Err("summary models are non-empty model references".into()),
        },
    )
    .map_err(D::Error::custom)
}
