use super::{normalize::parse_error, ConfigError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Permission patterns use last-match precedence. Keep authored order local to
// config files; enabling serde_json/preserve_order changes unrelated UI output.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum OrderedValue {
    Object(IndexMap<String, Self>),
    Array(Vec<Self>),
    Scalar(Value),
}

impl OrderedValue {
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        let value = json5::from_str(raw).map_err(parse_error)?;
        if matches!(value, Self::Object(_)) {
            Ok(value)
        } else {
            Err(ConfigError("expected an object".into()))
        }
    }

    pub fn json(&self) -> Result<Value, ConfigError> {
        serde_json::to_value(self).map_err(parse_error)
    }

    pub fn object(&self) -> Option<&IndexMap<String, Self>> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Self> {
        self.object()?.get(key)
    }

    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), ConfigError> {
        if let Self::Object(object) = self {
            if let Some(value) = object.shift_remove(old) {
                if object.contains_key(new) {
                    return Err(ConfigError(format!("use only one of {old} and {new}")));
                }
                object.insert(new.into(), value);
            }
        }
        Ok(())
    }

    pub fn merge(&mut self, incoming: Self) {
        match (self, incoming) {
            (Self::Object(target), Self::Object(incoming)) => {
                for (key, value) in incoming {
                    target
                        .entry(key)
                        .or_insert(Self::Scalar(Value::Null))
                        .merge(value);
                }
            }
            (target, value) => *target = value,
        }
    }

    // Apply validated edits or reference expansion without sorting untouched maps.
    pub fn replace(self, value: Value) -> Self {
        match (self, value) {
            (Self::Object(old), Value::Object(mut incoming)) => {
                let mut output = IndexMap::with_capacity(incoming.len());
                for (key, previous) in old {
                    if let Some(value) = incoming.remove(&key) {
                        output.insert(key, previous.replace(value));
                    }
                }
                output.extend(
                    incoming
                        .into_iter()
                        .map(|(key, value)| (key, Self::from(value))),
                );
                Self::Object(output)
            }
            (Self::Array(old), Value::Array(incoming)) => {
                let mut old = old.into_iter();
                Self::Array(
                    incoming
                        .into_iter()
                        .map(|value| {
                            old.next()
                                .unwrap_or(Self::Scalar(Value::Null))
                                .replace(value)
                        })
                        .collect(),
                )
            }
            (_, value) => Self::from(value),
        }
    }
}

impl From<Value> for OrderedValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Object(object) => Self::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key, Self::from(value)))
                    .collect(),
            ),
            Value::Array(array) => Self::Array(array.into_iter().map(Self::from).collect()),
            scalar => Self::Scalar(scalar),
        }
    }
}
