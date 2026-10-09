use super::*;

mod tools;
use tools::{child_tools, definition_allowed_types};

#[derive(Debug, Clone, Default)]
pub struct SubagentDefinitionRequest {
    pub subagent_type: String,
    pub type_specified: bool,
    pub resume: bool,
    /// Model-facing explicit selection; hidden/validation gates apply only to fresh spawns.
    pub model: Option<String>,
    /// Host-injected override, not a model argument.
    pub runtime_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub variant: Option<String>,
    pub capability_mode: Option<SubagentCapabilityMode>,
    pub persona: Option<String>,
    pub isolation: Option<SubagentIsolationMode>,
}

#[derive(Debug)]
pub struct SubagentDefinitionContext<'a> {
    pub cwd: &'a Path,
    pub definitions: &'a SubagentDefinitionSnapshot,
    pub parent_model: &'a str,
    pub parent_reasoning_effort: Option<&'a str>,
    pub parent_variant: Option<&'a str>,
    pub parent_max_turns: Option<NonZeroU32>,
    pub allowed_types: Option<&'a [String]>,
    pub catalog: Option<&'a SubagentModelCatalog>,
    /// Policy latched by the actor during parent construction.
    pub model_selection: SubagentModelSelection,
    pub tools: &'a [SubagentTool],
    pub operator_allowed_tools: Option<&'a [String]>,
    pub operator_denied_tools: &'a [String],
    /// Operator/session definition override, not the parent's runtime authority.
    pub parent_permission_mode: Option<SubagentPermissionMode>,
    pub managed_block_bypass: bool,
    pub child_depth: u32,
    pub parent_mcp_servers: &'a [String],
    pub parent_skills: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSubagentDefinition {
    pub definition: SubagentDefinition,
    pub subagent_type: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub variant: Option<String>,
    pub capability_mode: SubagentCapabilityMode,
    pub isolation: SubagentIsolationMode,
    pub permission_mode: SubagentPermissionMode,
    pub max_turns: Option<NonZeroU32>,
    pub persona: Option<String>,
    pub persona_instructions: Option<String>,
    pub role_name: Option<String>,
    pub role_prompt: Option<String>,
    pub tools: Vec<SubagentTool>,
    pub inherited_mcp_servers: Vec<String>,
    pub inherited_skills: Vec<String>,
    pub allowed_subagent_types: Option<Vec<String>>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubagentResolutionError {
    #[error("Subagents are disabled")]
    FeatureDisabled,
    #[error("Unknown subagent type: {name}. Available types: {available:?}")]
    Unknown {
        name: String,
        available: Vec<String>,
    },
    #[error("Subagent '{name}' is disabled via [subagents.toggle]")]
    Disabled { name: String },
    #[error("agent can only spawn: {allowed:?}; '{name}' not allowed")]
    NotAllowed { name: String, allowed: Vec<String> },
    #[error("Explicit subagent model selection is unavailable for this catalog. Retry without the model argument; configured defaults will apply.")]
    HiddenModelSelection,
    #[error("Cannot validate Task.model: model catalog validator is unavailable.")]
    ValidationUnavailable,
    #[error("Model '{model}' is unavailable")]
    InvalidModel { model: String },
    #[error("persona resolution failed: {0}")]
    Persona(String),
}

fn gate(
    name: &str,
    config: &SubagentRuntimeConfig,
    context: &SubagentDefinitionContext<'_>,
) -> Result<SubagentDefinition, SubagentResolutionError> {
    let definition =
        context
            .definitions
            .definition(name)
            .ok_or_else(|| SubagentResolutionError::Unknown {
                name: name.into(),
                available: context.definitions.available_names(&config.toggle),
            })?;
    if !config.toggle.get(name).copied().unwrap_or(true) {
        return Err(SubagentResolutionError::Disabled { name: name.into() });
    }
    if let Some(allowed) = context.allowed_types
        && !allowed.iter().any(|entry| entry.eq_ignore_ascii_case(name))
    {
        return Err(SubagentResolutionError::NotAllowed {
            name: name.into(),
            allowed: allowed.to_vec(),
        });
    }
    Ok(definition)
}

/// Resolve against one explicit parent/catalog/tool snapshot. No I/O or mutable global config.
/// On resume the actor supplies the source type/persona and pins its source model after resolution.
pub fn resolve_subagent_definition(
    config: &SubagentRuntimeConfig,
    request: &SubagentDefinitionRequest,
    context: &SubagentDefinitionContext<'_>,
) -> Result<ResolvedSubagentDefinition, SubagentResolutionError> {
    if !config.enabled {
        return Err(SubagentResolutionError::FeatureDisabled);
    }
    let public_model = (!request.resume)
        .then_some(request.model.as_deref())
        .flatten()
        .map(str::trim)
        .filter(|model| !sentinel(model));
    let public_model = public_model
        .map(|model| model_role(config, model, context.parent_model))
        .transpose()?;
    if public_model.is_some() && context.model_selection == SubagentModelSelection::Inherited {
        return Err(SubagentResolutionError::HiddenModelSelection);
    }
    let mut name = if request.subagent_type.is_empty() {
        "task".into()
    } else {
        request.subagent_type.clone()
    };
    if !request.type_specified && !request.resume && name.eq_ignore_ascii_case("task") {
        let task_enabled = config.toggle.get("task").copied().unwrap_or(true)
            && context.allowed_types.is_none_or(|allowed| {
                allowed
                    .iter()
                    .any(|entry| entry.eq_ignore_ascii_case("task"))
            });
        if !task_enabled {
            let mut enabled = context
                .allowed_types
                .into_iter()
                .flatten()
                .filter(|name| config.toggle.get(*name).copied().unwrap_or(true));
            if let Some(only) = enabled.next()
                && enabled.next().is_none()
                && !only.eq_ignore_ascii_case("task")
            {
                name.clone_from(only);
            }
        }
    }
    let definition = match gate(&name, config, context) {
        Err(SubagentResolutionError::NotAllowed { allowed, .. })
            if !request.type_specified
                && !request.resume
                && name.eq_ignore_ascii_case("task")
                && allowed.len() == 1 =>
        {
            name.clone_from(&allowed[0]);
            gate(&name, config, context)?
        }
        outcome => outcome?,
    };
    if let Some(model) = public_model.filter(|model| *model != context.parent_model) {
        let catalog = context
            .catalog
            .ok_or(SubagentResolutionError::ValidationUnavailable)?;
        if configured_model(catalog, model).is_none() {
            return Err(SubagentResolutionError::InvalidModel {
                model: model.into(),
            });
        }
    }
    let role_name = if context.definitions.roles.contains_key(&name) {
        Some(name.clone())
    } else {
        request
            .persona
            .clone()
            .filter(|name| context.definitions.roles.contains_key(name))
    };
    let role = role_name
        .as_deref()
        .and_then(|name| context.definitions.roles.get(name));
    let persona = request
        .persona
        .as_deref()
        .map(|name| {
            context.definitions.personas.get(name).ok_or_else(|| {
                SubagentResolutionError::Persona(format!("persona \"{name}\" not found in config"))
            })
        })
        .transpose()?;
    let persona_instructions = persona
        .map(|persona| {
            let mut parts = Vec::new();
            if let Some(instructions) = &persona.instructions {
                parts.push(instructions.clone());
            }
            if let Some(file) = &persona.instructions_file {
                let path = persona
                    .source_dir
                    .as_deref()
                    .unwrap_or(context.cwd)
                    .join(file);
                let content = prompt_file(context.definitions, &path)
                    .map_err(SubagentResolutionError::Persona)?;
                parts.push(content);
            }
            if parts.is_empty() {
                return Err(SubagentResolutionError::Persona(
                    "persona has no instructions or instructions_file".into(),
                ));
            }
            Ok(parts.join("\n\n"))
        })
        .transpose()?;
    let mut warnings = Vec::new();
    let role_prompt = role.and_then(|role| {
        let file = role.prompt_file.as_deref()?;
        let path = role.source_dir.as_deref().unwrap_or(context.cwd).join(file);
        match prompt_file(context.definitions, &path) {
            Ok(content) => Some(content),
            Err(error) => {
                warnings.push(format!("role prompt_file \"{file}\": {error}"));
                None
            }
        }
    });
    let capability_mode = request
        .capability_mode
        .unwrap_or_default()
        .intersect(
            role.and_then(|role| role.default_capability_mode.as_deref())
                .and_then(parse_enum)
                .unwrap_or_default(),
        )
        .intersect(definition.capability_mode.unwrap_or_default());
    let mut isolation = request
        .isolation
        .or_else(|| {
            role.and_then(|role| role.default_isolation.as_deref())
                .or_else(|| persona.and_then(|persona| persona.default_isolation.as_deref()))
                .and_then(parse_enum)
        })
        .unwrap_or_default();
    if isolation == SubagentIsolationMode::None
        && definition.isolation == Some(SubagentIsolationMode::Worktree)
    {
        isolation = SubagentIsolationMode::Worktree;
    }
    let override_model = request
        .runtime_model
        .as_deref()
        .or(public_model)
        .or_else(|| role.and_then(|role| role.model.as_deref()))
        .or_else(|| persona.and_then(|persona| persona.model.as_deref()));
    let override_source = if request.runtime_model.is_some() || public_model.is_some() {
        0
    } else if role.is_some_and(|role| role.model.is_some()) {
        1
    } else {
        2
    };
    let mut model = context.parent_model;
    let mut embedded_variants = [None; 4];
    for (candidate, source) in [
        (override_model, override_source),
        (config.models.get(&name).map(String::as_str), 3),
        (definition.model.as_deref(), 3),
    ]
    .into_iter()
    .filter_map(|(model, source)| model.map(|model| (model, source)))
    {
        if candidate.eq_ignore_ascii_case("inherit") {
            continue;
        }
        let candidate = model_role(config, candidate, context.parent_model)?;
        if candidate == context.parent_model {
            break;
        }
        if let Some((id, variant)) = context
            .catalog
            .and_then(|catalog| configured_model(catalog, candidate))
        {
            model = id;
            embedded_variants[source] = variant;
            break;
        }
        warnings.push(format!(
            "unknown subagent model \"{candidate}\"; using fallback"
        ));
    }
    let variant = request
        .variant
        .as_deref()
        .or(embedded_variants[0])
        .or_else(|| role.and_then(|role| role.variant.as_deref()))
        .or(embedded_variants[1])
        .or_else(|| persona.and_then(|persona| persona.variant.as_deref()))
        .or(embedded_variants[2])
        .or(definition.variant.as_deref())
        .or(embedded_variants[3])
        .or(context.parent_variant);
    let variant = variant.filter(|variant| {
        if config.model_variants.get(model).is_some_and(|variants| variants.iter().any(|name| name == variant)) {
            true
        } else {
            warnings.push(format!("unknown subagent variant \"{variant}\" for model \"{model}\"; ignoring variant"));
            false
        }
    }).map(str::to_owned);
    let effort = request
        .reasoning_effort
        .as_deref()
        .or_else(|| role.and_then(|role| role.reasoning_effort.as_deref()))
        .or_else(|| persona.and_then(|persona| persona.reasoning_effort.as_deref()))
        .or(definition.effort.as_deref())
        .or(context.parent_reasoning_effort);
    let reasoning_effort = effort
        .filter(|effort| {
            matches!(
                *effort,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            )
        })
        .map(str::to_owned);
    let mut permission_mode = match context.parent_permission_mode {
        Some(
            mode @ (SubagentPermissionMode::AcceptEdits
            | SubagentPermissionMode::BypassPermissions
            | SubagentPermissionMode::Auto),
        ) => mode,
        Some(
            SubagentPermissionMode::Default
            | SubagentPermissionMode::Plan
            | SubagentPermissionMode::DontAsk,
        )
        | None => definition.permission_mode,
    };
    if context.managed_block_bypass && permission_mode == SubagentPermissionMode::BypassPermissions
    {
        permission_mode = SubagentPermissionMode::Default;
    }
    let inherited_mcp_servers: Vec<_> = context
        .parent_mcp_servers
        .iter()
        .filter(|server| definition.mcp_inheritance.includes(server))
        .cloned()
        .collect();
    let inherited_skills = if definition.inherit_skills {
        context.parent_skills.to_vec()
    } else {
        Vec::new()
    };
    let tools = child_tools(
        config,
        context,
        &definition,
        capability_mode,
        &inherited_mcp_servers,
    );
    let max_turns = definition.max_turns.or(context.parent_max_turns);
    let allowed_subagent_types = definition_allowed_types(&definition);
    Ok(ResolvedSubagentDefinition {
        definition,
        subagent_type: name,
        model: model.to_owned(),
        reasoning_effort,
        variant,
        capability_mode,
        isolation,
        permission_mode,
        max_turns,
        persona: request.persona.clone(),
        persona_instructions,
        role_name,
        role_prompt,
        tools,
        inherited_mcp_servers,
        inherited_skills,
        allowed_subagent_types,
        warnings,
    })
}

fn sentinel(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || ["null", "none", "undefined"]
            .iter()
            .any(|sentinel| value.eq_ignore_ascii_case(sentinel))
}
fn parse_enum<T: serde::de::DeserializeOwned>(value: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(value.into())).ok()
}
fn prompt_file(snapshot: &SubagentDefinitionSnapshot, path: &Path) -> Result<String, String> {
    snapshot
        .prompt_files
        .get(path)
        .cloned()
        .unwrap_or_else(|| Err(format!("prompt file {} was not captured", path.display())))
}

