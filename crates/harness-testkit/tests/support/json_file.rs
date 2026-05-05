use std::fs;
use std::path::Path;

use serde_json::Value;

pub(crate) fn read_required_json(path: &Path) -> Result<Value, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read JSON artifact {}: {err}", path.display()))?;
    serde_json::from_str(&body)
        .map_err(|err| format!("failed to parse JSON artifact {}: {err}", path.display()))
}
