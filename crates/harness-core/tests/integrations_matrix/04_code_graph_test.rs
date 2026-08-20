// ---------------------------------------------------------------------------
// Code graph family
// ---------------------------------------------------------------------------

fn seed_graph_workspace(ws: &Path) {
    let src = ws.join("src");
    fs::create_dir_all(&src).unwrap_or_abort();
    fs::write(
        src.join("lib.rs"),
        "pub fn alpha() {}\npub struct Beta {}\n",
    )
    .unwrap_or_abort();
}

#[test]
fn code_graph_boundary_e2e_build_and_query_symbol_def_returns_hits() {
    // arrange — a workspace with source files
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);

    // act — the index is built and queried
    let (index_path, index) = build_persistent_graph_index(ws).expect("build");
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );

    // assert — the index is written and the query returns real hits
    assert!(index_path.is_file(), "index must be written");
    assert_eq!(index.symbols.len(), 2);
    assert!(result.is_hit(), "query must hit: {result:?}");
    assert!(result.hit_count() >= 1, "must have at least one hit");
}

#[test]
fn code_graph_bad_input_empty_symbol_rejected_by_query() {
    // arrange — a workspace with a built index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);
    build_persistent_graph_index(ws).expect("build");

    // act — a query with an empty symbol is made
    let result = query_persistent_graph(ws, &GraphQuery::with_kind("", GraphQueryKind::SymbolDef));

    // assert — the result is a Hit with zero hits (honest empty, not unavailable)
    assert!(result.is_hit(), "empty symbol should still hit: {result:?}");
    assert_eq!(result.hit_count(), 0);
}

#[test]
fn code_graph_permission_denial_query_without_index_fails_closed_without_writing() {
    // arrange — an empty workspace with no index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();

    // act — a query is made without an index
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );

    // assert — the result is Unavailable and no index was created (read-only)
    assert!(result.is_unavailable(), "must be unavailable: {result:?}");
    assert!(
        !ws.join(".agent-harness/code-graph-index.json").exists(),
        "query must not create an index"
    );
}

#[test]
fn code_graph_process_failure_corrupt_index_returns_unavailable() {
    // arrange — a workspace with a corrupt index file
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    let ah = ws.join(".agent-harness");
    fs::create_dir_all(&ah).unwrap_or_abort();
    fs::write(ah.join("code-graph-index.json"), "not valid json {{{").unwrap_or_abort();

    // act — a query is made against the corrupt index
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );

    // assert — the result is Unavailable (fail closed on parse error)
    assert!(
        result.is_unavailable(),
        "corrupt index must be unavailable: {result:?}"
    );
}

#[test]
fn code_graph_cancellation_restart_rebuild_index_after_corruption_recovers() {
    // arrange — a workspace with a corrupt index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);
    let ah = ws.join(".agent-harness");
    fs::create_dir_all(&ah).unwrap_or_abort();
    fs::write(ah.join("code-graph-index.json"), "corrupt").unwrap_or_abort();

    // act — the index is rebuilt (overwriting the corrupt one)
    let (index_path, index) = build_persistent_graph_index(ws).expect("rebuild");

    // assert — the index is valid and queries succeed
    assert!(index_path.is_file());
    assert_eq!(index.symbols.len(), 2);
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );
    assert!(result.is_hit(), "query must hit after rebuild: {result:?}");
    assert!(result.hit_count() >= 1);
}

#[test]
fn code_graph_redaction_query_result_does_not_contain_secret_patterns() {
    // arrange — a workspace with a built index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);
    build_persistent_graph_index(ws).expect("build");

    // act — the query result is serialized
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );
    let json = serde_json::to_string(&result).expect("serialize");

    // assert — the result JSON does not contain secret-like patterns
    assert!(!json.contains("Bearer "), "must not contain bearer tokens");
    assert!(!json.contains("sk-"), "must not contain API key patterns");
    assert!(!json.contains("password"), "must not contain password");
    assert!(json.contains("alpha"), "must contain the queried symbol");
}

