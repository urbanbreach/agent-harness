use super::*;
use crate::redact::Redactor;
use serde_json::{json, Value};

pub(super) fn normalize(ordered: ordered::OrderedValue) -> Result<HarnessConfig, ConfigError> {
    let mut value = ordered.json()?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| ConfigError("expected an object".into()))?;
    [
        ("provider", "providers"),
        ("agent", "agents"),
        ("permission", "permissions"),
        ("smallModel", "small_model"),
        ("hashlineEdit", "hashline_edit"),
    ]
    .into_iter()
    .try_for_each(|(old, new)| rename(object, old, new))?;
    let model = object
        .remove("model")
        .map(|v| string(&v, "model").map(canonical_model))
        .transpose()?
        .or_else(|| {
            object
                .get("agents")
                .and_then(|agents| agents.get("default"))
                .and_then(|agent| {
                    agent
                        .get("model")
                        .or_else(|| agent.get("model_ref"))
                        .or_else(|| agent.get("modelRef"))
                })
                .and_then(Value::as_str)
                .map(canonical_model)
        })
        .or_else(|| {
            object
                .get("providers")
                .and_then(Value::as_object)
                .and_then(|providers| {
                    providers.iter().find_map(|(provider, value)| {
                        value
                            .get("models")
                            .and_then(Value::as_object)
                            .and_then(|models| models.keys().next())
                            .map(|model| format!("{provider}:{model}"))
                    })
                })
        });
    if let Some(permission) = object.get_mut("permissions") {
        *permission = permissions(std::mem::take(permission), false, ordered.get("permission"))?;
    }
    if let Some(mcp) = object.remove("mcp") {
        let integrations = object.entry("integrations").or_insert_with(|| json!({}));
        let map = integrations
            .as_object_mut()
            .ok_or_else(|| ConfigError("integrations must be an object".into()))?;
        if map.contains_key("mcp") {
            return Err(ConfigError(
                "mcp and integrations.mcp cannot both be set".into(),
            ));
        }
        map.insert("mcp".into(), json!({"servers":mcp}));
    }
    if let Some(enabled) = object.get("formatter").and_then(Value::as_bool) {
        object.insert("formatter".into(), json!({"enabled":enabled}));
    }
    if let Some(formatters) = object.get_mut("formatter").and_then(Value::as_object_mut) {
        if let Some(value) = formatters.remove("uvformat") {
            formatters.entry("uv").or_insert(value);
        }
        if let Some(languages) = formatters.remove("languages") {
            let languages = languages
                .as_object()
                .ok_or_else(|| ConfigError("formatter.languages must be an object".into()))?;
            for (extension, formatter) in languages {
                let mut formatter = formatter
                    .as_object()
                    .ok_or_else(|| ConfigError("language formatter must be an object".into()))?
                    .clone();
                formatter.insert("extensions".into(), json!([extension]));
                formatters.insert(format!("_lang_{extension}"), Value::Object(formatter));
            }
        }
    }
    if object.get("lsp") == Some(&Value::Bool(false)) {
        object.insert("lsp".into(), json!({"disabled":true}));
    }
    let agents = object
        .entry("agents")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| ConfigError("agent must be an object".into()))?;
    for (name, profile) in agents.iter_mut() {
        let profile = profile
            .as_object_mut()
            .ok_or_else(|| ConfigError(format!("agent {name} must be an object")))?;
        for alias in ["model", "modelRef"] {
            rename(profile, alias, "model_ref")?;
        }
        rename(profile, "permission", "permissions")?;
        for (old, new) in [
            ("systemPrompt", "system_prompt"),
            ("prompt", "system_prompt"),
            ("topP", "top_p"),
            ("maxIters", "max_iters"),
            ("maxSteps", "max_iters"),
            ("steps", "max_iters"),
            ("toolFailureMode", "tool_failure_mode"),
        ] {
            rename(profile, old, new)?;
        }
        profile.insert(
            "model_ref_explicit".into(),
            Value::Bool(profile.contains_key("model_ref")),
        );
        if let Some(model) = profile.get_mut("model_ref") {
            *model = Value::String(canonical_model(string(model, "agent model")?));
        }
        if let Some(permission) = profile.get_mut("permissions") {
            let order = ordered
                .get("agent")
                .and_then(|agents| agents.get(name))
                .and_then(|agent| agent.get("permission").or_else(|| agent.get("permissions")));
            *permission = permissions(std::mem::take(permission), true, order)?;
        }
        if let Some(Value::Object(tools)) = profile.get("tools") {
            let mut enabled = Vec::new();
            for (name, on) in tools {
                if on
                    .as_bool()
                    .ok_or_else(|| ConfigError("tool switches must be booleans".into()))?
                {
                    enabled.push(Value::String(name.clone()));
                }
            }
            profile.insert("tools".into(), Value::Array(enabled));
        }
        if let Some(tools) = profile.get_mut("tools").and_then(Value::as_array_mut) {
            canonicalize_subagent_tools(tools);
        }
    }
    let mut profiles = shipped_profiles(model.as_deref().unwrap_or("mock:default"));
    for (name, value) in std::mem::take(agents) {
        let mut base =
            serde_json::to_value(profiles.remove(&name).unwrap_or_else(|| ProfileConfig {
                model_ref: model.clone().unwrap_or_else(|| "mock:default".into()),
                ..Default::default()
            }))
            .map_err(parse_error)?;
        merge(&mut base, value);
        profiles.insert(name, serde_json::from_value(base).map_err(parse_error)?);
    }
    *agents = profiles
        .into_iter()
        .map(|(name, value)| {
            serde_json::to_value(value)
                .map(|v| (name, v))
                .map_err(parse_error)
        })
        .collect::<Result<_, _>>()?;
    let mut config: HarnessConfig = serde_json::from_value(value).map_err(parse_error)?;
    for provider in config.providers.values_mut() {
        provider.normalize()?;
    }
    config.background_task = config.runtime.background_tasks.clone();
    config
        .paths
        .session_dir
        .clone_from(&config.runtime.session_dir);
    config.deterministic = config.runtime.deterministic.clone();
    config.validate()?;
    Ok(config)
}

