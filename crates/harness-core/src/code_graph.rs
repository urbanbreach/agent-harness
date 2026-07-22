//! First-party persistent codebase graph surface.
//!
//! Detects an external `.codegraph` index root and/or a harness-owned simple
//! on-disk symbol index under `.agent-harness/code-graph-index.json`. The simple
//! index can be built from workspace source files and queried for symbol
//! definitions and call/reference relationships (real filesystem side effects).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Relative path for the harness-owned simple symbol index.
pub const GRAPH_INDEX_REL: &str = ".agent-harness/code-graph-index.json";

/// Availability of a first-party persistent code graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PersistentGraphAvailability {
    /// Graph backend ready (not used by this MVP).
    Available { index_root: String },
    /// No first-party persistent graph; external tools may still exist.
    Unavailable { reason: String },
}

impl PersistentGraphAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (does not claim a graph backend).
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { index_root } => {
                format!("persistent graph: available (index_root={index_root})")
            }
            Self::Unavailable { reason } => {
                format!("persistent graph: unavailable ({reason})")
            }
        }
    }
}

/// Detect persistent graph availability for a workspace.
///
/// Available when a harness simple index exists and/or a `.codegraph` (or
/// `codegraph`) directory is present. External SQLite backends are not queried
/// directly; first-party query uses the simple index file.
pub fn detect_persistent_graph(workspace_root: &Path) -> PersistentGraphAvailability {
    let simple_index = workspace_root.join(GRAPH_INDEX_REL);
    if simple_index.is_file() {
        return PersistentGraphAvailability::Available {
            index_root: simple_index.display().to_string(),
        };
    }
    for candidate in [".codegraph", "codegraph"] {
        let path = workspace_root.join(candidate);
        if path.is_dir() {
            return PersistentGraphAvailability::Available {
                index_root: path.display().to_string(),
            };
        }
    }
    PersistentGraphAvailability::Unavailable {
        reason: "no first-party code-graph index and no `.codegraph`/`codegraph` directory; \
                 run build_persistent_graph_index to create a simple on-disk index"
            .to_string(),
    }
}

/// Kind of thin graph lookup (API shape only; backend not implemented).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphQueryKind {
    SymbolDef,
    Callers,
    Callees,
    References,
}

impl GraphQueryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SymbolDef => "symbol_def",
            Self::Callers => "callers",
            Self::Callees => "callees",
            Self::References => "references",
        }
    }
}

/// Thin read-only index API placeholder. Always fails closed until a backend lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQuery {
    pub symbol: String,
    #[serde(default = "default_graph_query_kind")]
    pub kind: GraphQueryKind,
}

fn default_graph_query_kind() -> GraphQueryKind {
    GraphQueryKind::SymbolDef
}

impl GraphQuery {
    pub fn symbol_def(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            kind: GraphQueryKind::SymbolDef,
        }
    }

    pub fn with_kind(symbol: impl Into<String>, kind: GraphQueryKind) -> Self {
        Self {
            symbol: symbol.into(),
            kind,
        }
    }
}

/// One symbol hit from the simple on-disk index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSymbolHit {
    pub symbol: String,
    pub path: String,
    pub line: u32,
    pub kind: GraphQueryKind,
}

/// Kind of relationship edge between two symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    Call,
    Reference,
}

/// A directed relationship edge: `caller` references or calls `callee`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub caller: String,
    pub callee: String,
    pub path: String,
    pub line: u32,
    pub kind: GraphEdgeKind,
}

/// On-disk simple symbol index (JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SimpleGraphIndex {
    pub schema: String,
    pub symbols: Vec<GraphSymbolHit>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GraphQueryResult {
    /// Index available and query completed (hits may be empty).
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
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub const fn is_hit(&self) -> bool {
        matches!(self, Self::Hit { .. })
    }

    pub fn hit_count(&self) -> usize {
        match self {
            Self::Hit { hits, .. } => hits.len(),
            Self::Unavailable { .. } => 0,
        }
    }

    /// Operator-facing one-line diagnostics for a single query result.
    pub fn one_line(&self) -> String {
        match self {
            Self::Hit { symbol, kind, hits } => format!(
                "graph query hit: {} `{}` ({} hit{})",
                kind.as_str(),
                symbol,
                hits.len(),
                if hits.len() == 1 { "" } else { "s" }
            ),
            Self::Unavailable {
                reason,
                symbol,
                kind,
            } => format!(
                "graph query unavailable: {} `{}` ({reason})",
                kind.as_str(),
                symbol
            ),
        }
    }
}

