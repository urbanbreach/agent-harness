use crate::{CliDeps, CliIo};
use harness_core::{
    config::*,
    redact::{redact_in_place, Redactor},
};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(clap::Args)]
pub(crate) struct ConfigCommand {
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    Validate,
    Show {
        #[arg(long, required = true)]
        effective: bool,
    },
    Sources,
    Explain {
        path: String,
    },
    Settings,
}

pub(crate) fn schema(tui: bool, io: &mut CliIo<'_>) -> Result<(), String> {
    if tui {
        io.stdout
            .write_all(include_bytes!("../../../configs/tui.json"))
            .map_err(|e| e.to_string())
    } else {
        crate::inspect::print_json(io, &config_json_schema())
    }
}

pub(crate) fn execute(
    command: ConfigCommand,
    path: Option<&Path>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    if matches!(command.action, Action::Settings) {
        return crate::inspect::print_json(
            io,
            &json!({"settings":settings_registry(),
            "summary":summarize_settings_registry()}),
        );
    }
    let context = deps.config_load_context();
    let mut loaded =
        load_config_or_defaults_with_lookup(path, &context, &|name| deps.env_var_value(name))
            .map_err(|e| e.to_string())?;
    loaded
        .config
        .apply_session_dir_override(session_dir.clone());
    let redactor = crate::inspect::redactor(&loaded.config, deps)?;
    let mut layers: Vec<_> = loaded
        .paths
        .iter()
        .map(|p| (p.display().to_string(), "runtime"))
        .collect();
    if context.runtime_content.is_some() {
        layers.push(("HARNESS_CONFIG_CONTENT".into(), "environment"));
    }
    layers.extend(
        resolve_tui_config_layer_paths_with_context(&context.discovery)
            .into_iter()
            .map(|p| (p.display().to_string(), "tui")),
    );
    if session_dir.is_some() {
        layers.push(("--session-dir".into(), "command_line"));
    }
    if matches!(command.action, Action::Validate) {
        if layers.is_empty() {
            return writeln!(
                io.stdout,
                "config valid: no configuration files; using built-in defaults"
            )
            .map_err(|e| e.to_string());
        }
        return writeln!(
            io.stdout,
            "config valid: {}",
            redactor.redact_text(
                &layers
                    .iter()
                    .map(|(path, _)| path.as_str())
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
        )
        .map_err(|e| e.to_string());
    }
    let primary = loaded.primary_path();
    let effective = serde_json::to_value(&loaded.config).map_err(|e| e.to_string())?;
    let mut report = match command.action {
        Action::Show { .. } => {
            json!({"schema_version":"harness-config-effective-v1", "redacted":true,
            "layers":layers.iter().map(|(p,_)| p).collect::<Vec<_>>(), "primary_path":primary,"effective":effective})
        }
        Action::Sources => {
            let mut report = json!({"schema_version":"harness-config-sources-v1","primary_path":primary,
                "searched":config_search_paths(&context.discovery, false),
                "layer_count":layers.len(),"layers":layers.iter().enumerate().map(|(i,(path,kind))|
                    json!({"order":i+1,"path":path,"kind":kind})).collect::<Vec<_>>() });
            if loaded.paths.is_empty() && context.runtime_content.is_none() {
                report["note"] = json!("No configuration files found. Harness uses built-in defaults and providers connected through /login, `harness auth login`, or provider API key environment variables.");
            }
            report
        }
        Action::Explain { path } => explain(
            &path,
            &layers,
            &context,
            &effective,
            session_dir,
            &redactor,
            deps,
        )?,
        _ => return Ok(()),
    };
    redact_in_place(&redactor, &mut report);
    crate::inspect::print_json(io, &report)
}

fn explain(
    path: &str,
    layers: &[(String, &str)],
    context: &ConfigLoadContext,
    effective: &Value,
    session_dir: Option<PathBuf>,
    redactor: &dyn Redactor,
    deps: &CliDeps,
) -> Result<Value, String> {
    if path.trim().is_empty() {
        return Err("config path cannot be empty".into());
    }
    let mut merged = json!({});
    let mut rows = Vec::new();
    let mut source = None;
    for (label, kind) in layers {
        let mut value = if *kind == "command_line" {
            json!({"runtime":{"session_dir":session_dir}})
        } else {
            let raw = if *kind == "environment" {
                context.runtime_content.clone().unwrap_or_default()
            } else {
                let mut raw = String::new();
                std::fs::File::open(label)
                    .and_then(|file| file.take(4 * 1024 * 1024 + 1).read_to_string(&mut raw))
                    .map_err(|e| e.to_string())?;
                raw
            };
            if raw.len() > 4 * 1024 * 1024 {
                return Err("configuration input exceeds 4 MiB".into());
            }
            if *kind == "tui" {
                json5::from_str(&raw).map_err(|e| e.to_string())?
            } else {
                let base = if *kind == "environment" {
                    context.discovery.current_dir.as_path()
                } else {
                    Path::new(label).parent().unwrap_or(Path::new("."))
                };
                config_layer_value(&raw, base, &|name| deps.env_var_value(name))
                    .map_err(|e| e.to_string())?
            }
        };
        redact_in_place(redactor, &mut value);
        let selected = at_path(&value, path);
        if selected.is_some() {
            source = Some(label.as_str());
        }
        rows.push(json!({"path":label,"defines_path":selected.is_some(),"value":selected}));
        merge(&mut merged, value);
    }
    let mut effective = effective.clone();
    redact_in_place(redactor, &mut effective);
    let value = at_path(&effective, path)
        .filter(|v| !v.is_null())
        .or_else(|| at_path(&merged, path));
    let primary = layers
        .iter()
        .rfind(|(_, kind)| *kind == "runtime")
        .map(|(path, _)| path);
    Ok(
        json!({"schema_version":"harness-config-explain-v1","path":path,"redacted":true,
        "found":value.is_some(),"effective":value,"source_path":source,"primary_path":primary,"layers":rows,
        "source_note":if source.is_none() && value.is_some() {"Built-in default."} else {"Last input layer defining this path; fields in merged objects may come from earlier layers."}}),
    )
}

fn at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.starts_with('/') {
        return value.pointer(path);
    }
    if let Some(found) = value.get(path) {
        return Some(found);
    }
    let (key, rest) = path
        .split_once('.')
        .map_or((path, None), |(k, r)| (k, Some(r)));
    let current = if let Some(fields) = value.as_object() {
        fields.get(key).or_else(|| {
            fields
                .iter()
                .find(|(candidate, _)| canonical(candidate) == canonical(key))
                .map(|(_, v)| v)
        })?
    } else {
        value.get(key.parse::<usize>().ok()?)?
    };
    rest.map_or(Some(current), |rest| at_path(current, rest))
}
fn canonical(key: &str) -> String {
    let key = match key {
        "providers" => "provider",
        "agents" => "agent",
        "permissions" => "permission",
        "model_ref" | "modelRef" => "model",
        _ => key,
    };
    key.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}
fn merge(target: &mut Value, incoming: Value) {
    if let (Some(target), Some(incoming)) = (target.as_object_mut(), incoming.as_object()) {
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
