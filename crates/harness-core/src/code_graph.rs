//! First-party persistent codebase graph surface.
//!
//! Detects an external `.codegraph` index root and/or a harness-owned simple
//! on-disk symbol index under `.agent-harness/code-graph-index.json`. The simple
//! index can be built from workspace source files and queried for symbol
//! definitions (real filesystem side effects). Callers/callees/references stay
//! fail-closed until a richer graph backend lands.

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

/// On-disk simple symbol index (JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SimpleGraphIndex {
    pub schema: String,
    pub symbols: Vec<GraphSymbolHit>,
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
/// `struct`, `enum`, `trait`, `type`, `class`, `def`) and writes
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
    let index = SimpleGraphIndex {
        schema: "harness-code-graph-index-v1".to_string(),
        symbols,
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
        GraphQueryKind::Callers | GraphQueryKind::Callees | GraphQueryKind::References => {
            GraphQueryResult::Unavailable {
                reason: format!(
                    "simple on-disk index supports symbol_def only; {} not implemented",
                    query.kind.as_str()
                ),
                symbol: query.symbol.clone(),
                kind: query.kind,
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
        assert!(callers.is_unavailable());
        assert!(callers.one_line().contains("callers"));
    }

    #[test]
    fn product_probe_builds_index_and_returns_symbol_def_hits() {
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
}