fn model_role<'a>(
    config: &'a SubagentRuntimeConfig,
    model: &'a str,
    parent: &'a str,
) -> Result<&'a str, SubagentResolutionError> {
    match model {
        "@smol" => Ok(config.model_roles.smol.as_deref().unwrap_or(parent)),
        "@slow" => Ok(config.model_roles.slow.as_deref().unwrap_or(parent)),
        model if model.starts_with('@') => Err(SubagentResolutionError::InvalidModel {
            model: model.into(),
        }),
        model => Ok(model),
    }
}

fn configured_model<'a, 'b>(
    catalog: &'a SubagentModelCatalog,
    reference: &'b str,
) -> Option<(&'a str, Option<&'b str>)> {
    fn matches(id: &str, reference: &str) -> bool {
        id == reference
            || id
                .split_once(':')
                .zip(reference.split_once('/'))
                .is_some_and(|(id, reference)| id == reference)
    }
    if let Some(model) = catalog
        .models
        .iter()
        .find(|model| matches(&model.id, reference))
    {
        return Some((&model.id, None));
    }
    let (base, variant) = reference.rsplit_once('/')?;
    catalog
        .models
        .iter()
        .find(|model| matches(&model.id, base))
        .map(|model| (model.id.as_str(), Some(variant)))
}
