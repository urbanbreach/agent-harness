use super::*;

fn catalog(ids: &[&str]) -> SubagentModelCatalog {
    SubagentModelCatalog {
        authority: SubagentCatalogAuthority::Complete,
        models: ids
            .iter()
            .map(|id| SubagentCatalogModel {
                id: (*id).into(),
                family: Some("xai".into()),
                picker_eligible: true,
            })
            .collect(),
    }
}

fn parent_context<'a>(
    definitions: &'a SubagentDefinitionSnapshot,
    catalog: &'a SubagentModelCatalog,
    tools: &'a [SubagentTool],
) -> SubagentDefinitionContext<'a> {
    SubagentDefinitionContext {
        cwd: Path::new("/workspace"),
        definitions,
        parent_model: "parent",
        parent_reasoning_effort: Some("medium"),
        parent_max_turns: NonZeroU32::new(2),
        allowed_types: None,
        catalog: Some(catalog),
        model_selection: SubagentModelSelection::Selectable,
        tools,
        operator_allowed_tools: None,
        operator_denied_tools: &[],
        parent_permission_mode: None,
        managed_block_bypass: false,
        child_depth: 1,
        parent_mcp_servers: &[],
        parent_skills: &[],
    }
}

mod discovery;
mod resolution;
mod settings;
