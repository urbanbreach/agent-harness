use crate::{CliDeps, CliIo};
use harness_core::memory::DurableMemoryStore;
use serde_json::json;
use std::path::PathBuf;

#[derive(clap::Args)]
pub(crate) struct MemoryCommand {
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: MemoryAction,
}
#[derive(clap::Subcommand)]
enum MemoryAction {
    List,
    Get { key: String },
    Put { key: String, value: String },
    Search { query: Option<String> },
}
pub(crate) fn memory(
    command: MemoryCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let root = workspace_root(command.workspace, deps)?;
    let store = DurableMemoryStore::for_workspace(&root);
    let report = match command.action {
        MemoryAction::Get { key } => json!(store
            .get(&key)
            .map_err(|e| e.to_string())?
            .ok_or("memory key not found")?),
        MemoryAction::Put { key, value } => {
            json!(store.put(&key, &value).map_err(|e| e.to_string())?)
        }
        MemoryAction::List => {
            json!({"store_path":store.path(),"entries":store.search("").map_err(|e| e.to_string())?})
        }
        MemoryAction::Search { query } => {
            json!({"store_path":store.path(),"entries":store.search(query.as_deref().unwrap_or("")).map_err(|e| e.to_string())?})
        }
    };
    crate::inspect::print_json(io, &report)
}

#[derive(clap::Args)]
pub(crate) struct CodeGraphCommand {
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: GraphAction,
}
#[derive(clap::Subcommand)]
enum GraphAction {
    Build,
    Query {
        symbol: String,
        #[arg(long, default_value = "symbol_def", value_parser = ["symbol_def", "callers", "callees", "references"])]
        kind: String,
    },
}
pub(crate) fn graph(
    command: CodeGraphCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    use harness_core::code_graph::{
        build_persistent_graph_index, query_persistent_graph, GraphQuery,
    };
    let root = workspace_root(command.workspace, deps)?;
    match command.action {
        GraphAction::Build => {
            let (path, index) = build_persistent_graph_index(&root).map_err(|e| e.to_string())?;
            crate::inspect::print_json(
                io,
                &json!({"schema_version":"harness-code-graph-build-v1","workspace":root,
                "index_path":path,"index_schema":index.schema,"symbol_count":index.symbols.len()}),
            )
        }
        GraphAction::Query { symbol, kind } => {
            let kind = serde_json::from_value(json!(kind)).map_err(|e| e.to_string())?;
            let result = query_persistent_graph(&root, &GraphQuery::with_kind(symbol, kind));
            crate::inspect::print_json(
                io,
                &json!({"schema_version":"harness-code-graph-query-v1","workspace":root,"result":result}),
            )?;
            if result.is_unavailable() {
                Err("code index is unavailable".into())
            } else {
                Ok(())
            }
        }
    }
}

#[derive(clap::Args)]
pub(crate) struct AttributionCommand {
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: AttributionAction,
}
#[derive(clap::Subcommand)]
enum AttributionAction {
    Diff { path: PathBuf },
    Blame { path: PathBuf },
}
pub(crate) fn attribution(
    command: AttributionCommand,
    config: Option<&std::path::Path>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let loaded = crate::inspect::configured(config, deps)?;
    let redactor = crate::inspect::redactor(&loaded.config, deps)?;
    let journal = harness_core::edit_attribution::EditAttributionJournal::open(workspace_root(
        command.workspace,
        deps,
    )?)
    .map_err(|e| e.to_string())?;
    let mut report = match command.action {
        AttributionAction::Diff { path } => json!(journal.diff(path).map_err(|e| e.to_string())?),
        AttributionAction::Blame { path } => {
            json!(journal.blame(path).map_err(|e| e.to_string())?)
        }
    };
    harness_core::redact::redact_in_place(&redactor, &mut report);
    crate::inspect::print_json(io, &report)
}
pub(crate) fn workspace_root(path: Option<PathBuf>, deps: &CliDeps) -> Result<PathBuf, String> {
    let root = deps
        .current_dir()
        .map_err(|e| e.to_string())?
        .join(path.unwrap_or_default());
    if !root.is_dir() {
        return Err("workspace must be an existing directory".into());
    }
    Ok(root)
}
