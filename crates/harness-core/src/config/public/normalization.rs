// allow: SIZE_OK — config normalization functions (alias canonicalization + section transforms)
use super::*;
use serde_json::{json, Value};

fn canonicalize_object_aliases(
    object: &mut serde_json::Map<String, serde_json::Value>,
    aliases: &[(&str, &str)],
) {
    for (alias, canonical) in aliases {
        if let Some(value) = object.remove(*alias) {
            match object.get_mut(*canonical) {
                Some(existing) => merge_config_value(existing, value),
                None => {
                    object.insert((*canonical).to_string(), value);
                }
            }
        }
    }
}

pub(super) fn canonicalize_runtime_aliases(runtime: &mut serde_json::Value) {
    let Some(runtime_object) = runtime.as_object_mut() else {
        return;
    };
    let contract = public_config_contract();

    canonicalize_object_aliases(
        runtime_object,
        &contract
            .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimeRoot)
            .map(|alias| (alias.alias, alias.canonical))
            .collect::<Vec<_>>(),
    );

    if let Some(background_tasks) = runtime_object
        .get_mut("background_tasks")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            background_tasks,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimeBackgroundTasks)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }

    if let Some(permissions) = runtime_object
        .get_mut("permissions")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            permissions,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimePermissions)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }

    if let Some(prompt) = runtime_object
        .get_mut("prompt")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            prompt,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimePrompt)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }

    if let Some(compaction) = runtime_object
        .get_mut("compaction")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            compaction,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimeCompaction)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }
}

pub(super) fn translate_public_permission_value(
    value: serde_json::Value,
) -> Result<serde_json::Value, ConfigError> {
    if value
        .as_object()
        .map(|object| object.contains_key("defaults"))
        .unwrap_or(false)
    {
        return Ok(value);
    }

    let parsed: PublicPermissionValue =
        serde_json::from_value(value).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    let fallback = default_internal_permissions_config();

    let parsed = match parsed {
        PublicPermissionValue::Config(parsed) => parsed,
        PublicPermissionValue::Mode(mode) => {
            // Scalar allow is OC-like allow-with-safety-exceptions, not YOLO on
            // external_directory / doom_loop / question. Scalar ask/deny still
            // paints every kind with that mode.
            let defaults = match mode {
                PermissionMode::Allow => PermissionDefaultsConfig {
                    edit: PermissionMode::Allow,
                    shell: PermissionMode::Allow,
                    network: PermissionMode::Allow,
                    question: Some(PermissionMode::Deny),
                    task: Some(PermissionMode::Allow),
                    webfetch: Some(PermissionMode::Allow),
                    websearch: Some(PermissionMode::Allow),
                    codesearch: Some(PermissionMode::Allow),
                    lsp: Some(PermissionMode::Allow),
                    read: Some(PermissionMode::Allow),
                    external_directory: Some(PermissionMode::Ask),
                    doom_loop: Some(PermissionMode::Ask),
                },
                PermissionMode::Ask | PermissionMode::Deny => PermissionDefaultsConfig {
                    edit: mode.clone(),
                    shell: mode.clone(),
                    network: mode.clone(),
                    question: Some(mode.clone()),
                    task: Some(mode.clone()),
                    webfetch: Some(mode.clone()),
                    websearch: Some(mode.clone()),
                    codesearch: Some(mode.clone()),
                    lsp: Some(mode.clone()),
                    read: Some(mode.clone()),
                    external_directory: Some(mode.clone()),
                    doom_loop: Some(mode),
                },
            };
            return serde_json::to_value(PermissionsConfig {
                defaults,
                fallback: None,
                rules: crate::config::default_permission_rule_set_with_read_env(),
                shell_allowlist: fallback.shell_allowlist,
            })
            .map_err(|err| ConfigError::ParseJson5(err.to_string()));
        }
    };

    let global = parsed.fallback.clone();
    let edit = public_rule_mode(&parsed.edit)
        .or_else(|| global.clone())
        .unwrap_or(fallback.defaults.edit);
    let shell = public_rule_mode(&parsed.bash)
        .or_else(|| global.clone())
        .unwrap_or(fallback.defaults.shell);
    let task = public_rule_mode(&parsed.task)
        .or_else(|| global.clone())
        .or(fallback.defaults.task);
    let read = public_rule_mode(&parsed.read)
        .or_else(|| global.clone())
        .or(fallback.defaults.read);
    let external_directory = public_rule_mode(&parsed.external_directory)
        .or_else(|| global.clone())
        .or(fallback.defaults.external_directory);
    let edit_rules = public_selector_rules("edit", parsed.edit)?;
    let shell_rules = public_selector_rules("bash", parsed.bash)?;
    let task_rules = public_selector_rules("task", parsed.task)?;
    let read_rules = public_selector_rules("read", parsed.read)?;
    let external_directory_rules =
        public_selector_rules("external_directory", parsed.external_directory)?;

    serde_json::to_value(PermissionsConfig {
        defaults: PermissionDefaultsConfig {
            edit,
            shell,
            network: parsed
                .network
                .or_else(|| global.clone())
                .unwrap_or(fallback.defaults.network),
            question: parsed
                .question
                .or_else(|| global.clone())
                .or(fallback.defaults.question),
            task,
            webfetch: parsed
                .webfetch
                .or_else(|| global.clone())
                .or(fallback.defaults.webfetch),
            websearch: parsed
                .websearch
                .or_else(|| global.clone())
                .or(fallback.defaults.websearch),
            codesearch: parsed
                .codesearch
                .or_else(|| global.clone())
                .or(fallback.defaults.codesearch),
            lsp: parsed
                .lsp
                .or_else(|| global.clone())
                .or(fallback.defaults.lsp),
            read,
            external_directory,
            doom_loop: parsed
                .doom_loop
                .or_else(|| global.clone())
                .or(fallback.defaults.doom_loop),
        },
        fallback: parsed.fallback,
        rules: PermissionRuleSet {
            shell: shell_rules,
            edit: edit_rules,
            task: task_rules,
            read: read_rules,
            external_directory: external_directory_rules,
        },
        shell_allowlist: parsed.shell_allowlist.unwrap_or(fallback.shell_allowlist),
    })
    .map_err(|err| ConfigError::ParseJson5(err.to_string()))
}

