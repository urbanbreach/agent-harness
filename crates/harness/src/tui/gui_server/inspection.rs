use std::io::Read;
use std::path::{Component, Path, PathBuf};

use harness_core::memory::DurableMemoryStore;
use harness_core::storage_paths::ProjectPaths;
use harness_core::store::{create_private_dir, open_private_file};
use serde::Deserialize;
use serde_json::{json, Value};

#[path = "foreign_import.rs"]
mod foreign_import;

pub(super) async fn dispatch(
    server: &super::Server,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    if method == "projects.scratch" {
        let prompt = super::required(params, "prompt")?.to_owned();
        let home = server
            .config_context
            .discovery
            .data_dir
            .clone()
            .ok_or("unavailable: Harness home is not configured")?;
        return blocking(move || scratch(&home, &prompt)).await;
    }
    if matches!(method, "sessions.exportMarkdown" | "sessions.trace") {
        let run = absolute(params, "runDir")?;
        let dest = absolute(params, "dest")?;
        let cwd = server.config_context.discovery.current_dir.clone();
        let settings = server.settings(&cwd).await?;
        let deps = dependencies(server, &cwd, &settings.data_dir);
        let config = server.config_path.clone();
        let root = settings.session_dir;
        let trace = method == "sessions.trace";
        return blocking(move || export(trace, &run, &dest, config.as_deref(), &root, deps)).await;
    }
    let cwd = absolute(params, "cwd")?;
    let settings = server.settings(&cwd).await?;
    let runtime = ProjectPaths::new(&settings.data_dir, &cwd)
        .map_err(|error| error.to_string())?
        .runtime_dir();
    let deps = dependencies(server, &cwd, &settings.data_dir);
    let config = server.config_path.clone();
    let root = settings.session_dir;
    let method = method.to_owned();
    let params = params.clone();
    blocking(move || match method.as_str() {
        "sessions.discoverForeign" | "sessions.importForeign" => {
            let source = absolute(&params, "from")?;
            if method == "sessions.discoverForeign" {
                foreign_import::discover(&source)
            } else {
                Ok(json!(foreign_import::import(&source, &root)?))
            }
        }
        "worktrees.list"
        | "worktrees.remove"
        | "diagnostics.rebuildSessionIndex"
        | "diagnostics.crashScan"
        | "diagnostics.refreshModelCatalog"
        | "providers.protocols"
        | "config.validate"
        | "config.explain" => {
            let command = maintenance_command(&method, &params)?;
            let bytes = cli_output(command, config.as_deref(), &root, deps)?;
            if matches!(
                method.as_str(),
                "config.validate" | "diagnostics.refreshModelCatalog"
            ) {
                let message = String::from_utf8(bytes).map_err(|error| error.to_string())?;
                Ok(if method == "config.validate" {
                    json!({"valid":true,"message":message.trim()})
                } else {
                    json!({"message":message.trim()})
                })
            } else {
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())
            }
        }
        "sessions.tree" => {
            let bytes = cli_output(
                vec!["sessions".into(), "--json".into(), "tree".into()],
                config.as_deref(),
                &root,
                deps,
            )?;
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())
        }
        "history.prompts" => prompts(&harness_tui::app::prompt_history_path_for_session_dir(
            &root,
        )),
        "memory.list" | "memory.get" | "memory.delete" | "memory.search" => {
            memory(&runtime, &method, &params)
        }
        "plans.list" | "plans.read" => plans(&runtime, &method, &params),
        _ => Err(format!("method_not_found: {method}")),
    })
    .await
}

