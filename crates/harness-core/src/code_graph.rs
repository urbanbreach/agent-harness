use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
mod index;
pub use index::{build_persistent_graph_index, load_simple_graph_index};
pub const GRAPH_INDEX_REL: &str = ".agent-harness/code-graph-index.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PersistentGraphAvailability {
    Available { index_root: String },
    Unavailable { reason: String },
}
impl PersistentGraphAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
    pub fn is_unavailable(&self) -> bool {
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { index_root } => format!("persistent graph: available ({index_root})"),
            Self::Unavailable { reason } => format!("persistent graph: unavailable ({reason})"),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphQueryKind {
    #[default]
    SymbolDef,
    Callers,
    Callees,
    References,
}
impl GraphQueryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SymbolDef => "symbol_def",
            Self::Callers => "callers",
            Self::Callees => "callees",
            Self::References => "references",
        }
    }
}
pub const ALL_GRAPH_QUERY_KINDS: &[GraphQueryKind] = &[
    GraphQueryKind::SymbolDef,
    GraphQueryKind::Callers,
    GraphQueryKind::Callees,
    GraphQueryKind::References,
];
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQuery {
    pub symbol: String,
    #[serde(default)]
    pub kind: GraphQueryKind,
}
impl GraphQuery {
    pub fn symbol_def(symbol: impl Into<String>) -> Self {
        Self::with_kind(symbol, GraphQueryKind::SymbolDef)
    }
    pub fn with_kind(symbol: impl Into<String>, kind: GraphQueryKind) -> Self {
        Self {
            symbol: symbol.into(),
            kind,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSymbolHit {
    pub symbol: String,
    pub path: String,
    pub line: u32,
    pub kind: GraphQueryKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    Call,
    Reference,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub caller: String,
    pub callee: String,
    pub path: String,
    pub line: u32,
    pub kind: GraphEdgeKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimpleGraphIndex {
    pub schema: String,
    pub symbols: Vec<GraphSymbolHit>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum GraphQueryResult {
    Hit {
        symbol: String,
        kind: GraphQueryKind,
        hits: Vec<GraphSymbolHit>,
    },
    Unavailable {
        reason: String,
        symbol: String,
        kind: GraphQueryKind,
    },
}
impl GraphQueryResult {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
    pub fn is_hit(&self) -> bool {
        matches!(self, Self::Hit { .. })
    }
    pub fn hit_count(&self) -> usize {
        match self {
            Self::Hit { hits, .. } => hits.len(),
            _ => 0,
        }
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Hit { symbol, kind, hits } => format!(
                "graph query hit: {} `{symbol}` ({} hits)",
                kind.as_str(),
                hits.len()
            ),
            Self::Unavailable {
                reason,
                symbol,
                kind,
            } => format!(
                "graph query unavailable: {} `{symbol}` ({reason})",
                kind.as_str()
            ),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQueryBatchResult {
    pub results: Vec<GraphQueryResult>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQueryBatchSummary {
    pub total: usize,
    pub unavailable: usize,
    pub hits: usize,
    pub hit_results: usize,
}
impl GraphQueryBatchSummary {
    pub fn all_unavailable(&self) -> bool {
        self.total > 0 && self.unavailable == self.total
    }
    pub fn one_line(&self) -> String {
        format!(
            "graph batch: {} unavailable, {} hit_results ({} total; {} symbol hits)",
            self.unavailable, self.hit_results, self.total, self.hits
        )
    }
}
impl GraphQueryBatchResult {
    pub fn summary(&self) -> GraphQueryBatchSummary {
        GraphQueryBatchSummary {
            total: self.results.len(),
            unavailable: self.results.iter().filter(|r| r.is_unavailable()).count(),
            hits: self.results.iter().map(GraphQueryResult::hit_count).sum(),
            hit_results: self.results.iter().filter(|r| r.is_hit()).count(),
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum CodeGraphError {
    #[error("code index I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("code index: {0}")]
    Invalid(&'static str),
}
pub fn detect_persistent_graph(workspace_root: &Path) -> PersistentGraphAvailability {
    match load_simple_graph_index(workspace_root) {
        Ok(Some(_)) => PersistentGraphAvailability::Available {
            index_root: workspace_root.join(GRAPH_INDEX_REL).display().to_string(),
        },
        Ok(None) => PersistentGraphAvailability::Unavailable {
            reason: "no saved code index; build one explicitly".into(),
        },
        Err(error) => PersistentGraphAvailability::Unavailable {
            reason: error.to_string(),
        },
    }
}
pub fn query_persistent_graph(workspace_root: &Path, query: &GraphQuery) -> GraphQueryResult {
    match load_simple_graph_index(workspace_root) {
        Ok(Some(index)) => query_index(&index, query),
        Ok(None) => unavailable(query, "no saved code index"),
        Err(error) => unavailable(query, &error.to_string()),
    }
}
fn unavailable(query: &GraphQuery, reason: &str) -> GraphQueryResult {
    GraphQueryResult::Unavailable {
        symbol: query.symbol.clone(),
        kind: query.kind,
        reason: reason.into(),
    }
}
fn query_index(index: &SimpleGraphIndex, query: &GraphQuery) -> GraphQueryResult {
    let hits = if query.kind == GraphQueryKind::SymbolDef {
        index
            .symbols
            .iter()
            .filter(|h| h.symbol == query.symbol)
            .cloned()
            .collect()
    } else {
        index
            .edges
            .iter()
            .filter_map(|edge| {
                if query.kind != GraphQueryKind::References && edge.kind != GraphEdgeKind::Call {
                    return None;
                }
                let symbol = match query.kind {
                    GraphQueryKind::Callees if edge.caller == query.symbol => &edge.callee,
                    GraphQueryKind::Callers | GraphQueryKind::References
                        if edge.callee == query.symbol =>
                    {
                        &edge.caller
                    }
                    _ => return None,
                };
                Some(GraphSymbolHit {
                    symbol: symbol.clone(),
                    path: edge.path.clone(),
                    line: edge.line,
                    kind: query.kind,
                })
            })
            .collect()
    };
    GraphQueryResult::Hit {
        symbol: query.symbol.clone(),
        kind: query.kind,
        hits,
    }
}
pub fn query_persistent_graph_batch(
    workspace_root: &Path,
    queries: &[GraphQuery],
) -> GraphQueryBatchResult {
    let loaded = load_simple_graph_index(workspace_root);
    let results = queries
        .iter()
        .map(|query| match &loaded {
            Ok(Some(index)) => query_index(index, query),
            Ok(None) => unavailable(query, "no saved code index"),
            Err(error) => unavailable(query, &error.to_string()),
        })
        .collect();
    GraphQueryBatchResult { results }
}
pub fn query_persistent_graph_multi_symbol(
    workspace_root: &Path,
    symbols: &[&str],
    kinds: &[GraphQueryKind],
) -> GraphQueryBatchResult {
    let kinds = if kinds.is_empty() {
        ALL_GRAPH_QUERY_KINDS
    } else {
        kinds
    };
    let queries: Vec<_> = symbols
        .iter()
        .flat_map(|symbol| {
            kinds
                .iter()
                .map(move |kind| GraphQuery::with_kind(*symbol, *kind))
        })
        .collect();
    query_persistent_graph_batch(workspace_root, &queries)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentGraphProductProbe {
    pub availability: PersistentGraphAvailability,
    pub batch: GraphQueryBatchResult,
    pub index_path: Option<PathBuf>,
}
impl PersistentGraphProductProbe {
    pub fn summary(&self) -> GraphQueryBatchSummary {
        self.batch.summary()
    }
    pub fn is_unavailable(&self) -> bool {
        self.availability.is_unavailable() && self.summary().all_unavailable()
    }
}
/// An explicit index build; ordinary query and readiness paths never call it.
pub fn probe_persistent_graph_product(
    workspace_root: &Path,
    symbols: &[&str],
) -> PersistentGraphProductProbe {
    match build_persistent_graph_index(workspace_root) {
        Ok((path, index)) => PersistentGraphProductProbe {
            availability: PersistentGraphAvailability::Available {
                index_root: path.display().to_string(),
            },
            batch: GraphQueryBatchResult {
                results: symbols
                    .iter()
                    .flat_map(|symbol| {
                        ALL_GRAPH_QUERY_KINDS
                            .iter()
                            .map(|kind| query_index(&index, &GraphQuery::with_kind(*symbol, *kind)))
                    })
                    .collect(),
            },
            index_path: Some(path),
        },
        Err(error) => {
            let reason = error.to_string();
            PersistentGraphProductProbe {
                availability: PersistentGraphAvailability::Unavailable {
                    reason: reason.clone(),
                },
                batch: GraphQueryBatchResult {
                    results: symbols
                        .iter()
                        .flat_map(|symbol| {
                            ALL_GRAPH_QUERY_KINDS.iter().map(|kind| {
                                unavailable(&GraphQuery::with_kind(*symbol, *kind), &reason)
                            })
                        })
                        .collect(),
                },
                index_path: None,
            }
        }
    }
}