pub(super) fn normalize_public_mcp_servers(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(servers) = value else {
        return value;
    };

    let mut normalized_servers = serde_json::Map::new();
    for (name, server) in servers {
        let mut normalized = server;
        let Some(server_object) = normalized.as_object_mut() else {
            normalized_servers.insert(name, normalized);
            continue;
        };

        if server_object.len() == 1
            && matches!(
                server_object.get("enabled"),
                Some(serde_json::Value::Bool(false))
            )
        {
            continue;
        }

        if !server_object.contains_key("transport") {
            if let Some(kind) = server_object.remove("type") {
                let transport = match kind.as_str() {
                    Some("local") => "stdio",
                    Some("remote") => "http",
                    Some(other) => other,
                    None => "",
                };
                if !transport.is_empty() {
                    server_object.insert(
                        "transport".to_string(),
                        serde_json::Value::String(transport.to_string()),
                    );
                }
            }
        }

        normalized_servers.insert(name, normalized);
    }

    serde_json::Value::Object(normalized_servers)
}

pub(crate) fn translate_public_formatter_config(
    value: Option<&serde_json::Value>,
) -> Result<FormatterConfig, ConfigError> {
    match value {
        None => Ok(FormatterConfig::default()),
        Some(serde_json::Value::Bool(false)) => Ok(FormatterConfig {
            enabled: false,
            ..FormatterConfig::default()
        }),
        Some(serde_json::Value::Bool(true)) => Ok(FormatterConfig::default()),
        Some(value) => {
            let serde_json::Value::Object(mut object) = value.clone() else {
                return Err(ConfigError::ParseJson5(
                    "formatter must be a boolean or an object".to_string(),
                ));
            };
            if let Some(languages) = object.remove("languages") {
                let languages = languages.as_object().ok_or_else(|| {
                    ConfigError::ParseJson5("formatter.languages must be an object".to_string())
                })?;
                for (extension, language_value) in languages {
                    let mut override_object = serde_json::Map::new();
                    override_object.insert(
                        "extensions".to_string(),
                        serde_json::json!([format!(".{extension}")]),
                    );
                    if let Some(command) = language_value.get("command") {
                        override_object.insert("command".to_string(), command.clone());
                    }
                    object.insert(
                        format!("_lang_{extension}"),
                        serde_json::Value::Object(override_object),
                    );
                }
            }
            // Backward-compatible alias: older harness configs used "uvformat"
            // for the uv Python formatter. Harness uses "uv", which is now canonical.
            if let Some(value) = object.remove("uvformat") {
                object.entry("uv".to_string()).or_insert(value);
            }
            serde_json::from_value(serde_json::Value::Object(object))
                .map_err(|err| ConfigError::ParseJson5(err.to_string()))
        }
    }
}

pub(super) fn normalize_public_lsp_config(value: &serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Bool(false) => Some(serde_json::json!({ "disabled": true })),
        serde_json::Value::Bool(true) | serde_json::Value::Null => None,
        serde_json::Value::Object(object) if object.contains_key("servers") => Some(value.clone()),
        serde_json::Value::Object(object) => {
            Some(serde_json::json!({ "servers": serde_json::Value::Object(object.clone()) }))
        }
        _ => None,
    }
}

pub(super) fn normalize_public_skills_config(value: &serde_json::Value) -> serde_json::Value {
    let mut overlay = value.clone();
    let Some(object) = overlay.as_object_mut() else {
        return overlay;
    };
    object.remove("urls");
    canonicalize_skill_alias(object, "projectRoots", "project_roots");
    canonicalize_skill_alias(object, "paths", "project_roots");
    canonicalize_skill_alias(object, "globalRoots", "global_roots");
    canonicalize_skill_alias(object, "disabledIds", "disabled");
    canonicalize_skill_alias(object, "walkToGitRoot", "walk_to_git_root");

    let mut normalized =
        serde_json::to_value(SkillsConfig::default()).unwrap_or_else(|_| serde_json::json!({}));
    merge_config_value(&mut normalized, overlay);
    normalized
}

fn canonicalize_skill_alias(
    object: &mut serde_json::Map<String, serde_json::Value>,
    alias: &str,
    canonical: &str,
) {
    let Some(value) = object.remove(alias) else {
        return;
    };
    object.entry(canonical.to_string()).or_insert(value);
}
