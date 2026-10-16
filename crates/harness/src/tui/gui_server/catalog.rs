use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use harness_core::config::{self, SettingEditorKind, SettingSurface};
use harness_tui::app::ModelOption;
use harness_tui::{LiveUpdate, OperatorNoticeLevel};
use serde_json::{json, Value};

use super::super::auth_backend::{
    run_tui_auth_backend_streaming_with_deps, send_tui_auth_backend_input,
    spawn_tui_auth_backend_task, TuiAuthBackendContext,
};
use super::Server;

pub(super) async fn handle(
    server: &Server,
    method: &str,
    params: &Value,
) -> Option<Result<Value, String>> {
    Some(match method {
        "models.catalog" => models(server, params).await,
        "commands.list" => commands(server, params).await,
        "commands.expand" => expand(server, params).await,
        "mcp.resources" | "agents.list" => mentions(server, method, params).await,
        "files.index" => files(server, params).await,
        "providers.list" => providers(server).await,
        "settings.list" => settings_list(server, params).await,
        "settings.write" | "settings.reset" => settings_write(server, method, params).await,
        "auth.start" => auth_start(server, params).await,
        "auth.command" => auth_start(server, params).await,
        "auth.input" => auth_input(server, params),
        "auth.logout" => auth_logout(server, params).await,
        _ => return None,
    })
}

fn string<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("invalid_params: {key} must be a string"))
}

fn cwd(server: &Server, params: &Value) -> Result<PathBuf, String> {
    let path = match params.get("cwd") {
        None => server.config_context.discovery.current_dir.clone(),
        Some(Value::String(path)) => PathBuf::from(path),
        Some(_) => return Err("invalid_params: cwd must be a string".into()),
    };
    if !path.is_absolute() || !path.is_dir() {
        return Err("invalid_params: cwd must be an absolute workspace directory".into());
    }
    Ok(path)
}

fn model_entry(option: &ModelOption) -> Value {
    let mut entry = json!({
        "profile": option.profile, "provider": option.provider, "model": option.model,
        "label": option.display_label().or_else(|| option.model_display_label()).unwrap_or(&option.model),
    });
    for (key, value) in [
        ("providerLabel", option.provider_display_label()),
        ("modelLabel", option.model_display_label()),
        ("variant", option.variant()),
        ("variantLabel", option.variant_display_label()),
        ("reasoningEffort", option.reasoning_effort()),
        ("description", option.description()),
    ] {
        if let Some(value) = value {
            entry[key] = json!(value);
        }
    }
    if let Some(window) = option.model_limits.context_window_tokens() {
        entry["contextWindow"] = json!(window);
    }
    entry
}

async fn models(server: &Server, params: &Value) -> Result<Value, String> {
    let settings = server.settings(&cwd(server, params)?).await?;
    let metadata = &settings.launch_metadata;
    let current = metadata.model().map(|model| {
        let mut option = ModelOption::from_model_ref(
            metadata.profile(),
            &format!("{}/{}", metadata.provider(), model),
        );
        option.provider_display_label = metadata.provider_display_label().map(str::to_owned);
        option.model_display_label = metadata.model_display_label().map(str::to_owned);
        option.variant = metadata.variant().map(str::to_owned);
        option.variant_display_label = metadata.variant_display_label().map(str::to_owned);
        option.display_label = metadata.display_label().map(str::to_owned);
        option.reasoning_effort = metadata.reasoning_effort().map(str::to_owned);
        option.description = metadata.description().map(str::to_owned);
        option.model_limits = metadata.model_limits().clone();
        model_entry(&option)
    });
    Ok(
        json!({"current": current, "models": metadata.available_models().iter().map(model_entry).collect::<Vec<_>>(), "profiles": metadata.switchable_profiles(), "defaultAccessMode":if settings.yolo {"full"} else {"supervised"}}),
    )
}