fn canonicalize_subagent_tools(tools: &mut [Value]) {
    for tool in tools {
        let canonical = match tool.as_str() {
            Some("task") => Some("spawn_subagent"),
            Some("get_task_output") => Some("get_command_or_subagent_output"),
            Some("wait_tasks") => Some("wait_commands_or_subagents"),
            Some("kill_task") => Some("kill_command_or_subagent"),
            _ => None,
        };
        if let Some(canonical) = canonical {
            *tool = Value::String(canonical.into());
        }
    }
}

pub(super) fn parse_error(error: impl std::fmt::Display) -> ConfigError {
    ConfigError(crate::redact::DefaultRedactor::default().redact_text(&error.to_string()))
}

pub(super) fn merge(target: &mut Value, incoming: Value) {
    if let (Value::Object(target), Value::Object(incoming)) = (&mut *target, &incoming) {
        for (key, value) in incoming {
            merge(
                target.entry(key.clone()).or_insert(Value::Null),
                value.clone(),
            );
        }
    } else {
        *target = incoming;
    }
}

pub(super) fn rename(
    object: &mut serde_json::Map<String, Value>,
    old: &str,
    new: &str,
) -> Result<(), ConfigError> {
    if let Some(value) = object.remove(old) {
        if object.contains_key(new) {
            return Err(ConfigError(format!("use only one of {old} and {new}")));
        }
        object.insert(new.into(), value);
    }
    Ok(())
}
fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str, ConfigError> {
    value
        .as_str()
        .ok_or_else(|| ConfigError(format!("{name} must be a string")))
}
fn canonical_model(model: &str) -> String {
    if model.contains(':') {
        model.into()
    } else {
        model.replacen('/', ":", 1)
    }
}

