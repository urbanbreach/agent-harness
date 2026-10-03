use super::SpawnSubagentInput;
use crate::config::{
    subagent_type_schema, SubagentDefinitionSnapshot, SubagentModelCatalog, SubagentModelPolicy,
    SubagentModelSelection, SubagentRuntimeConfig,
};
use serde_json::Value;

/// Schema hints come from the same resolved definition snapshot used for admission.
/// The actor still validates the complete, untruncated set of definitions.
pub fn spawn_subagent_schema(
    settings: &SubagentRuntimeConfig,
    definitions: &SubagentDefinitionSnapshot,
    catalog: Option<&SubagentModelCatalog>,
    allowed_types: Option<&[String]>,
) -> Value {
    let mut schema: Value = schemars::schema_for!(SpawnSubagentInput).into();
    let types = definitions.selectable_types(&settings.toggle, allowed_types);
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        if let Some(type_schema) = subagent_type_schema(&types) {
            properties.insert("subagent_type".into(), type_schema);
        }
        if catalog.is_some_and(|catalog| {
            SubagentModelPolicy::latch(settings.model_inheritance, catalog, None).selection
                == SubagentModelSelection::Inherited
        }) {
            properties.remove("model");
        }
    }
    schema
}