async fn commands(server: &Server, params: &Value) -> Result<Value, String> {
    let settings = server.settings(&cwd(server, params)?).await?;
    let reserved = harness_tui::keybindings::reserved_slash_names();
    let project = harness_core::commands::discover(&settings.workspace_root, None, &reserved);
    let mut roots: Vec<_> = settings.workspace_root.ancestors().collect();
    if let Some(at) = roots.iter().position(|root| root.join(".git").exists()) {
        roots.truncate(at + 1);
    }
    let mut rows = harness_tui::keybindings::slash_commands().iter().map(|command| {
        json!({"name": command.id, "description": harness_tui::keybindings::slash_command_description(command.id), "scope": "builtin"})
    }).collect::<Vec<_>>();
    for command in harness_core::commands::discover(
        &settings.workspace_root,
        Some(&settings.data_dir),
        &reserved,
    )
    .commands
    {
        let project_file = roots.iter().any(|root| {
            root.join(".harness/commands")
                .join(format!("{}.md", command.name))
                .is_file()
        });
        let scope = if project_file && project.commands.iter().any(|entry| entry == &command) {
            "project"
        } else if settings
            .data_dir
            .join("commands")
            .join(format!("{}.md", command.name))
            .is_file()
        {
            "user"
        } else {
            "builtin"
        };
        let mut row = json!({"name": command.name, "description": command.description, "scope": scope, "template": command.template});
        if let Some(hint) = command.argument_hint {
            row["argumentHint"] = json!(hint);
        }
        rows.push(row);
    }
    Ok(Value::Array(rows))
}

async fn expand(server: &Server, params: &Value) -> Result<Value, String> {
    let name = string(params, "name")?;
    let args = string(params, "args")?;
    let settings = server.settings(&cwd(server, params)?).await?;
    let discovered = harness_core::commands::discover(
        &settings.workspace_root,
        Some(&settings.data_dir),
        &harness_tui::keybindings::reserved_slash_names(),
    );
    let command = discovered
        .commands
        .iter()
        .find(|command| command.name == name)
        .ok_or("not_found: prompt command not found")?;
    Ok(json!({"text": command.expand(args)}))
}

async fn mentions(server: &Server, method: &str, params: &Value) -> Result<Value, String> {
    let settings = server.settings(&cwd(server, params)?).await?;
    let metadata = &settings.launch_metadata;
    if method == "agents.list" {
        return Ok(json!(metadata
            .switchable_profiles()
            .iter()
            .filter(|name| name.as_str() != metadata.profile())
            .map(|name| json!({"name": name}))
            .collect::<Vec<_>>()));
    }
    Ok(json!(metadata
        .mcp_resources()
        .iter()
        .map(|resource| json!({
            "name": resource.name, "uri": resource.uri, "mime": resource.mime,
            "description": resource.description,
        }))
        .collect::<Vec<_>>()))
}

