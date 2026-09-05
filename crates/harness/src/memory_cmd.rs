//! Product CLI for durable workspace memory (`harness memory`).
//!
//! Surfaces put/get/search/list over [`harness_core::memory::DurableMemoryStore`].
//! Values are redacted on write by the store; this command never prints raw
//! secret material beyond what the store already redacted.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::memory::{DurableMemoryStore, MemoryEntry, MemoryError};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct MemoryCommand {
    #[command(subcommand)]
    command: MemorySubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum MemorySubcommand {
    /// List durable memory entries for the workspace (JSON).
    List(MemoryListCommand),
    /// Get one durable memory entry by exact key (JSON).
    Get(MemoryGetCommand),
    /// Put or update one durable memory entry (value redacted on write).
    Put(MemoryPutCommand),
    /// Search durable memory by key prefix or case-insensitive substring.
    Search(MemorySearchCommand),
}

#[derive(Debug, Args, Clone, Default)]
struct MemoryListCommand {
    /// Workspace root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct MemoryGetCommand {
    /// Memory key.
    key: String,
    /// Workspace root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct MemoryPutCommand {
    /// Memory key.
    key: String,
    /// Memory value (redacted on write).
    value: String,
    /// Workspace root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct MemorySearchCommand {
    /// Search query (empty lists all).
    query: Option<String>,
    /// Workspace root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct MemoryListJson {
    store_path: String,
    entries: Vec<MemoryEntry>,
}

pub(crate) fn execute_with_io(command: MemoryCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    match command.command {
        MemorySubcommand::List(cmd) => run_list(cmd.workspace, io, deps),
        MemorySubcommand::Get(cmd) => run_get(cmd.key, cmd.workspace, io, deps),
        MemorySubcommand::Put(cmd) => run_put(cmd.key, cmd.value, cmd.workspace, io, deps),
        MemorySubcommand::Search(cmd) => {
            run_search(cmd.query.unwrap_or_default(), cmd.workspace, io, deps)
        }
    }
}

fn resolve_workspace(
    workspace: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<PathBuf, i32> {
    match workspace {
        Some(path) => Ok(path),
        None => deps.current_dir().map_err(|err| {
            let _ = writeln!(
                io.stderr,
                "memory: failed to resolve current working directory: {err}"
            );
            2
        }),
    }
}

fn store_for_workspace(workspace: &std::path::Path) -> DurableMemoryStore {
    DurableMemoryStore::for_workspace(workspace)
}

fn write_json(io: &mut CliIo<'_>, value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "memory: failed to serialize JSON: {err}");
            1
        }
    }
}

fn map_memory_error(io: &mut CliIo<'_>, err: MemoryError) -> i32 {
    let _ = writeln!(io.stderr, "memory: {err}");
    1
}

fn run_list(workspace: Option<PathBuf>, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let workspace = match resolve_workspace(workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let store = store_for_workspace(&workspace);
    match store.search("") {
        Ok(entries) => write_json(
            io,
            &MemoryListJson {
                store_path: store.path().display().to_string(),
                entries,
            },
        ),
        Err(err) => map_memory_error(io, err),
    }
}

fn run_get(key: String, workspace: Option<PathBuf>, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let workspace = match resolve_workspace(workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let store = store_for_workspace(&workspace);
    match store.get(&key) {
        Ok(Some(entry)) => write_json(io, &entry),
        Ok(None) => {
            let _ = writeln!(io.stderr, "memory: key not found: {key}");
            1
        }
        Err(err) => map_memory_error(io, err),
    }
}

fn run_put(
    key: String,
    value: String,
    workspace: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let workspace = match resolve_workspace(workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let store = store_for_workspace(&workspace);
    match store.put(&key, &value) {
        Ok(entry) => write_json(io, &entry),
        Err(err) => map_memory_error(io, err),
    }
}

fn run_search(
    query: String,
    workspace: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let workspace = match resolve_workspace(workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let store = store_for_workspace(&workspace);
    match store.search(&query) {
        Ok(entries) => write_json(
            io,
            &MemoryListJson {
                store_path: store.path().display().to_string(),
                entries,
            },
        ),
        Err(err) => map_memory_error(io, err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliIo;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn deps_with_cwd(cwd: PathBuf) -> CliDeps {
        CliDeps::real().with_current_dir(cwd)
    }

    #[test]
    fn put_get_list_roundtrip_via_cli_handlers() {
        // Given
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let deps = deps_with_cwd(workspace.clone());

        // When: put
        {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut stdin = Cursor::new(Vec::new());
            let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
            let put_code = run_put(
                "pref.theme".to_string(),
                "dark".to_string(),
                Some(workspace.clone()),
                &mut io,
                &deps,
            );
            assert_eq!(put_code, 0);
        }

        // When: get
        {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut stdin = Cursor::new(Vec::new());
            let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
            let get_code = run_get(
                "pref.theme".to_string(),
                Some(workspace.clone()),
                &mut io,
                &deps,
            );
            assert_eq!(get_code, 0);
            let get_out = String::from_utf8_lossy(&stdout);
            assert!(get_out.contains("pref.theme"));
            assert!(get_out.contains("dark"));
        }

        // When: list
        {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut stdin = Cursor::new(Vec::new());
            let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
            let list_code = run_list(Some(workspace), &mut io, &deps);
            assert_eq!(list_code, 0);
            let list_out = String::from_utf8_lossy(&stdout);
            assert!(list_out.contains("entries"));
            assert!(list_out.contains("pref.theme"));
        }
    }

    #[test]
    fn get_missing_key_returns_nonzero() {
        let dir = tempdir().unwrap();
        let deps = deps_with_cwd(dir.path().to_path_buf());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut stdin = Cursor::new(Vec::new());
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let code = run_get(
            "missing".to_string(),
            Some(dir.path().to_path_buf()),
            &mut io,
            &deps,
        );
        assert_eq!(code, 1);
        drop(io);
        assert!(String::from_utf8_lossy(&stderr).contains("not found"));
    }

    #[test]
    fn cli_put_redacts_secret_like_value_before_get() {
        // arrange
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let deps = deps_with_cwd(workspace.clone());
        let secret = "sk-abcdefghijklmnopqrstuvwxyz";

        // act — put the secret-like value through the CLI, then get it back
        let put_out = {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut stdin = Cursor::new(Vec::new());
            let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
            let code = run_put(
                "creds.token".to_string(),
                secret.to_string(),
                Some(workspace.clone()),
                &mut io,
                &deps,
            );
            assert_eq!(code, 0);
            stdout
        };
        let get_out = {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut stdin = Cursor::new(Vec::new());
            let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
            let code = run_get("creds.token".to_string(), Some(workspace), &mut io, &deps);
            assert_eq!(code, 0);
            String::from_utf8_lossy(&stdout).to_string()
        };

        // assert — raw secret survives nowhere; the redaction marker does
        assert!(!String::from_utf8_lossy(&put_out).contains(secret));
        assert!(!get_out.contains(secret));
        assert!(get_out.contains("[REDACTED_API_KEY]"));
    }
}
