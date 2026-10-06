use harness::CliDeps;
use harness_core::{
    config::{
        resolve_model_selection, HarnessConfig, SubagentCatalogAuthority, SubagentCatalogModel,
        SubagentDiscoveryContext, SubagentModelCatalog,
    },
    coord::CoordinatorConfig,
};
use std::path::PathBuf;

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
    let home = deps
        .env_var_value("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let user_root = deps
        .env_var_value("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".config")))
        .map(|root| root.join("harness"));
    let discovery = SubagentDiscoveryContext {
        cwd: workspace,
        project_trusted: true,
        home,
        user_root,
        bundled_root: None,
        plugins: Vec::new(),
        cli_definitions: Vec::new(),
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
    if let Some(model) = &config.small_model {
        for name in ["scout", "sonic"] {
            result
                .subagents
                .models
                .entry(name.into())
                .or_insert_with(|| model.clone());
        }
    }
    for model in result.subagents.models.values_mut() {
        if !model.eq_ignore_ascii_case("inherit")
            && let Ok(selection) = resolve_model_selection(config, model, None)
        {
            *model = selection.primary.model_ref;
        }
    }
    Ok(())
}