async fn files(server: &Server, params: &Value) -> Result<Value, String> {
    let root = cwd(server, params)?;
    let limit = match params.get("limit") {
        None => 20_000,
        Some(value) => usize::try_from(
            value
                .as_u64()
                .ok_or("invalid_params: limit must be a nonnegative integer")?,
        )
        .unwrap_or(20_000)
        .min(20_000),
    };
    tokio::task::spawn_blocking(move || {
        let mut rows = Vec::new();
        if limit == 0 {
            return Ok(Value::Array(rows));
        }
        let walk = ignore::WalkBuilder::new(&root)
            .hidden(false)
            .require_git(false)
            .follow_links(false)
            .sort_by_file_path(|a, b| a.cmp(b))
            .filter_entry(|entry| {
                let name = entry.file_name().to_str().unwrap_or_default();
                !(matches!(
                    name,
                    ".git" | "target" | "node_modules" | "dist" | "build" | "vendor"
                ) || (name == "sessions"
                    && entry
                        .path()
                        .parent()
                        .is_some_and(|parent| parent.ends_with(".harness"))))
            })
            .build();
        for entry in walk {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.depth() == 0 {
                continue;
            }
            let Some(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_file() && !kind.is_dir() {
                continue;
            }
            let path = entry
                .path()
                .strip_prefix(&root)
                .map_err(|error| error.to_string())?;
            rows.push(json!({"path": path, "dir": kind.is_dir()}));
            if rows.len() == limit {
                break;
            }
        }
        Ok(Value::Array(rows))
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn providers(server: &Server) -> Result<Value, String> {
    let settings = server
        .settings(&server.config_context.discovery.current_dir)
        .await?;
    let context = server.config_context.clone();
    let mock = server.mock;
    tokio::task::spawn_blocking(move || {
        let catalog = if mock { harness_core::provider_catalog::ProviderCatalog::from_embedded() }
            else { harness_core::provider_catalog::ProviderCatalog::from_env() }.map_err(|error| error.to_string())?;
        let store = harness_core::auth::CredentialStore::from_env();
        let connected = if mock { Vec::new() } else {
            crate::runtime_catalog::resolve_runtime_catalog(settings.config.clone(), None, None, &context,
                store.as_ref(), &|name| std::env::var(name).ok())?.connected_provider_ids
        };
        let registry = harness_core::auth::plugin::AuthPluginRegistry::with_builtins();
        Ok(Value::Array(super::super::connect_provider_options(settings.config.as_ref(), &registry, Some(&catalog))
            .into_iter().map(|provider| {
                let methods = provider.methods.iter().map(|method| {
                    let (id, label) = match method {
                        harness_core::auth::plugin::AuthMethodSpec::OAuthAuto { label, .. } => ("browser", label),
                        harness_core::auth::plugin::AuthMethodSpec::OAuthCode { label } => ("device", label),
                        harness_core::auth::plugin::AuthMethodSpec::ApiKey { label } => ("api-key", label),
                        harness_core::auth::plugin::AuthMethodSpec::Prompts { label, .. } => ("device", label),
                    };
                    json!({"id":id,"label":label})
                }).collect::<Vec<_>>();
                json!({"id": provider.id, "label": provider.label, "description": provider.description,
                    "methods": methods, "connected": connected.iter().any(|id| id == provider.id.as_str())})
            }).collect()))
    }).await.map_err(|error| error.to_string())?
}

fn kind(id: &str) -> Option<SettingEditorKind> {
    if id == "runtime.deterministic.enabled" {
        Some(SettingEditorKind::Boolean)
    } else {
        config::setting_editor_kind(id)
    }
}

fn parsed(value: &str, kind: Option<SettingEditorKind>) -> Value {
    if matches!(
        kind,
        Some(SettingEditorKind::String | SettingEditorKind::Choice(_))
    ) {
        Value::String(value.into())
    } else {
        serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.into()))
    }
}

fn tui_flag(server: &Server, root: &Path, id: &str) -> Result<(bool, PathBuf), String> {
    let context = server
        .config_context
        .discovery
        .clone()
        .with_current_dir(root.to_path_buf());
    match id {
        "confirm_before_rewind" => {
            config::rewind_confirmation_settings(server.config_path.as_deref(), &context)
        }
        "collapse_compacted_history" => {
            config::collapse_compacted_history_settings(server.config_path.as_deref(), &context)
        }
        _ => return Err("invalid_params: unsupported TUI setting".into()),
    }
    .map_err(|error| error.to_string())
}

async fn settings_list(server: &Server, params: &Value) -> Result<Value, String> {
    let root = cwd(server, params)?;
    let _ = server.settings(&root).await?;
    // Registry values describe authored configuration, even when the runtime is mocked.
    let context = server.config_context.clone().with_current_dir(root.clone());
    let loaded = config::load_config_or_defaults_with_lookup(
        server.config_path.as_deref(),
        &context,
        &|name| std::env::var(name).ok(),
    )
    .map_err(|error| error.to_string())?;
    let mut effective = serde_json::to_value(&loaded.config).map_err(|error| error.to_string())?;
    harness_core::redact::redact_in_place(
        &harness_core::redact::DefaultRedactor::default(),
        &mut effective,
    );
    let mut rows = Vec::new();
    for definition in config::settings_registry() {
        let id = definition.setting_id.as_str();
        let editor = kind(id);
        let default = definition
            .default_value
            .map(|value| parsed(value, editor))
            .unwrap_or(Value::Null);
        let pointer = match id.split_once('.') {
            Some(("permission", tail)) => format!("/permissions/{}", tail.replace('.', "/")),
            Some(("provider", tail)) => format!("/providers/{}", tail.replace('.', "/")),
            _ => format!(
                "/{}",
                match id {
                    "agent" => "agents".into(),
                    "provider" => "providers".into(),
                    _ => id.replace('.', "/"),
                }
            ),
        };
        let tui = matches!(id, "confirm_before_rewind" | "collapse_compacted_history");
        let value = if definition.is_secret() {
            Value::Null
        } else if tui {
            json!(tui_flag(server, &root, id)?.0)
        } else {
            effective
                .pointer(&pointer)
                .cloned()
                .unwrap_or_else(|| default.clone())
        };
        let copy = config::setting_copy(id);
        let mut row = json!({"id": id, "surface": definition.surface, "label": copy.map_or(id, |copy| copy.label),
            "description": copy.map_or("", |copy| copy.description),
            "kind": match editor { Some(SettingEditorKind::Boolean) => "boolean", Some(SettingEditorKind::Integer) => "integer", Some(SettingEditorKind::String) => "string", Some(SettingEditorKind::Choice(_)) => "choice", _ if tui => "boolean", _ => "json" },
            "value": value, "defaultValue": default,
            "editable": !definition.is_secret() && definition.is_editable() && (tui || editor.is_some())});
        if let Some(SettingEditorKind::Choice(choices)) = editor {
            row["choices"] = json!(choices);
        }
        rows.push(row);
    }
    Ok(Value::Array(rows))
}

async fn settings_write(server: &Server, method: &str, params: &Value) -> Result<Value, String> {
    let root = cwd(server, params)?;
    let _ = server.settings(&root).await?;
    let id = string(params, "id")?;
    let definition = config::setting_definition(id).ok_or("invalid_params: unknown setting")?;
    if definition.is_secret() || !definition.is_editable() {
        return Err("invalid_params: setting is not editable".into());
    }
    let reset = method == "settings.reset";
    if definition.surface == SettingSurface::Tui {
        let (_, path) = tui_flag(server, &root, id)?;
        let enabled = if reset {
            true
        } else {
            params
                .get("value")
                .and_then(Value::as_bool)
                .ok_or("invalid_params: value must be boolean")?
        };
        match id {
            "confirm_before_rewind" => config::write_rewind_confirmation(&path, enabled),
            "collapse_compacted_history" => {
                config::write_collapse_compacted_history(&path, enabled)
            }
            _ => return Err("invalid_params: setting has no write path".into()),
        }
        .map_err(|error| error.to_string())?;
    } else {
        let path = project_settings_path(server, &root);
        if reset {
            config::reset_project_setting_to_default(&path, id)
                .map_err(|error| error.to_string())?;
        } else {
            let value = params
                .get("value")
                .ok_or("invalid_params: value is required")?;
            let editor = kind(id).ok_or("invalid_params: setting has no write path")?;
            let valid = match editor {
                SettingEditorKind::Boolean => value.is_boolean(),
                SettingEditorKind::Integer => value.as_u64().is_some(),
                SettingEditorKind::String | SettingEditorKind::Choice(_) => value.is_string(),
                SettingEditorKind::Json => true,
            };
            if !valid {
                return Err("invalid_params: value has the wrong setting type".into());
            }
            let input = match editor {
                SettingEditorKind::String | SettingEditorKind::Choice(_) => {
                    string(params, "value")?.to_owned()
                }
                _ => value.to_string(),
            };
            config::write_project_setting_value(&path, id, &input)
                .map_err(|error| error.to_string())?;
        }
    }
    server.settings.lock().await.clear();
    Ok(Value::Null)
}

fn project_settings_path(server: &Server, root: &Path) -> PathBuf {
    if let Some(path) = &server.config_path {
        return server.config_context.discovery.current_dir.join(path);
    }
    let jsonc = root.join("harness.jsonc");
    let json = root.join("harness.json");
    if !jsonc.exists() && json.exists() {
        json
    } else {
        jsonc
    }
}

async fn auth_start(server: &Server, params: &Value) -> Result<Value, String> {
    let args: Vec<String> = serde_json::from_value(
        params
            .get("args")
            .cloned()
            .ok_or("invalid_params: args is required")?,
    )
    .map_err(|_| "invalid_params: args must be a string array")?;
    let stdin = match params.get("stdin") {
        None => None,
        Some(Value::String(value)) => Some(value.clone()),
        _ => return Err("invalid_params: stdin must be a string".into()),
    };
    let settings = server.settings(&cwd(server, params)?).await?;
    let attempt = server.auth_sequence.fetch_add(1, Ordering::Relaxed) + 1;
    let mut active = server
        .auth_attempt
        .lock()
        .map_err(|error| error.to_string())?;
    send_tui_auth_backend_input(None);
    *active = attempt;
    let (tx, rx) = harness_tui::live_update_channel();
    spawn_tui_auth_backend_task(
        Some(attempt),
        None,
        args,
        stdin,
        TuiAuthBackendContext::from_settings(&settings),
        tx,
    );
    drop(active);
    let output = server.output.clone();
    let cache = std::sync::Arc::clone(&server.settings);
    let active = std::sync::Arc::clone(&server.auth_attempt);
    tokio::task::spawn_blocking(move || {
        while let Ok(update) = rx.recv() {
            let params = match update {
                LiveUpdate::AuthBackendNotice { message, level, .. } => {
                    json!({"attempt": attempt, "kind": "notice", "level": if matches!(level, OperatorNoticeLevel::Error) { "error" } else { "info" }, "message": message})
                }
                LiveUpdate::AuthBackendResult {
                    success, message, ..
                } => {
                    cache.blocking_lock().clear();
                    if let Ok(mut current) = active.lock()
                        && *current == attempt
                    {
                        *current = 0;
                    }
                    json!({"attempt": attempt, "kind": "result", "success": success, "message": message})
                }
                _ => continue,
            };
            if output
                .send(json!({"method": "auth.update", "params": params}))
                .is_err()
            {
                break;
            }
        }
    });
    Ok(json!({"attempt": attempt}))
}

fn auth_input(server: &Server, params: &Value) -> Result<Value, String> {
    let attempt = params
        .get("attempt")
        .and_then(Value::as_u64)
        .ok_or("invalid_params: attempt must be an integer")?;
    let line = match params.get("line") {
        Some(Value::Null) => None,
        Some(Value::String(line)) => Some(line.clone()),
        _ => return Err("invalid_params: line must be a string or null".into()),
    };
    let mut active = server
        .auth_attempt
        .lock()
        .map_err(|error| error.to_string())?;
    if attempt == 0 || *active != attempt {
        return Err("not_found: authentication attempt is no longer active".into());
    }
    let cancel = line.is_none();
    send_tui_auth_backend_input(line);
    if cancel {
        *active = 0;
        let _ = server.output.send(json!({"method": "auth.update", "params": {"attempt": attempt, "kind": "result", "success": false, "message": "Authentication cancelled"}}));
    }
    Ok(Value::Null)
}

async fn auth_logout(server: &Server, params: &Value) -> Result<Value, String> {
    let provider = string(params, "provider")?.to_owned();
    if harness_core::auth::ProviderId::parse(&provider).is_none() {
        return Err("invalid_params: invalid provider".into());
    }
    let config_path = server.config_path.clone();
    let session_dir = server.session_dir.clone();
    let root = server.config_context.discovery.current_dir.clone();
    tokio::task::spawn_blocking(move || {
        let deps = harness::CliDeps::real().with_current_dir(root);
        let (message, _, success) = run_tui_auth_backend_streaming_with_deps(
            vec!["logout".into(), provider],
            config_path,
            session_dir,
            "",
            &deps,
            None,
            None,
        );
        if success {
            Ok(())
        } else {
            Err(message)
        }
    })
    .await
    .map_err(|error| error.to_string())??;
    server.settings.lock().await.clear();
    Ok(Value::Null)
}
