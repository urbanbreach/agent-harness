use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use harness_core::event::EventV1;
use harness_core::session::AssistantPart;
use harness_core::store::{
    create_private_dir, open_private_file, validate_session_id, write_private_atomic, Journal,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{required, Server};
use crate::replay::{inspect_session_catalog, read_bounded_history};

// Serialize read-modify-write across concurrent pin requests, without creating a lock file.
static PINS_LOCK: Mutex<()> = Mutex::new(());

pub(super) async fn handle(
    server: &Server,
    method: &str,
    params: &Value,
) -> Option<Result<Value, String>> {
    Some(match method {
        "state.read" | "state.write" => state(server, method, params).await,
        "sessions.list" => list(server, params).await,
        "sessions.events" => events(params).await,
        "sessions.search" => search(server, params).await,
        "sessions.rename" => rename(server, params).await,
        "sessions.delete" => delete(server, params).await,
        "sessions.pins" | "sessions.setPinned" => pins(server, method, params).await,
        _ => return None,
    })
}

fn absolute_path(value: &str, key: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("invalid_params: {key} must be an absolute path"));
    }
    Ok(path)
}

fn cwd(params: &Value) -> Result<PathBuf, String> {
    absolute_path(required(params, "cwd")?, "cwd")
}

fn run_dir(params: &Value) -> Result<PathBuf, String> {
    absolute_path(required(params, "runDir")?, "runDir")
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| error.to_string())?
}

fn state_path(home: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name.len() > 32
        || !name.as_bytes()[0].is_ascii_lowercase()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("invalid_params: invalid state document name".into());
    }
    Ok(home.join("gui").join(format!("{name}.json")))
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    let mut file = match open_private_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let parent = path.parent().ok_or("state path has no parent")?;
    create_private_dir(parent).map_err(|error| error.to_string())?;
    write_private_atomic(path, &bytes).map_err(|error| error.to_string())
}

async fn state(server: &Server, method: &str, params: &Value) -> Result<Value, String> {
    let name = required(params, "name")?;
    // Reject names before resolving settings or touching storage.
    let relative = state_path(Path::new(""), name)?;
    let value = if method == "state.write" {
        Some(
            params
                .get("value")
                .ok_or("invalid_params: value is required")?
                .clone(),
        )
    } else {
        None
    };
    let settings = server
        .settings(&server.config_context.discovery.current_dir)
        .await?;
    let path = settings.data_dir.join(relative);
    blocking(move || {
        if let Some(value) = value {
            write_json(&path, &value)?;
            Ok(Value::Null)
        } else {
            Ok(read_json(&path)?.unwrap_or(Value::Null))
        }
    })
    .await
}

async fn list(server: &Server, params: &Value) -> Result<Value, String> {
    let cwd = cwd(params)?;
    let settings = server.settings(&cwd).await?;
    let live: Vec<_> = server
        .sessions
        .lock()
        .await
        .values()
        .map(|session| (session.run_dir.clone(), session.cwd.clone()))
        .collect();
    blocking(move || {
        let rows = inspect_session_catalog(&settings.session_dir)?;
        let worktrees =
            harness_core::worktree::list_session_worktrees(&cwd, None, &settings.data_dir)
                .unwrap_or_default();
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            if !row.is_visible_in_operator_history() {
                continue;
            }
            let live_cwd = live
                .iter()
                .find(|(path, _)| path == &row.run_dir)
                .map(|(_, cwd)| cwd);
            let workspace = live_cwd
                .cloned()
                .or_else(|| row.catalog.workspace_root.as_ref().map(PathBuf::from))
                .unwrap_or_else(|| cwd.clone());
            let created_at = harness_core::proj::load_run_metadata(&row.run_dir)
                .and_then(|metadata| metadata.created_at);
            let mut value = json!({
                "runId": row.catalog.run_id,
                "runDir": row.run_dir,
                "cwd": workspace,
                "projectCwd": cwd,
                "catalog": row.catalog,
                "live": live_cwd.is_some(),
            });
            if let Some(worktree) = worktrees.iter().find(|entry| entry.path == workspace) {
                value["branch"] = json!(worktree.branch);
                value["worktree"] = json!(worktree.harness_managed);
            }
            if let Some(created_at) = created_at {
                value["createdAt"] = created_at.into();
            }
            result.push(value);
        }
        Ok(Value::Array(result))
    })
    .await
}