fn permissions(
    value: Value,
    profile: bool,
    order: Option<&ordered::OrderedValue>,
) -> Result<Value, ConfigError> {
    if value.get("defaults").is_some() || value.get("rules").is_some() {
        return Ok(value);
    }
    let mut defaults = if profile {
        json!({})
    } else {
        serde_json::to_value(PermissionDefaultsConfig::default()).map_err(parse_error)?
    };
    let mut rules = if profile {
        PermissionRuleSet::default()
    } else {
        default_permission_rule_set_with_read_env()
    };
    let fields = [
        "edit",
        "shell",
        "network",
        "question",
        "task",
        "webfetch",
        "websearch",
        "codesearch",
        "lsp",
        "read",
        "external_directory",
        "doom_loop",
    ];
    let object = if value.is_string() {
        json!({"*":value})
    } else {
        value
    };
    let object = object
        .as_object()
        .ok_or_else(|| ConfigError("permissions must be a mode or object".into()))?;
    if let Some(global) = object.get("*") {
        let mode: PermissionMode = serde_json::from_value(global.clone()).map_err(parse_error)?;
        for key in fields {
            if profile
                || mode != PermissionMode::Allow
                || !matches!(key, "question" | "external_directory" | "doom_loop")
            {
                defaults[key] = global.clone();
            }
        }
    }
    let mut shell_allowlist = None;
    for (name, value) in object {
        let order = order
            .and_then(|order| order.get(name))
            .and_then(ordered::OrderedValue::object);
        let name = if name == "bash" {
            "shell"
        } else {
            name.as_str()
        };
        if name == "*" {
            continue;
        }
        if matches!(name, "shell_allowlist" | "shellAllowlist") && !profile {
            shell_allowlist = Some(value.clone());
            continue;
        }
        if !(fields.contains(&name) || profile && name == "todowrite") {
            return Err(ConfigError(format!("unknown permission: {name}")));
        }
        if let Some(patterns) = value.as_object() {
            let target = match name {
                "shell" => &mut rules.shell,
                "edit" => &mut rules.edit,
                "task" => &mut rules.task,
                "read" => &mut rules.read,
                "external_directory" => &mut rules.external_directory,
                _ => {
                    return Err(ConfigError(format!(
                        "permission {name} requires a scalar mode"
                    )))
                }
            };
            target.clear();
            let mut patterns: Vec<_> = patterns.iter().collect();
            if let Some(order) = order {
                patterns.sort_by_key(|(key, _)| order.get_index_of(*key).unwrap_or(usize::MAX));
            }
            for (pattern, value) in patterns {
                let mode = serde_json::from_value(value.clone()).map_err(parse_error)?;
                target.push(PermissionSelectorRule {
                    selector: if pattern == "*" {
                        PermissionSelector::CatchAll
                    } else {
                        PermissionSelector::Glob(pattern.clone())
                    },
                    mode,
                });
            }
        } else {
            let _: PermissionMode = serde_json::from_value(value.clone()).map_err(parse_error)?;
            defaults[name] = value.clone();
        }
    }
    let mut result = if profile {
        defaults
    } else {
        json!({"defaults":defaults})
    };
    result["rules"] = serde_json::to_value(rules).map_err(parse_error)?;
    if let Some(global) = object.get("*") {
        result["*"] = global.clone();
    }
    if let Some(allowlist) = shell_allowlist {
        result["shell_allowlist"] = allowlist;
    }
    Ok(result)
}

fn shipped_profiles(model: &str) -> BTreeMap<String, ProfileConfig> {
    const READ: &str = "read glob grep list ast_grep_search webfetch websearch session_list session_read session_search session_info batch bash lsp skill";
    ["default", "explore", "general", "librarian"]
        .into_iter()
        .map(|name| {
            let mut tools: Vec<String> = READ.split_whitespace().map(str::to_owned).collect();
            if name != "explore" {
                tools.push("codesearch".into());
            }
            if matches!(name, "default" | "general") {
                tools.extend(["edit", "write", "apply_patch"].map(str::to_owned));
            }
            if name == "default" {
                tools.extend(
                    [
                        "todowrite",
                        "todoread",
                        "question",
                        "spawn_subagent",
                        "get_command_or_subagent_output",
                        "wait_commands_or_subagents",
                        "kill_command_or_subagent",
                        "send_subagent_message",
                    ]
                    .map(str::to_owned),
                );
            }
            if name == "general" {
                tools.retain(|tool| tool != "skill");
            }
            (
                name.into(),
                ProfileConfig {
                    model_ref: model.into(),
                    description: match name {
                        "explore" => "Read-only codebase exploration.",
                        "librarian" => "Documentation and external research.",
                        _ => "General-purpose implementation and research.",
                    }
                    .into(),
                    mode: if name == "default" {
                        AgentMode::Primary
                    } else {
                        AgentMode::Subagent
                    },
                    tools,
                    permissions: (name != "default").then(|| ProfilePermissions {
                        edit: Some(if name == "general" {
                            PermissionMode::Allow
                        } else {
                            PermissionMode::Deny
                        }),
                        task: Some(PermissionMode::Deny),
                        question: Some(PermissionMode::Deny),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
        })
        .collect()
}
