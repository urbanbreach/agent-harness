use harness::CliDeps;
use harness_core::{
    config::{
        resolve_model_selection, HarnessConfig, SubagentCatalogAuthority, SubagentCatalogModel,
        SubagentDiscoveryContext, SubagentModelCatalog,
    },
    coord::CoordinatorConfig,
};

pub(super) fn configure(
    config: &HarnessConfig,
    deps: &CliDeps,
    result: &mut CoordinatorConfig,
) -> Result<(), String> {
    result.subagents = config.subagents.resolve_with_lookup(
        None,
        &config.features,
        &Default::default(),
        &Default::default(),
        &|name| deps.env_var_value(name),
    );
    let workspace = deps.current_dir().map_err(|e| e.to_string())?;
    let user_root =
        harness_core::storage_paths::data_dir_from_lookup(&|name| deps.env_var_value(name));
    let discovery = SubagentDiscoveryContext {
        cwd: workspace,
        project_trusted: true,
        user_root,
    };
    result.subagent_definitions = Some(harness_core::config::discover_subagent_definitions(
        &result.subagents,
        &discovery,
    ));
    result.subagent_discovery = Some(discovery);
    result.subagent_model_catalog = Some(SubagentModelCatalog {
        authority: SubagentCatalogAuthority::Complete,
        models: result
            .model_catalog
            .iter()
            .filter(|entry| entry.variant.is_none())
            .map(|entry| SubagentCatalogModel {
                id: format!("{}:{}", entry.provider, entry.model),
                family: config
                    .providers
                    .get(&entry.provider)
                    .and_then(|provider| provider.models().get(&entry.model))
                    .and_then(|model| model.metadata.family.clone()),
                picker_eligible: true,
            })
            .collect(),
    });
    result.subagents.model_roles = config.model_roles.clone();
    for model in [
        &mut result.subagents.model_roles.smol,
        &mut result.subagents.model_roles.slow,
    ]
    .into_iter()
    .flatten()
    {
        let target = resolve_model_selection(config, model, None)
            .map_err(|e| e.to_string())?
            .primary;
        *model = match target.variant {
            Some(variant) => format!("{}/{variant}", target.model_ref),
            None => target.model_ref,
        };
    }
    for (provider, backend) in &config.providers {
        for (model, entry) in backend.models() {
            result.subagents.model_variants.insert(
                format!("{provider}:{model}"),
                entry
                    .variants
                    .iter()
                    .filter(|(_, variant)| !variant.disabled)
                    .map(|(name, _)| name.clone())
                    .collect(),
            );
        }
    }
    for model in result.subagents.models.values_mut() {
        if !model.eq_ignore_ascii_case("inherit")
            && let Ok(selection) = resolve_model_selection(config, model, None)
        {
            *model = match selection.primary.variant {
                Some(variant) => format!("{}/{variant}", selection.primary.model_ref),
                None => selection.primary.model_ref,
            };
        }
    }
    Ok(())
}
