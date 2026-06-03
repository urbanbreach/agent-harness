use std::collections::BTreeMap;

use crate::text::non_empty_trimmed;

use super::ConfigError;

pub(super) fn merge_string_alias(
    target: &mut impl StringAliasTarget,
    alias: Option<String>,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError> {
    let Some(alias) = alias.map(|value| value.trim().to_string()) else {
        return Ok(());
    };
    if alias.is_empty() {
        return Ok(());
    }

    match target.current_value() {
        Some(current) if current == alias => Ok(()),
        Some(_) => Err(ConfigError::InvalidReference(format!(
            "{target_path} conflicts with {alias_path}; use one value"
        ))),
        None => {
            target.set_value(alias);
            Ok(())
        }
    }
}

pub(super) fn merge_map_alias(
    target: &mut BTreeMap<String, String>,
    alias: BTreeMap<String, String>,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError> {
    merge_alias_value(target, alias, BTreeMap::is_empty, target_path, alias_path)
}

pub(super) fn merge_vec_alias(
    target: &mut Vec<String>,
    alias: Vec<String>,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError> {
    merge_alias_value(target, alias, Vec::is_empty, target_path, alias_path)
}

pub(super) fn merge_option_alias<T: PartialEq>(
    target: &mut Option<T>,
    alias: Option<T>,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError> {
    match (target.as_ref(), alias) {
        (_, None) => Ok(()),
        (None, Some(alias)) => {
            *target = Some(alias);
            Ok(())
        }
        (Some(current), Some(alias)) if *current == alias => Ok(()),
        (Some(_), Some(_)) => Err(ConfigError::InvalidReference(format!(
            "{target_path} conflicts with {alias_path}; use one value"
        ))),
    }
}

fn merge_alias_value<T>(
    target: &mut T,
    alias: T,
    is_empty: impl Fn(&T) -> bool,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError>
where
    T: PartialEq,
{
    if is_empty(&alias) {
        return Ok(());
    }
    if is_empty(target) {
        *target = alias;
        return Ok(());
    }
    if *target == alias {
        return Ok(());
    }

    Err(ConfigError::InvalidReference(format!(
        "{target_path} conflicts with {alias_path}; use one value"
    )))
}

pub(super) trait StringAliasTarget {
    fn current_value(&self) -> Option<&str>;
    fn set_value(&mut self, value: String);
}

impl StringAliasTarget for String {
    fn current_value(&self) -> Option<&str> {
        non_empty_trimmed(self)
    }

    fn set_value(&mut self, value: String) {
        *self = value;
    }
}

impl StringAliasTarget for Option<String> {
    fn current_value(&self) -> Option<&str> {
        self.as_deref().and_then(non_empty_trimmed)
    }

    fn set_value(&mut self, value: String) {
        *self = Some(value);
    }
}
