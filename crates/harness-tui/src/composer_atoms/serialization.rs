use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug)]
pub enum SerializationError {
    Json(serde_json::Error),
}

impl std::fmt::Display for SerializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "composer atom JSON error: {error}"),
        }
    }
}

impl std::error::Error for SerializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
        }
    }
}

impl From<serde_json::Error> for SerializationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn serialize<T: Serialize>(value: &T) -> Result<String, SerializationError> {
    serde_json::to_string(value).map_err(SerializationError::from)
}

pub fn deserialize<T: DeserializeOwned>(json: &str) -> Result<T, SerializationError> {
    serde_json::from_str(json).map_err(SerializationError::from)
}
