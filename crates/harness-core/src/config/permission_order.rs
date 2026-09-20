//! Source order for permission selectors only; the existing config parser owns values and validation.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

#[derive(Default)]
pub(super) enum PermissionOrder {
    #[default]
    Replaced,
    Fields(BTreeMap<String, Self>),
    Selectors(Vec<String>),
}

impl PermissionOrder {
    pub(super) fn get(&self, key: &str) -> &Self {
        match self {
            Self::Fields(fields) => fields.get(key).unwrap_or(&Self::Replaced),
            _ => &Self::Replaced,
        }
    }

    pub(super) fn take(&mut self, key: &str) -> Self {
        match self {
            Self::Fields(fields) => fields.remove(key).unwrap_or_default(),
            _ => Self::Replaced,
        }
    }

    pub(super) fn get_or_alias(&self, key: &str, alias: &str) -> &Self {
        match self {
            Self::Fields(fields) => fields
                .get(key)
                .or_else(|| fields.get(alias))
                .unwrap_or(&Self::Replaced),
            _ => &Self::Replaced,
        }
    }

    pub(super) fn fold_alias(&mut self, alias: &str, canonical: &str) {
        if let Self::Fields(fields) = self {
            if let Some(value) = fields.remove(alias) {
                fields
                    .entry(canonical.to_string())
                    .or_default()
                    .merge(value);
            }
        }
    }

    pub(super) fn insert(&mut self, key: &str, value: Self) {
        if !matches!(self, Self::Fields(_)) {
            *self = Self::Fields(BTreeMap::new());
        }
        if let Self::Fields(fields) = self {
            fields.insert(key.to_string(), value);
        }
    }

    pub(super) fn merge(&mut self, overlay: Self) {
        match (self, overlay) {
            (Self::Fields(base), Self::Fields(overlay)) => {
                for (key, value) in overlay {
                    base.entry(key).or_default().merge(value);
                }
            }
            (Self::Selectors(base), Self::Selectors(overlay)) => {
                let overridden: BTreeSet<_> = overlay.iter().collect();
                base.retain(|key| !overridden.contains(key));
                base.extend(overlay);
            }
            (base, overlay) => *base = overlay,
        }
    }
}

impl<'de> Deserialize<'de> for PermissionOrder {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Scope::Root.deserialize(deserializer)
    }
}

#[derive(Clone, Copy)]
enum Scope {
    Root,
    Agents,
    Agent,
    Permission,
    InternalPermission,
    LegacyRules,
    Selectors,
    Ignore,
}

impl Scope {
    fn child(self, key: &str) -> Self {
        match (self, key) {
            (Self::Root, "permission") | (Self::Agent, "permission" | "permissions") => {
                Self::Permission
            }
            (Self::Root, "permissions") => Self::InternalPermission,
            (Self::Root, "agent") => Self::Agents,
            (Self::Agents, _) => Self::Agent,
            (Self::Permission | Self::InternalPermission, "rules") => Self::LegacyRules,
            (
                Self::Permission,
                "bash" | "shell" | "edit" | "task" | "read" | "external_directory",
            ) => Self::Selectors,
            _ => Self::Ignore,
        }
    }
}

impl<'de> DeserializeSeed<'de> for Scope {
    type Value = PermissionOrder;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        if matches!(self, Self::Ignore) {
            IgnoredAny::deserialize(deserializer)?;
            Ok(PermissionOrder::Replaced)
        } else {
            deserializer.deserialize_any(self)
        }
    }
}

impl<'de> Visitor<'de> for Scope {
    type Value = PermissionOrder;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("permission configuration")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut fields = BTreeMap::new();
        let mut selectors = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            if matches!(self, Self::Selectors) {
                map.next_value::<IgnoredAny>()?;
                selectors.push(key);
            } else {
                let scope = self.child(&key);
                let value = map.next_value_seed(scope)?;
                if !matches!(scope, Self::Ignore) || matches!(self, Self::LegacyRules) {
                    // Match Value's last-value-wins behavior for duplicate fields.
                    fields.insert(key, value);
                }
            }
        }
        if matches!(self, Self::Selectors) {
            let mut seen = BTreeSet::new();
            selectors.reverse();
            selectors.retain(|key| seen.insert(key.clone()));
            selectors.reverse();
            return Ok(PermissionOrder::Selectors(selectors));
        }

        Ok(PermissionOrder::Fields(fields))
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(PermissionOrder::Replaced)
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(PermissionOrder::Replaced)
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(PermissionOrder::Replaced)
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(PermissionOrder::Replaced)
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(PermissionOrder::Replaced)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(PermissionOrder::Replaced)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        while sequence.next_element::<IgnoredAny>()?.is_some() {}
        Ok(PermissionOrder::Replaced)
    }
}
