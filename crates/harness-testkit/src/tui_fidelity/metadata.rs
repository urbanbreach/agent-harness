use std::collections::BTreeSet;

use super::error::ScenarioError;
use super::types::AdapterKind;

pub(super) fn validate_schema_version(version: &str) -> Result<(), ScenarioError> {
    if version == super::SCENARIO_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ScenarioError::InvalidSchemaVersion {
            expected: super::SCENARIO_SCHEMA_VERSION.to_owned(),
            observed: version.to_owned(),
        })
    }
}

pub(super) fn validate_id(id: &str) -> Result<(), ScenarioError> {
    if id.is_empty() {
        return Err(ScenarioError::EmptyScenarioId);
    }
    let valid = id.len() <= 64
        && id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && id.as_bytes()[0].is_ascii_lowercase();
    if valid {
        Ok(())
    } else {
        Err(ScenarioError::InvalidScenarioId)
    }
}

pub(super) fn validate_adapters(adapters: &[AdapterKind]) -> Result<(), ScenarioError> {
    if adapters.is_empty() {
        return Err(ScenarioError::NoAdapters);
    }
    let mut seen = BTreeSet::new();
    for adapter in adapters {
        if !seen.insert(*adapter) {
            return Err(ScenarioError::DuplicateAdapter(*adapter));
        }
    }
    Ok(())
}

pub(super) fn validate_adapter_selection(
    adapters: &[AdapterKind],
    requested: AdapterKind,
) -> Result<(), ScenarioError> {
    if adapters.contains(&requested) {
        Ok(())
    } else {
        Err(ScenarioError::AdapterNotSelected(requested))
    }
}