fn maintenance_command(method: &str, params: &Value) -> Result<Vec<std::ffi::OsString>, String> {
    let flag = |name: &str| -> Result<bool, String> {
        match params.get(name) {
            None => Ok(false),
            Some(Value::Bool(value)) => Ok(*value),
            Some(_) => Err(format!("invalid_params: {name} must be a boolean")),
        }
    };
    let mut command: Vec<std::ffi::OsString> = match method {
        "worktrees.list" => vec!["worktree".into(), "list".into()],
        "worktrees.remove" => vec!["worktree".into(), "remove".into()],
        "diagnostics.rebuildSessionIndex" => {
            vec!["sessions".into(), "--json".into(), "rebuild-index".into()]
        }
        "diagnostics.crashScan" => vec!["sessions".into(), "--json".into(), "crash-scan".into()],
        "diagnostics.refreshModelCatalog" => vec!["models".into(), "refresh".into()],
        "providers.protocols" => vec!["providers".into(), "protocols".into()],
        "config.validate" => vec!["config".into(), "validate".into()],
        "config.explain" => vec![
            "config".into(),
            "explain".into(),
            "--".into(),
            super::required(params, "path")?.into(),
        ],
        _ => return Err(format!("method_not_found: {method}")),
    };
    if method == "worktrees.list" && flag("all")? {
        command.push("--all".into());
    }
    if method == "worktrees.remove" {
        if flag("force")? {
            command.push("--force".into());
        }
        if flag("keepBranch")? {
            command.push("--keep-branch".into());
        }
        command.extend(["--".into(), super::required(params, "slug")?.into()]);
    }
    Ok(command)
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| error.to_string())?
}

fn absolute(params: &Value, key: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(super::required(params, key)?);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(format!(
            "invalid_params: {key} must be absolute without parent traversal"
        ));
    }
    Ok(path)
}

fn dependencies(server: &super::Server, cwd: &Path, home: &Path) -> harness::CliDeps {
    let mut deps = harness::CliDeps::real()
        .with_current_dir(cwd.to_owned())
        .with_env("HARNESS_HOME", home.to_string_lossy());
    if server.mock {
        deps = deps.with_env("HARNESS_DISABLE_MODELS_FETCH", "1");
    }
    for (name, value) in [
        (
            "HARNESS_CONFIG",
            server
                .config_context
                .discovery
                .runtime_config_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
        ),
        (
            "HARNESS_CONFIG_CONTENT",
            server.config_context.runtime_content.clone(),
        ),
    ] {
        deps = match value {
            Some(value) => deps.with_env(name, value),
            None => deps.without_env(name),
        };
    }
    deps
}

