//! Compaction threshold values and the persisted grammar of the per-model and per-agent maps.
use super::numbers::positive;
use super::*;
use num_traits::ToPrimitive;
use serde::de::Error;
use serde_json::Value;
use std::borrow::Cow;

/// A trigger percentage above 0 and at most 100. Decimals are allowed (`0.5`, `80.5%`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompactionThresholdPercent(f64);

// The constructor rejects NaN, so equality is total.
impl Eq for CompactionThresholdPercent {}

impl CompactionThresholdPercent {
    pub fn new(value: f64) -> Result<Self, &'static str> {
        if value.is_finite() && value > 0.0 && value <= 100.0 {
            Ok(Self(value))
        } else {
            Err("compaction percentage must be above 0 and at most 100")
        }
    }

    pub const fn get(self) -> f64 {
        self.0
    }

    /// Parse a trimmed `N%` string such as `80%` or `80.5%`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let digits = text
            .trim()
            .strip_suffix('%')
            .ok_or("percentages are written N%, for example 80% or 80.5%")?;
        let shaped = !digits.is_empty()
            && digits.matches('.').count() <= 1
            && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
            && digits.starts_with(|c: char| c.is_ascii_digit())
            && digits.ends_with(|c: char| c.is_ascii_digit());
        if !shaped {
            return Err("percentages are written N%, for example 80% or 80.5%".into());
        }
        let value = digits.parse::<f64>().map_err(|e| e.to_string())?;
        Self::new(value).map_err(str::to_owned)
    }

    /// A JSON number or an `N%` string.
    pub(super) fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Number(number) => number
                .as_f64()
                .ok_or_else(|| "compaction percentage must be a number".to_owned())
                .and_then(|value| Self::new(value).map_err(str::to_owned)),
            Value::String(text) => Self::parse(text),
            _ => Err("compaction percentage must be a number or an N% string".into()),
        }
    }
}

impl std::fmt::Display for CompactionThresholdPercent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<f64> for CompactionThresholdPercent {
    type Error = &'static str;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CompactionThresholdPercent> for f64 {
    fn from(value: CompactionThresholdPercent) -> Self {
        value.0
    }
}

impl Serialize for CompactionThresholdPercent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Whole percentages keep the integer spelling they had before decimals existed.
        match self.0.to_u64().filter(|_| self.0.fract() == 0.0) {
            Some(whole) => serializer.serialize_u64(whole),
            None => serializer.serialize_f64(self.0),
        }
    }
}

impl<'de> Deserialize<'de> for CompactionThresholdPercent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_value(&Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for CompactionThresholdPercent {
    fn schema_name() -> Cow<'static, str> {
        "CompactionThresholdPercent".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": ["number", "string"]})
    }
}

/// How one entry of a threshold map triggers compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionThreshold {
    Percent(CompactionThresholdPercent),
    Tokens { tokens: NonZeroU32 },
    Base { base: NonZeroU32 },
}

#[derive(Serialize)]
#[serde(untagged)]
enum ThresholdRepr {
    Text(String),
    Tokens { tokens: NonZeroU32 },
    Base { base: NonZeroU32 },
}

impl Serialize for CompactionThreshold {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let repr = match *self {
            Self::Percent(percent) => ThresholdRepr::Text(format!("{percent}%")),
            Self::Tokens { tokens } => ThresholdRepr::Tokens { tokens },
            Self::Base { base } => ThresholdRepr::Base { base },
        };
        repr.serialize(serializer)
    }
}

impl JsonSchema for CompactionThreshold {
    fn schema_name() -> Cow<'static, str> {
        "CompactionThreshold".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": ["number", "string", "object"]})
    }
}

/// Which map an entry belongs to. A bare number means a base window in the model map and a
/// trigger token count in the agent map.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    Model,
    Agent,
}

const POSITIVE: &str = "token counts must be positive whole numbers";

impl CompactionThreshold {
    /// `None` is a `null` entry: it removes any inherited key.
    pub(super) fn parse(value: &Value, scope: Scope) -> Result<Option<Self>, String> {
        match value {
            Value::Null => Ok(None),
            Value::Number(_) => {
                let tokens = positive(value).ok_or(POSITIVE)?;
                Ok(Some(match scope {
                    Scope::Model => Self::Base { base: tokens },
                    Scope::Agent => Self::Tokens { tokens },
                }))
            }
            Value::String(text) => Self::parse_text(text, scope).map(Some),
            Value::Object(fields) => {
                let mut entries = fields.iter();
                match (entries.next(), entries.next()) {
                    (Some((key, value)), None) if key == "tokens" => Ok(Some(Self::Tokens {
                        tokens: positive(value).ok_or(POSITIVE)?,
                    })),
                    (Some((key, _)), None) if key == "base" && scope == Scope::Agent => {
                        Err("agent thresholds do not accept base windows".into())
                    }
                    (Some((key, value)), None) if key == "base" => Ok(Some(Self::Base {
                        base: positive(value).ok_or(POSITIVE)?,
                    })),
                    _ => Err("threshold objects take exactly one of tokens or base".into()),
                }
            }
            _ => Err(
                "thresholds are numbers, N% or fN strings, or {tokens} or {base} objects".into(),
            ),
        }
    }

    fn parse_text(text: &str, scope: Scope) -> Result<Self, String> {
        let trimmed = text.trim();
        if trimmed.ends_with('%') {
            return CompactionThresholdPercent::parse(trimmed).map(Self::Percent);
        }
        let Some(digits) = trimmed.strip_prefix(['f', 'F']) else {
            return Err("threshold strings must be N% or fN".into());
        };
        if scope == Scope::Agent {
            return Err("agent thresholds do not accept fN strings; use a number or N%".into());
        }
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err("fixed thresholds are written fN with digits only".into());
        }
        // Digit strings beyond u64 saturate like any other oversized token count.
        let tokens = digits
            .parse::<u64>()
            .map_or(u32::MAX, |value| u32::try_from(value).unwrap_or(u32::MAX));
        NonZeroU32::new(tokens)
            .map(|tokens| Self::Tokens { tokens })
            .ok_or_else(|| POSITIVE.to_owned())
    }
}

/// Whether a persisted entry is the legacy numeric percentage form (1 to 100).
pub(super) fn legacy_percent(value: &Value) -> Option<u8> {
    value
        .as_u64()
        .filter(|percent| (1..=100).contains(percent))
        .and_then(|percent| u8::try_from(percent).ok())
}