/// Batch query result: one structured unavailable entry per input query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQueryBatchResult {
    pub results: Vec<GraphQueryResult>,
}

/// Operator-facing counts for a graph query batch (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GraphQueryBatchSummary {
    pub total: usize,
    pub unavailable: usize,
    pub hits: usize,
    pub hit_results: usize,
}

impl GraphQueryBatchSummary {
    pub fn one_line(&self) -> String {
        format!(
            "graph batch: {} unavailable, {} hit_results ({} total; {} symbol hits)",
            self.unavailable, self.hit_results, self.total, self.hits
        )
    }

    pub const fn all_unavailable(&self) -> bool {
        self.total > 0 && self.unavailable == self.total
    }
}

impl GraphQueryBatchResult {
    /// Summarize batch outcomes for operator/CLI surfaces.
    pub fn summary(&self) -> GraphQueryBatchSummary {
        let mut summary = GraphQueryBatchSummary {
            total: self.results.len(),
            ..GraphQueryBatchSummary::default()
        };
        for result in &self.results {
            match result {
                GraphQueryResult::Unavailable { .. } => {
                    summary.unavailable = summary.unavailable.saturating_add(1);
                }
                GraphQueryResult::Hit { hits, .. } => {
                    summary.hit_results = summary.hit_results.saturating_add(1);
                    summary.hits = summary.hits.saturating_add(hits.len());
                }
            }
        }
        summary
    }
}

/// Errors for simple index build / load.
#[derive(Debug, thiserror::Error)]
pub enum CodeGraphError {
    #[error("read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("parse {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("create parent {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// Build a simple on-disk symbol index from workspace source files.
///
/// Scans limited text/source extensions for definition-like lines (`fn`,
/// `struct`, `enum`, `trait`, `type`, `class`, `def`) and extracts call/reference
/// edges by matching known symbol names in source lines. Writes
/// [`.agent-harness/code-graph-index.json`](GRAPH_INDEX_REL).
pub fn build_persistent_graph_index(
    workspace_root: &Path,
) -> Result<(PathBuf, SimpleGraphIndex), CodeGraphError> {
    let mut symbols = Vec::new();
    walk_for_symbols(workspace_root, workspace_root, 0, &mut symbols)?;
    symbols.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then(a.path.cmp(&b.path))
            .then(a.line.cmp(&b.line))
    });
    let known_names: std::collections::HashSet<&str> =
        symbols.iter().map(|s| s.symbol.as_str()).collect();
    let mut edges = Vec::new();
    walk_for_edges(workspace_root, workspace_root, 0, &known_names, &mut edges)?;
    edges.sort_by(|a, b| {
        a.caller
            .cmp(&b.caller)
            .then(a.callee.cmp(&b.callee))
            .then(a.path.cmp(&b.path))
            .then(a.line.cmp(&b.line))
    });
    let index = SimpleGraphIndex {
        schema: "harness-code-graph-index-v1".to_string(),
        symbols,
        edges,
    };
    let path = workspace_root.join(GRAPH_INDEX_REL);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CodeGraphError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let body = serde_json::to_string_pretty(&index).map_err(|err| CodeGraphError::Write {
        path: path.display().to_string(),
        source: io::Error::other(err),
    })?;
    fs::write(&path, format!("{body}\n")).map_err(|source| CodeGraphError::Write {
        path: path.display().to_string(),
        source,
    })?;
    Ok((path, index))
}