async fn events(params: &Value) -> Result<Value, String> {
    let path = run_dir(params)?;
    blocking(move || {
        serde_json::to_value(read_bounded_history(&path)?).map_err(|error| error.to_string())
    })
    .await
}

async fn search(server: &Server, params: &Value) -> Result<Value, String> {
    let query = required(params, "query")?.to_lowercase();
    let limit = params["limit"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or("invalid_params: limit must be a nonnegative integer")?;
    let cwds = params["cwds"]
        .as_array()
        .ok_or("invalid_params: cwds must be an array of absolute paths")?;
    let mut directories = BTreeSet::new();
    for value in cwds {
        let cwd = absolute_path(
            value
                .as_str()
                .ok_or("invalid_params: cwds must contain strings")?,
            "cwds",
        )?;
        directories.insert(server.settings(&cwd).await?.session_dir);
    }
    blocking(move || search_history(directories, &query, limit)).await
}

fn search_history(
    directories: BTreeSet<PathBuf>,
    query: &str,
    limit: usize,
) -> Result<Value, String> {
    let mut result = Vec::new();
    if query.is_empty() || limit == 0 {
        return Ok(Value::Array(result));
    }
    for directory in directories {
        for row in inspect_session_catalog(&directory)? {
            if !row.is_visible_in_operator_history() {
                continue;
            }
            for event in read_bounded_history(&row.run_dir)? {
                let Some(value) = search_result(event.payload, &row.run_dir, query) else {
                    continue;
                };
                result.push(value);
                if result.len() == limit {
                    return Ok(Value::Array(result));
                }
            }
        }
    }
    Ok(Value::Array(result))
}

fn search_result(payload: EventV1, run_dir: &Path, query: &str) -> Option<Value> {
    let (role, text) = match payload {
        EventV1::UserMessageSubmitted(message) => ("user", message.text),
        EventV1::AssistantMessageFinished(message) => {
            let text = message
                .parts
                .into_iter()
                .filter_map(|part| match part {
                    AssistantPart::Text { text } => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            ("assistant", text)
        }
        _ => return None,
    };
    let snippet = snippet(&text, query)?;
    Some(json!({"runDir": run_dir, "role": role, "snippet": snippet}))
}

fn snippet(text: &str, query: &str) -> Option<String> {
    let offset = text.to_lowercase().find(query)?;
    // Lowercasing may expand a Unicode character, so offsets cannot index the original text.
    let mut folded_bytes = 0;
    let position = text
        .chars()
        .take_while(|character| {
            if folded_bytes >= offset {
                return false;
            }
            folded_bytes += character.to_lowercase().map(char::len_utf8).sum::<usize>();
            true
        })
        .count();
    let start = position.saturating_sub(80);
    let mut result = String::new();
    if start > 0 {
        result.push_str("...");
    }
    let mut characters = text.chars().skip(start);
    result.extend(characters.by_ref().take(240));
    if characters.next().is_some() {
        result.push_str("...");
    }
    Some(result)
}

async fn rename(server: &Server, params: &Value) -> Result<Value, String> {
    let path = run_dir(params)?;
    let title = required(params, "title")?.to_owned();
    if title.trim().is_empty() || title.len() > 1024 {
        return Err("invalid_params: invalid session title".into());
    }
    let path = blocking(move || path.canonicalize().map_err(|error| error.to_string())).await?;
    let coordinator = {
        let sessions = server.sessions.lock().await;
        sessions
            .values()
            .find(|session| session.run_dir == path)
            .map(|session| session.coordinator.clone())
    };
    if let Some(coordinator) = coordinator {
        coordinator
            .update_session_title(title)
            .await
            .map_err(|error| error.to_string())?;
    } else {
        let inspect_path = path.clone();
        let workspace = blocking(move || {
            Ok(crate::replay::inspect_session(&inspect_path)?
                .catalog
                .workspace_root)
        })
        .await?
        .map(PathBuf::from)
        .unwrap_or_else(|| server.config_context.discovery.current_dir.clone());
        let settings = server.settings(&workspace).await?;
        blocking(move || {
            let deps = harness::CliDeps::real()
                .with_current_dir(workspace)
                .with_env("HARNESS_HOME", settings.data_dir.to_string_lossy());
            let config = settings.config.unwrap_or_default();
            rename_idle(&path, title, &config, &deps)
        })
        .await?;
    }
    Ok(Value::Null)
}

fn rename_idle(
    path: &Path,
    title: String,
    config: &harness_core::config::HarnessConfig,
    deps: &harness::CliDeps,
) -> Result<(), String> {
    let (parent, id) = session_location(path)?;
    let store = harness_core::auth::CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    let secrets = crate::bootstrap::secret_values(config, deps, store.as_ref())?;
    harness_core::session_title::update_offline_title(parent, id, title, secrets)
        .map_err(|error| error.to_string())
}

fn session_location(path: &Path) -> Result<(&Path, &str), String> {
    let parent = path
        .parent()
        .ok_or("invalid_params: session directory has no parent")?;
    let id = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid_params: invalid session directory name")?;
    validate_session_id(id).map_err(|error| format!("invalid_params: {error}"))?;
    Ok((parent, id))
}

async fn delete(server: &Server, params: &Value) -> Result<Value, String> {
    let path = run_dir(params)?;
    let path = blocking(move || path.canonicalize().map_err(|error| error.to_string())).await?;
    let sessions = server.sessions.lock().await;
    if sessions.values().any(|session| session.run_dir == path) {
        return Err("cannot delete a live session; close it first".into());
    }
    // Keep the registry locked until the move finishes so a session cannot open mid-delete.
    let result = blocking(move || {
        let (parent, id) = session_location(&path)?;
        let _journal =
            Journal::open_existing(parent, id, false).map_err(|error| error.to_string())?;
        if read_bounded_history(&path)?.is_empty() {
            return Err("session journal is empty".into());
        }
        let trash = parent.join("trash");
        create_private_dir(&trash).map_err(|error| error.to_string())?;
        let destination = tempfile::Builder::new()
            .prefix(&format!("{id}-"))
            .tempdir_in(&trash)
            .map_err(|error| error.to_string())?;
        std::fs::rename(&path, destination.path()).map_err(|error| error.to_string())?;
        let destination = destination.keep();
        Ok(json!({"trashedTo": destination}))
    })
    .await;
    drop(sessions);
    result
}

#[derive(Deserialize, Serialize)]
struct PersistedPins {
    schema_version: u8,
    pinned_run_ids: BTreeSet<String>,
}

fn load_pins(path: &Path) -> Result<BTreeSet<String>, String> {
    let Some(value) = read_json(path)? else {
        return Ok(BTreeSet::new());
    };
    let pins: PersistedPins = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if pins.schema_version != 1 {
        return Err(format!(
            "unsupported session pins schema {}",
            pins.schema_version
        ));
    }
    Ok(pins.pinned_run_ids)
}

async fn pins(server: &Server, method: &str, params: &Value) -> Result<Value, String> {
    let cwd = cwd(params)?;
    let change = if method == "sessions.setPinned" {
        let run_id = required(params, "runId")?.to_owned();
        validate_session_id(&run_id).map_err(|error| format!("invalid_params: {error}"))?;
        let pinned = params["pinned"]
            .as_bool()
            .ok_or("invalid_params: pinned must be a boolean")?;
        Some((run_id, pinned))
    } else {
        None
    };
    let settings = server.settings(&cwd).await?;
    let path = settings.session_dir.join("tui/session-pins.json");
    blocking(move || {
        let _guard = PINS_LOCK.lock().map_err(|error| error.to_string())?;
        let mut pins = load_pins(&path)?;
        if let Some((run_id, pinned)) = change {
            if pinned {
                pins.insert(run_id);
            } else {
                pins.remove(&run_id);
            }
            let value = serde_json::to_value(PersistedPins {
                schema_version: 1,
                pinned_run_ids: pins,
            })
            .map_err(|error| error.to_string())?;
            write_json(&path, &value)?;
            Ok(Value::Null)
        } else {
            serde_json::to_value(pins).map_err(|error| error.to_string())
        }
    })
    .await
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;
