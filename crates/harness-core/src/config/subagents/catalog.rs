use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubagentCatalogAuthority {
    Complete,
    #[default]
    Provisional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentCatalogModel {
    pub id: String,
    /// Explicit backend catalog family, never inferred from a model name.
    pub family: Option<String>,
    pub picker_eligible: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentModelCatalog {
    pub authority: SubagentCatalogAuthority,
    pub models: Vec<SubagentCatalogModel>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubagentModelSelection {
    #[default]
    Selectable,
    Inherited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentCatalogClassification {
    Provisional,
    Empty,
    UnknownFamily,
    FirstPartyOnly,
    ThirdPartyOnly,
    Mixed,
}

/// A value captured once by the constructing actor, not reclassified per spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentModelPolicy {
    pub selection: SubagentModelSelection,
    pub classification: SubagentCatalogClassification,
    pub model_slugs: Vec<String>,
}

impl SubagentModelPolicy {
    pub fn latch(
        inheritance: bool,
        catalog: &SubagentModelCatalog,
        forked: Option<SubagentModelSelection>,
    ) -> Self {
        let eligible: Vec<_> = catalog
            .models
            .iter()
            .filter(|model| model.picker_eligible)
            .collect();
        let classification = if catalog.authority == SubagentCatalogAuthority::Provisional {
            SubagentCatalogClassification::Provisional
        } else if eligible.is_empty() {
            SubagentCatalogClassification::Empty
        } else if eligible.iter().any(|model| {
            model
                .family
                .as_deref()
                .is_none_or(|family| family.trim().is_empty())
        }) {
            SubagentCatalogClassification::UnknownFamily
        } else {
            let first = eligible
                .iter()
                .filter(|model| model.family.as_deref().map(str::trim) == Some("xai"))
                .count();
            if first == eligible.len() {
                SubagentCatalogClassification::FirstPartyOnly
            } else if first == 0 {
                SubagentCatalogClassification::ThirdPartyOnly
            } else {
                SubagentCatalogClassification::Mixed
            }
        };
        let selection = forked.unwrap_or_else(|| {
            if inheritance && classification == SubagentCatalogClassification::FirstPartyOnly {
                SubagentModelSelection::Inherited
            } else {
                SubagentModelSelection::Selectable
            }
        });
        Self {
            selection,
            classification,
            model_slugs: eligible.into_iter().map(|model| model.id.clone()).collect(),
        }
    }
}

impl SubagentModelCatalog {
    pub fn contains(&self, id: &str) -> bool {
        self.models.iter().any(|model| model.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentTypeDescriptor {
    pub name: String,
    pub description: String,
}

/// Bounded schema hints are not admission policy. Runtime resolves the full discovery snapshot.
pub fn subagent_type_schema(types: &[SubagentTypeDescriptor]) -> Option<serde_json::Value> {
    let listed: Vec<_> = types
        .iter()
        .filter(|entry| entry.name.len() <= 128)
        .take(64)
        .collect();
    if listed.is_empty() {
        return None;
    }
    let lines: Vec<_> = listed
        .iter()
        .map(|entry| {
            let mut description = entry
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if description.len() > 200 {
                let end = description.floor_char_boundary(197);
                description.truncate(end);
                description.push('\u{2026}');
            }
            format!("- {}: {description}", entry.name)
        })
        .collect();
    Some(serde_json::json!({
        "type": "string", "enum": listed.iter().map(|entry| &entry.name).collect::<Vec<_>>(),
        "description": format!("Omit for a general-purpose subagent. Set it to hand the task to one of these agents:\n{}", lines.join("\n"))
    }))
}