/// Load the harness simple index when present.
pub fn load_simple_graph_index(
    workspace_root: &Path,
) -> Result<Option<SimpleGraphIndex>, CodeGraphError> {
    let path = workspace_root.join(GRAPH_INDEX_REL);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(|source| CodeGraphError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let index: SimpleGraphIndex =
        serde_json::from_str(&raw).map_err(|err| CodeGraphError::Parse {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;
    Ok(Some(index))
}

/// Query the persistent graph using the first-party simple index when present.
pub fn query_persistent_graph(workspace_root: &Path, query: &GraphQuery) -> GraphQueryResult {
    match load_simple_graph_index(workspace_root) {
        Ok(Some(index)) => query_simple_index(&index, query),
        Ok(None) => {
            let reason = match detect_persistent_graph(workspace_root) {
                PersistentGraphAvailability::Available { index_root } => {
                    format!(
                        "external index root present ({index_root}) but no first-party simple index at {GRAPH_INDEX_REL}; \
                         run build_persistent_graph_index for queryable hits"
                    )
                }
                PersistentGraphAvailability::Unavailable { reason } => reason,
            };
            GraphQueryResult::Unavailable {
                reason,
                symbol: query.symbol.clone(),
                kind: query.kind,
            }
        }
        Err(err) => GraphQueryResult::Unavailable {
            reason: err.to_string(),
            symbol: query.symbol.clone(),
            kind: query.kind,
        },
    }
}

fn query_simple_index(index: &SimpleGraphIndex, query: &GraphQuery) -> GraphQueryResult {
    match query.kind {
        GraphQueryKind::SymbolDef => {
            let hits: Vec<GraphSymbolHit> = index
                .symbols
                .iter()
                .filter(|h| h.symbol == query.symbol && h.kind == GraphQueryKind::SymbolDef)
                .cloned()
                .collect();
            GraphQueryResult::Hit {
                symbol: query.symbol.clone(),
                kind: query.kind,
                hits,
            }
        }
        GraphQueryKind::Callers => {
            let hits: Vec<GraphSymbolHit> = index
                .edges
                .iter()
                .filter(|e| e.callee == query.symbol)
                .map(|e| GraphSymbolHit {
                    symbol: e.caller.clone(),
                    path: e.path.clone(),
                    line: e.line,
                    kind: GraphQueryKind::Callers,
                })
                .collect();
            GraphQueryResult::Hit {
                symbol: query.symbol.clone(),
                kind: query.kind,
                hits,
            }
        }
        GraphQueryKind::Callees => {
            let hits: Vec<GraphSymbolHit> = index
                .edges
                .iter()
                .filter(|e| e.caller == query.symbol)
                .map(|e| GraphSymbolHit {
                    symbol: e.callee.clone(),
                    path: e.path.clone(),
                    line: e.line,
                    kind: GraphQueryKind::Callees,
                })
                .collect();
            GraphQueryResult::Hit {
                symbol: query.symbol.clone(),
                kind: query.kind,
                hits,
            }
        }
        GraphQueryKind::References => {
            let hits: Vec<GraphSymbolHit> = index
                .edges
                .iter()
                .filter(|e| e.callee == query.symbol)
                .map(|e| GraphSymbolHit {
                    symbol: e.caller.clone(),
                    path: e.path.clone(),
                    line: e.line,
                    kind: GraphQueryKind::References,
                })
                .collect();
            GraphQueryResult::Hit {
                symbol: query.symbol.clone(),
                kind: query.kind,
                hits,
            }
        }
    }
}

const MAX_WALK_DEPTH: usize = 8;
const MAX_INDEX_FILES: usize = 400;
const MAX_INDEX_SYMBOLS: usize = 8_000;

fn walk_for_symbols(
    workspace_root: &Path,
    current: &Path,
    depth: usize,
    out: &mut Vec<GraphSymbolHit>,
) -> Result<(), CodeGraphError> {
    if depth > MAX_WALK_DEPTH || out.len() >= MAX_INDEX_SYMBOLS {
        return Ok(());
    }
    let entries = fs::read_dir(current).map_err(|source| CodeGraphError::Read {
        path: current.display().to_string(),
        source,
    })?;
    let mut files_seen = 0usize;
    for entry in entries.flatten() {
        if out.len() >= MAX_INDEX_SYMBOLS || files_seen >= MAX_INDEX_FILES {
            break;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
            || name == "inspirations"
        {
            continue;
        }
        if path.is_dir() {
            walk_for_symbols(workspace_root, &path, depth + 1, out)?;
            continue;
        }
        if !path.is_file() || !is_indexable_source(&path) {
            continue;
        }
        files_seen = files_seen.saturating_add(1);
        index_file(workspace_root, &path, out)?;
    }
    Ok(())
}

fn is_indexable_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go")
    )
}

fn index_file(
    workspace_root: &Path,
    path: &Path,
    out: &mut Vec<GraphSymbolHit>,
) -> Result<(), CodeGraphError> {
    let raw = fs::read_to_string(path).map_err(|source| CodeGraphError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let rel = path
        .strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    for (idx, line) in raw.lines().enumerate() {
        if out.len() >= MAX_INDEX_SYMBOLS {
            break;
        }
        if let Some(symbol) = extract_definition_symbol(line) {
            out.push(GraphSymbolHit {
                symbol,
                path: rel.clone(),
                line: u32::try_from(idx.saturating_add(1)).unwrap_or(u32::MAX),
                kind: GraphQueryKind::SymbolDef,
            });
        }
    }
    Ok(())
}

const MAX_EDGE_FILES: usize = 400;
const MAX_EDGES: usize = 16_000;

fn walk_for_edges(
    workspace_root: &Path,
    current: &Path,
    depth: usize,
    known_names: &std::collections::HashSet<&str>,
    out: &mut Vec<GraphEdge>,
) -> Result<(), CodeGraphError> {
    if depth > MAX_WALK_DEPTH || out.len() >= MAX_EDGES {
        return Ok(());
    }
    let entries = fs::read_dir(current).map_err(|source| CodeGraphError::Read {
        path: current.display().to_string(),
        source,
    })?;
    let mut files_seen = 0usize;
    for entry in entries.flatten() {
        if out.len() >= MAX_EDGES || files_seen >= MAX_EDGE_FILES {
            break;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
            || name == "inspirations"
        {
            continue;
        }
        if path.is_dir() {
            walk_for_edges(workspace_root, &path, depth + 1, known_names, out)?;
            continue;
        }
        if !path.is_file() || !is_indexable_source(&path) {
            continue;
        }
        files_seen = files_seen.saturating_add(1);
        extract_edges_from_file(workspace_root, &path, known_names, out)?;
    }
    Ok(())
}

fn extract_edges_from_file(
    workspace_root: &Path,
    path: &Path,
    known_names: &std::collections::HashSet<&str>,
    out: &mut Vec<GraphEdge>,
) -> Result<(), CodeGraphError> {
    let raw = fs::read_to_string(path).map_err(|source| CodeGraphError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let rel = path
        .strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut current_caller: Option<String> = None;
    for (idx, line) in raw.lines().enumerate() {
        if out.len() >= MAX_EDGES {
            break;
        }
        let line_no = u32::try_from(idx.saturating_add(1)).unwrap_or(u32::MAX);
        if let Some(def) = extract_definition_symbol(line) {
            current_caller = Some(def);
        }
        let caller = match &current_caller {
            Some(c) => c.clone(),
            None => continue,
        };
        for word in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            if word.is_empty() || word.len() < 2 {
                continue;
            }
            if !known_names.contains(word) {
                continue;
            }
            if word == caller {
                continue;
            }
            let is_call = line.contains(&format!("{word}("))
                || line.contains(&format!("{word}::"))
                || line.contains(&format!(".{word}("));
            out.push(GraphEdge {
                caller: caller.clone(),
                callee: word.to_string(),
                path: rel.clone(),
                line: line_no,
                kind: if is_call {
                    GraphEdgeKind::Call
                } else {
                    GraphEdgeKind::Reference
                },
            });
        }
    }
    Ok(())
}

fn extract_definition_symbol(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") {
        return None;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let mut i = 0usize;
    while i < tokens.len()
        && matches!(
            tokens[i],
            "pub" | "pub(crate)" | "async" | "const" | "static" | "unsafe" | "export" | "default"
        )
    {
        i = i.saturating_add(1);
    }
    if i >= tokens.len() {
        return None;
    }
    let keyword = tokens[i];
    if !matches!(
        keyword,
        "fn" | "struct" | "enum" | "trait" | "type" | "class" | "def" | "interface" | "function"
    ) {
        return None;
    }
    let name_tok = tokens.get(i + 1)?;
    let name = name_tok
        .split(['(', '<', '{', ':', '=', ','])
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
    let first = name.chars().next()?;
    if name.is_empty() || !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(name.to_string())
}

/// Query multiple symbols/kinds. MVP fails closed for every entry (no partial success).
pub fn query_persistent_graph_batch(
    workspace_root: &std::path::Path,
    queries: &[GraphQuery],
) -> GraphQueryBatchResult {
    GraphQueryBatchResult {
        results: queries
            .iter()
            .map(|q| query_persistent_graph(workspace_root, q))
            .collect(),
    }
}

/// All graph query kinds (product batch surface).
pub const ALL_GRAPH_QUERY_KINDS: &[GraphQueryKind] = &[
    GraphQueryKind::SymbolDef,
    GraphQueryKind::Callers,
    GraphQueryKind::Callees,
    GraphQueryKind::References,
];

/// Multi-symbol multi-kind product batch.
///
/// Builds the cartesian product of `symbols` × `kinds` (defaults to all kinds
/// when `kinds` is empty) and runs [`query_persistent_graph_batch`].
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
    let mut queries = Vec::with_capacity(symbols.len().saturating_mul(kinds.len()));
    for symbol in symbols {
        for kind in kinds {
            queries.push(GraphQuery::with_kind(*symbol, *kind));
        }
    }
    query_persistent_graph_batch(workspace_root, &queries)
}

/// Detect + multi-symbol product probe for a workspace.
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

/// Product path: build simple index (side effect) + detect + multi-symbol batch.
///
/// Always attempts to materialize [`.agent-harness/code-graph-index.json`](GRAPH_INDEX_REL)
/// so queries can return real [`GraphQueryResult::Hit`] for `symbol_def`.
pub fn probe_persistent_graph_product(
    workspace_root: &Path,
    symbols: &[&str],
) -> PersistentGraphProductProbe {
    let index_path = match build_persistent_graph_index(workspace_root) {
        Ok((path, _)) => Some(path),
        Err(_) => {
            let path = workspace_root.join(GRAPH_INDEX_REL);
            path.is_file().then_some(path)
        }
    };
    let availability = detect_persistent_graph(workspace_root);
    let batch = query_persistent_graph_multi_symbol(workspace_root, symbols, ALL_GRAPH_QUERY_KINDS);
    PersistentGraphProductProbe {
        availability,
        batch,
        index_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detect_reports_unavailable_for_empty_workspace() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");

        // When
        let availability = detect_persistent_graph(dir.path());

        // Then
        assert!(availability.is_unavailable());
        assert!(!availability.is_available());
        match availability {
            PersistentGraphAvailability::Unavailable { reason } => {
                assert!(reason.contains("no first-party") || reason.contains("graph"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn detect_available_when_codegraph_dir_present() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(dir.path().join(".codegraph")).expect("codegraph");

        // When
        let availability = detect_persistent_graph(dir.path());

        // Then
        assert!(availability.is_available());
        match availability {
            PersistentGraphAvailability::Available { index_root } => {
                assert!(index_root.contains(".codegraph"));
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn query_fails_closed_with_structured_unavailable_without_index() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");

        // When
        let result = query_persistent_graph(dir.path(), &GraphQuery::symbol_def("Coordinator"));

        // Then
        match result {
            GraphQueryResult::Unavailable {
                reason,
                symbol,
                kind,
            } => {
                assert!(!reason.is_empty());
                assert_eq!(symbol, "Coordinator");
                assert_eq!(kind, GraphQueryKind::SymbolDef);
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn batch_query_fails_closed_for_every_entry_without_index() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let queries = [
            GraphQuery::with_kind("Coordinator", GraphQueryKind::SymbolDef),
            GraphQuery::with_kind("Coordinator", GraphQueryKind::Callers),
            GraphQuery::with_kind("RunState", GraphQueryKind::Callees),
        ];

        // When
        let batch = query_persistent_graph_batch(dir.path(), &queries);

        // Then
        assert_eq!(batch.results.len(), 3);
        for (query, result) in queries.iter().zip(batch.results.iter()) {
            match result {
                GraphQueryResult::Unavailable {
                    reason,
                    symbol,
                    kind,
                } => {
                    assert!(!reason.is_empty());
                    assert_eq!(symbol, &query.symbol);
                    assert_eq!(*kind, query.kind);
                }
                other => panic!("expected Unavailable, got {other:?}"),
            }
        }
        assert_eq!(GraphQueryKind::References.as_str(), "references");
    }

    #[test]
    fn graph_operator_diagnostics_cover_detect_query_and_batch() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let availability = detect_persistent_graph(root);
        let single = query_persistent_graph(
            root,
            &GraphQuery::with_kind("Coordinator", GraphQueryKind::Callers),
        );
        let batch = query_persistent_graph_batch(
            root,
            &[
                GraphQuery::symbol_def("A"),
                GraphQuery::with_kind("B", GraphQueryKind::References),
            ],
        );

        // When
        let summary = batch.summary();

        // Then
        assert!(availability
            .one_line()
            .contains("persistent graph: unavailable"));
        assert!(single.is_unavailable());
        assert!(single.one_line().contains("graph query unavailable"));
        assert!(single.one_line().contains("callers"));
        assert!(single.one_line().contains("Coordinator"));
        assert_eq!(
            summary,
            GraphQueryBatchSummary {
                total: 2,
                unavailable: 2,
                hits: 0,
                hit_results: 0,
            }
        );
        assert!(summary.all_unavailable());
        assert!(summary.one_line().contains("2 unavailable"));
    }

    #[test]
    fn multi_symbol_batch_without_index_is_all_unavailable() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let symbols = ["(probe)", "(probe-alt)", "(probe-module)"];

        // When
        let multi = query_persistent_graph_multi_symbol(root, &symbols, ALL_GRAPH_QUERY_KINDS);

        // Then: 3 symbols × 4 kinds = 12 structured unavailable results
        assert_eq!(multi.results.len(), 12);
        let summary = multi.summary();
        assert_eq!(summary.total, 12);
        assert_eq!(summary.unavailable, 12);
        assert!(summary.all_unavailable());
        for result in &multi.results {
            assert!(result.is_unavailable());
        }
    }

    #[test]
    fn build_index_and_query_symbol_def_returns_hits() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let src = root.join("src");
        fs::create_dir_all(&src).expect("src");
        fs::write(
            src.join("lib.rs"),
            "pub fn Coordinator() {}\npub struct RunState {}\n",
        )
        .expect("write");

        // When
        let (index_path, index) = build_persistent_graph_index(root).expect("build");
        let def = query_persistent_graph(root, &GraphQuery::symbol_def("Coordinator"));
        let callers = query_persistent_graph(
            root,
            &GraphQuery::with_kind("Coordinator", GraphQueryKind::Callers),
        );

        // Then: index file side effect
        assert!(index_path.is_file());
        assert!(index_path.ends_with(GRAPH_INDEX_REL));
        assert!(!index.symbols.is_empty());
        assert!(detect_persistent_graph(root).is_available());

        match def {
            GraphQueryResult::Hit { symbol, kind, hits } => {
                assert_eq!(symbol, "Coordinator");
                assert_eq!(kind, GraphQueryKind::SymbolDef);
                assert!(!hits.is_empty());
                assert!(hits.iter().any(|h| h.path.contains("lib.rs")));
            }
            other => panic!("expected Hit, got {other:?}"),
        }
        assert!(callers.is_hit());
        assert_eq!(callers.hit_count(), 0);
        assert!(callers.one_line().contains("callers"));
    }

    #[test]
    fn batch_query_mixed_kinds_resolve_per_entry_with_index() {
        // arrange — workspace with two definitions, indexed
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let src = root.join("src");
        fs::create_dir_all(&src).expect("src");
        fs::write(
            src.join("lib.rs"),
            "pub fn Coordinator() {}\npub struct RunState {}\n",
        )
        .expect("write");
        build_persistent_graph_index(root).expect("build");

        // act — one batch interleaving a supported and an unsupported query kind
        let batch = query_persistent_graph_batch(
            root,
            &[
                GraphQuery::symbol_def("Coordinator"),
                GraphQuery::with_kind("Coordinator", GraphQueryKind::Callers),
                GraphQuery::symbol_def("RunState"),
            ],
        );

        // assert — per-entry outcomes incl. line precision; relationship queries return Hit (possibly empty)
        assert_eq!(batch.results.len(), 3);
        match &batch.results[0] {
            GraphQueryResult::Hit { hits, .. } => {
                assert!(hits
                    .iter()
                    .any(|hit| hit.symbol == "Coordinator" && hit.line == 1));
            }
            other => panic!("expected Hit, got {other:?}"),
        }
        assert!(batch.results[1].is_hit());
        assert_eq!(batch.results[1].hit_count(), 0);
        match &batch.results[2] {
            GraphQueryResult::Hit { hits, .. } => {
                assert!(hits
                    .iter()
                    .any(|hit| hit.symbol == "RunState" && hit.line == 2));
            }
            other => panic!("expected Hit, got {other:?}"),
        }
    }

    #[test]
    fn product_probe_builds_index_and_returns_symbol_def_hits() {
        // arrange
        // act
        // assert
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("src")).expect("src");
        fs::write(
            root.join("src/mod.rs"),
            "pub fn alpha() {}\npub fn beta() {}\n",
        )
        .expect("write");

        // When
        let probe = probe_persistent_graph_product(root, &["alpha", "beta", "missing"]);

        // Then
        assert!(probe.availability.is_available());
        assert!(probe.index_path.as_ref().is_some_and(|p| p.is_file()));
        let summary = probe.summary();
        assert_eq!(summary.total, 12); // 3 symbols × 4 kinds
        assert!(summary.hit_results >= 2);
        assert!(summary.hits >= 2);
        assert!(!summary.all_unavailable());

        let alpha_def = probe
            .batch
            .results
            .iter()
            .find(|r| match r {
                GraphQueryResult::Hit { symbol, kind, .. } => {
                    symbol == "alpha" && *kind == GraphQueryKind::SymbolDef
                }
                _ => false,
            })
            .expect("alpha symbol_def");
        assert!(alpha_def.is_hit());
        assert!(alpha_def.hit_count() >= 1);
    }

    #[test]
    fn build_index_and_query_relationships_returns_real_edges() {
        // arrange — workspace where beta calls alpha
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let src = root.join("src");
        fs::create_dir_all(&src).expect("src");
        fs::write(
            src.join("lib.rs"),
            "pub fn alpha() {}\npub fn beta() {\n    alpha();\n}\n",
        )
        .expect("write");

        // act — build index and query callers/callees/references
        let (_path, index) = build_persistent_graph_index(root).expect("build");
        let callers_of_alpha = query_persistent_graph(
            root,
            &GraphQuery::with_kind("alpha", GraphQueryKind::Callers),
        );
        let callees_of_beta = query_persistent_graph(
            root,
            &GraphQuery::with_kind("beta", GraphQueryKind::Callees),
        );
        let refs_of_alpha = query_persistent_graph(
            root,
            &GraphQuery::with_kind("alpha", GraphQueryKind::References),
        );

        // assert — index has edges
        assert!(!index.edges.is_empty(), "edges should not be empty");
        assert!(
            index
                .edges
                .iter()
                .any(|e| e.caller == "beta" && e.callee == "alpha"),
            "expected edge beta->alpha, got edges: {:?}",
            index.edges
        );

        // assert — callers of alpha returns beta
        match &callers_of_alpha {
            GraphQueryResult::Hit { symbol, kind, hits } => {
                assert_eq!(symbol, "alpha");
                assert_eq!(*kind, GraphQueryKind::Callers);
                assert!(
                    hits.iter().any(|h| h.symbol == "beta"),
                    "expected caller beta, got hits: {:?}",
                    hits
                );
            }
            other => panic!("expected Hit for callers, got {other:?}"),
        }

        // assert — callees of beta returns alpha
        match &callees_of_beta {
            GraphQueryResult::Hit { symbol, kind, hits } => {
                assert_eq!(symbol, "beta");
                assert_eq!(*kind, GraphQueryKind::Callees);
                assert!(
                    hits.iter().any(|h| h.symbol == "alpha"),
                    "expected callee alpha, got hits: {:?}",
                    hits
                );
            }
            other => panic!("expected Hit for callees, got {other:?}"),
        }

        // assert — references of alpha returns beta
        match &refs_of_alpha {
            GraphQueryResult::Hit { symbol, kind, hits } => {
                assert_eq!(symbol, "alpha");
                assert_eq!(*kind, GraphQueryKind::References);
                assert!(
                    hits.iter().any(|h| h.symbol == "beta"),
                    "expected reference from beta, got hits: {:?}",
                    hits
                );
            }
            other => panic!("expected Hit for references, got {other:?}"),
        }
    }
}
