//! Code graph CLI surface (`harness code-graph build|query`).
//!
//! Surfaces the real first-party simple symbol index from
//! [`harness_core::code_graph`] behind operator commands. `build` writes a
//! durable `.agent-harness/code-graph-index.json` from workspace sources;
//! `query` answers `symbol_def` with real file/line hits and structurally
//! fails closed (Unavailable) for callers/callees/references until a richer
//! graph backend lands. Never claims relationship queries it cannot answer.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::code_graph::{
    build_persistent_graph_index, query_persistent_graph, GraphQuery, GraphQueryKind,
};

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct CodeGraphCommand {
    #[command(subcommand)]
    command: CodeGraphSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum CodeGraphSubcommand {
    /// Build the durable on-disk symbol index from workspace sources.
    Build(CodeGraphBuildCommand),
    /// Query the persistent graph (symbol_def hits; relationship kinds fail closed).
    Query(CodeGraphQueryCommand),
}

#[derive(Debug, Args, Clone)]
struct CodeGraphBuildCommand {
    /// Workspace root to index (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct CodeGraphQueryCommand {
    /// Symbol name to look up.
    symbol: String,

    /// Query kind: symbol_def (default), callers, callees, or references.
    #[arg(long, default_value = "symbol_def")]
    kind: String,

    /// Workspace root containing `.agent-harness/code-graph-index.json` (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

pub(crate) fn execute_with_io(
    command: CodeGraphCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    match command.command {
        CodeGraphSubcommand::Build(cmd) => run_build(cmd, io, deps),
        CodeGraphSubcommand::Query(cmd) => run_query(cmd, io, deps),
    }
}

fn resolve_workspace_root(explicit: &Option<PathBuf>, deps: &CliDeps) -> PathBuf {
    explicit
        .clone()
        .unwrap_or_else(|| deps.current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn parse_kind(raw: &str) -> Option<GraphQueryKind> {
    match raw {
        "symbol_def" => Some(GraphQueryKind::SymbolDef),
        "callers" => Some(GraphQueryKind::Callers),
        "callees" => Some(GraphQueryKind::Callees),
        "references" => Some(GraphQueryKind::References),
        _ => None,
    }
}

fn run_build(cmd: CodeGraphBuildCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&cmd.workspace, deps);
    match build_persistent_graph_index(&root) {
        Ok((index_path, index)) => {
            let output = serde_json::json!({
                "schema_version": "harness-code-graph-build-v1",
                "workspace": root.display().to_string(),
                "index_path": index_path.display().to_string(),
                "index_schema": index.schema,
                "symbol_count": index.symbols.len(),
            });
            write_json(io, &output)
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "code-graph build failed: {err}");
            1
        }
    }
}

fn run_query(cmd: CodeGraphQueryCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let symbol = cmd.symbol.trim();
    if symbol.is_empty() {
        let _ = writeln!(io.stderr, "code-graph query requires a non-empty symbol");
        return 2;
    }
    let kind = match parse_kind(cmd.kind.as_str()) {
        Some(kind) => kind,
        None => {
            let _ = writeln!(
                io.stderr,
                "code-graph query: unknown kind `{}` (expected symbol_def, callers, callees, or references)",
                cmd.kind
            );
            return 2;
        }
    };
    let root = resolve_workspace_root(&cmd.workspace, deps);
    let result = query_persistent_graph(&root, &GraphQuery::with_kind(symbol, kind));
    let output = serde_json::json!({
        "schema_version": "harness-code-graph-query-v1",
        "workspace": root.display().to_string(),
        "result": result,
    });
    write_json(io, &output)
}

fn write_json(io: &mut CliIo<'_>, output: &serde_json::Value) -> i32 {
    match serde_json::to_string_pretty(output) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "code-graph: failed to serialize JSON: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliIo;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn run_cli(cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real().with_current_dir(cwd.to_path_buf());
        let mut argv: Vec<String> = vec!["harness".to_string()];
        argv.extend(args.iter().copied().map(str::to_string));
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn seed_workspace(ws: &std::path::Path) {
        let src = ws.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "pub fn alpha() {}\npub struct Beta {}\n",
        )
        .unwrap();
    }

    #[test]
    fn code_graph_build_writes_durable_index_from_workspace_sources() {
        // arrange — workspace with two definitions
        let dir = tempdir().unwrap();
        let ws = dir.path();
        seed_workspace(ws);

        // act
        let (code, stdout, stderr) = run_cli(ws, &["code-graph", "build"]);

        // assert — durable index side effect + honest symbol count
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["schema_version"].as_str(),
            Some("harness-code-graph-build-v1"),
            "stdout: {stdout}"
        );
        assert_eq!(output["symbol_count"].as_u64(), Some(2), "stdout: {stdout}");
        assert!(
            ws.join(".agent-harness/code-graph-index.json").is_file(),
            "build must write the durable index file"
        );
    }

    #[test]
    fn code_graph_query_symbol_def_returns_file_line_hits_after_build() {
        // arrange — built index over a seeded workspace
        let dir = tempdir().unwrap();
        let ws = dir.path();
        seed_workspace(ws);
        let (build_code, _, build_stderr) = run_cli(ws, &["code-graph", "build"]);
        assert_eq!(build_code, 0, "build stderr: {build_stderr}");

        // act
        let (code, stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha"]);

        // assert — real file/line precision from the on-disk index
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["result"]["status"].as_str(),
            Some("hit"),
            "stdout: {stdout}"
        );
        let hits = output["result"]["hits"].as_array().expect("hits array");
        assert!(
            hits.iter().any(|hit| {
                hit["path"]
                    .as_str()
                    .is_some_and(|path| path.contains("lib.rs"))
                    && hit["line"].as_u64() == Some(1)
            }),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn code_graph_query_relationship_kind_fails_closed_after_build() {
        // arrange — built index over a seeded workspace
        let dir = tempdir().unwrap();
        let ws = dir.path();
        seed_workspace(ws);
        let (build_code, _, build_stderr) = run_cli(ws, &["code-graph", "build"]);
        assert_eq!(build_code, 0, "build stderr: {build_stderr}");

        // act — query a relationship kind the backend does not implement
        let (code, stdout, stderr) =
            run_cli(ws, &["code-graph", "query", "alpha", "--kind", "callers"]);

        // assert — honest structured Unavailable even with a built index
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["result"]["status"].as_str(),
            Some("unavailable"),
            "stdout: {stdout}"
        );
        let reason = output["result"]["reason"].as_str().expect("reason");
        assert!(reason.contains("symbol_def only"), "reason: {reason}");
        assert!(reason.contains("callers"), "reason: {reason}");
    }

    #[test]
    fn code_graph_query_without_index_fails_closed_without_creating_one() {
        // arrange — empty workspace, no index
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act
        let (code, stdout, stderr) = run_cli(ws, &["code-graph", "query", "alpha"]);

        // assert — unavailable, and query never writes an index
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["result"]["status"].as_str(),
            Some("unavailable"),
            "stdout: {stdout}"
        );
        assert!(
            output["result"]["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()),
            "stdout: {stdout}"
        );
        assert!(
            !ws.join(".agent-harness/code-graph-index.json").exists(),
            "query must not create an index"
        );
    }

    #[test]
    fn code_graph_query_rejects_unknown_kind_with_usage_error() {
        // arrange — empty workspace
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act
        let (code, _stdout, stderr) =
            run_cli(ws, &["code-graph", "query", "alpha", "--kind", "bogus"]);

        // assert — usage error, nothing executed
        assert_eq!(code, 2);
        assert!(stderr.contains("unknown kind"), "stderr: {stderr}");
    }

    #[test]
    fn code_graph_build_explicit_workspace_isolates_index_from_cwd() {
        // arrange — cwd differs from the explicit workspace
        let cwd_dir = tempdir().unwrap();
        let ws_dir = tempdir().unwrap();
        let ws = ws_dir.path();
        seed_workspace(ws);

        // act
        let ws_arg = ws.display().to_string();
        let (code, stdout, stderr) = run_cli(
            cwd_dir.path(),
            &["code-graph", "build", "--workspace", ws_arg.as_str()],
        );

        // assert — index lands under --workspace, not under cwd
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(output["symbol_count"].as_u64(), Some(2), "stdout: {stdout}");
        assert!(
            ws.join(".agent-harness/code-graph-index.json").is_file(),
            "index must be written under --workspace"
        );
        assert!(
            !cwd_dir
                .path()
                .join(".agent-harness/code-graph-index.json")
                .exists(),
            "index must not be written under cwd when --workspace is given"
        );
    }
}