fn cli_output(
    command: Vec<std::ffi::OsString>,
    config: Option<&Path>,
    root: &Path,
    deps: harness::CliDeps,
) -> Result<Vec<u8>, String> {
    let mut args = vec![
        "harness".into(),
        "--session-dir".into(),
        root.as_os_str().to_owned(),
    ];
    if let Some(config) = config {
        args.extend(["--config".into(), config.as_os_str().to_owned()]);
    }
    args.extend(command);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let outcome = harness::run(
        args,
        &mut harness::CliIo::new(&mut std::io::empty(), &mut stdout, &mut stderr),
        deps,
    );
    if outcome.code != 0 {
        return Err(format!(
            "CLI exited with status {}: {}",
            outcome.code,
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    Ok(stdout)
}

fn export(
    trace: bool,
    run: &Path,
    dest: &Path,
    config: Option<&Path>,
    root: &Path,
    deps: harness::CliDeps,
) -> Result<Value, String> {
    // Both native CLI exporters validate the journal, redact secrets, and reject session-storage destinations.
    let mut command = vec![
        if trace {
            "trace".into()
        } else {
            "export".into()
        },
        run.as_os_str().to_owned(),
        "--output".into(),
        dest.as_os_str().to_owned(),
    ];
    if trace {
        command.push("--json".into());
    }
    let bytes = cli_output(command, config, root, deps)?;
    if trace {
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    } else {
        Ok(
            json!({"dest":dest,"bytes":std::fs::metadata(dest).map_err(|error| error.to_string())?.len()}),
        )
    }
}

fn memory(runtime: &Path, method: &str, params: &Value) -> Result<Value, String> {
    let store = DurableMemoryStore::for_runtime(runtime);
    match method {
        "memory.get" => Ok(json!(store
            .get_scoped(super::required(params, "key")?)
            .map_err(|error| error.to_string())?
            .ok_or("not_found: memory key not found")?)),
        "memory.delete" => Ok(
            json!({"deleted":store.release(super::required(params, "key")?)
            .map_err(|error| error.to_string())?}),
        ),
        _ => {
            let query = if method == "memory.search" {
                super::required(params, "query")?
            } else {
                ""
            };
            Ok(
                json!({"store_path":store.path(),"entries":store.search_scoped(query, None).map_err(|error| error.to_string())?}),
            )
        }
    }
}

fn read_text(path: &Path, max: u64) -> Result<Option<String>, String> {
    let file = match open_private_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("invalid_params: artifact must be a regular file".into());
    }
    let mut text = String::new();
    file.take(max + 1)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.len() as u64 > max {
        return Err("artifact exceeds read limit".into());
    }
    Ok(Some(text))
}

fn prompts(path: &Path) -> Result<Value, String> {
    #[derive(Deserialize)]
    struct History {
        schema_version: u8,
        prompts: Vec<String>,
    }
    let Some(text) = read_text(path, 4 * 1024 * 1024)? else {
        return Ok(json!({"prompts":[]}));
    };
    let history: History = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if history.schema_version != 1 {
        return Err("unavailable: unsupported prompt history schema".into());
    }
    Ok(json!({"prompts":history.prompts}))
}

fn plans(runtime: &Path, method: &str, params: &Value) -> Result<Value, String> {
    let entries = harness_core::plan::project_plan_list(runtime, None);
    if method == "plans.list" {
        return Ok(json!({"plans":entries}));
    }
    let path = absolute(params, "path")?;
    if !entries.iter().any(|entry| Path::new(&entry.path) == path) {
        return Err("invalid_params: path must identify a listed project plan".into());
    }
    let content = read_text(&path, 4 * 1024 * 1024)?.ok_or("not_found: plan not found")?;
    Ok(json!({"path":path,"content":content}))
}

fn scratch(home: &Path, prompt: &str) -> Result<Value, String> {
    let slug: String = prompt
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .take(48)
        .collect();
    let slug = if slug.is_empty() { "scratch" } else { &slug };
    let root = home.join("scratch");
    for ancestor in root.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_symlink() => {
                return Err("invalid_params: scratch storage cannot contain symlinks".into())
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string())
            }
            _ => {}
        }
    }
    create_private_dir(&root).map_err(|error| error.to_string())?;
    let directory = tempfile::Builder::new()
        .prefix(&format!("{slug}-"))
        .tempdir_in(&root)
        .map_err(|error| error.to_string())?
        .keep();
    let cwd = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    Ok(json!({"cwd":cwd}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_artifacts_remain_authoritative_and_plan_reads_are_confined(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let runtime = temp.path().join("runtime");
        let store = DurableMemoryStore::for_runtime(&runtime);
        store.put("decision", "Use native journals")?;
        assert_eq!(
            memory(&runtime, "memory.search", &json!({"query":"NATIVE"}))?["entries"][0]["key"],
            "decision"
        );
        assert_eq!(
            memory(&runtime, "memory.delete", &json!({"key":"decision"}))?["deleted"],
            true
        );
        assert!(store.get("decision")?.is_none());
        let plan = harness_core::plan::plan_file_path(&runtime, "run-1");
        create_private_dir(plan.parent().ok_or("missing parent")?)?;
        std::fs::write(&plan, "native plan")?;
        assert_eq!(
            plans(&runtime, "plans.read", &json!({"path":plan}))?["content"],
            "native plan"
        );
        assert!(plans(
            &runtime,
            "plans.read",
            &json!({"path":temp.path().join("outside.md")})
        )
        .is_err());
        let history = temp.path().join("history.json");
        std::fs::write(
            &history,
            r#"{"schema_version":1,"prompts":["recorded prompt"]}"#,
        )?;
        assert_eq!(prompts(&history)?, json!({"prompts":["recorded prompt"]}));
        std::fs::write(&history, r#"{"schema_version":2,"prompts":[]}"#)?;
        assert!(prompts(&history).is_err());
        Ok(())
    }

    #[test]
    fn scratch_workspaces_are_distinct_and_cannot_escape_home(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let first = scratch(temp.path(), "../../ unsafe / project")?;
        let second = scratch(temp.path(), "../../ unsafe / project")?;
        let cwd = Path::new(first["cwd"].as_str().ok_or("missing cwd")?);
        assert!(cwd.is_dir());
        assert!(cwd.starts_with(temp.path().join("scratch")));
        assert_ne!(first["cwd"], second["cwd"]);
        #[cfg(unix)]
        {
            let alias = temp.path().join("alias");
            std::os::unix::fs::symlink(temp.path(), &alias)?;
            assert!(scratch(&alias, "unsafe storage").is_err());
        }
        Ok(())
    }
}
