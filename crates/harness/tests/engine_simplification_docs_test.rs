use std::fs;
use std::path::Path;

use harness::UnwrapOrAbort;

fn read_doc(path: &str) -> String {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_abort();
    fs::read_to_string(workspace.join(path)).unwrap_or_abort()
}

#[test]
fn engine_docs_name_canonical_projection_bounded_index_and_compatibility_matrix() {
    // arrange
    let target = read_doc("docs/architecture/engine-target.md");
    let sessions = read_doc("docs/architecture/sessions-and-replay.md");
    let migration = read_doc("docs/architecture/engine-migration.md");

    // act
    let combined = format!("{target}\n{sessions}\n{migration}");

    // assert
    for required in [
        "CanonicalSessionProjection",
        ".session-history-index-v1.json",
        "sessions search",
        "sessions rebuild-index",
        "ephemeral TUI overlay",
        "compatibility-only",
        "removed",
    ] {
        assert!(
            combined.contains(required),
            "engine docs must contain `{required}`"
        );
    }
    assert!(
        combined.contains("CompactionRequested") && combined.contains("SessionCompaction"),
        "docs must distinguish legacy decode variants from Compaction V2"
    );
}

#[test]
fn engine_docs_describe_shipped_settled_read_and_advisory_index_boundaries() {
    // arrange
    let documents = [
        read_doc("docs/architecture/engine-inventory.md"),
        read_doc("docs/architecture/engine-target.md"),
        read_doc("docs/architecture/engine-migration.md"),
        read_doc("docs/architecture/architecture.md"),
        read_doc("docs/architecture/sessions-and-replay.md"),
    ]
    .join("\n");
    let required_contracts = [
        "one authoritative composed `CanonicalSessionProjection` facade per journal load or settlement",
        "focused pure reducers; it is neither a monolithic reducer nor a single physical pass",
        "history-index row is updated after each successful durable commit",
        "warm journal opens: 0",
        "timestamp, run id, and run-directory bytes",
        "invalid or stale cursor fails closed",
        "`EventV1` variant count remains 39",
        "only inside the read-only `session::legacy` compatibility boundary",
        "No live-provider assertion is made without configured credentials and a live signoff",
    ];

    // act
    let missing = required_contracts
        .iter()
        .find(|contract| !documents.contains(**contract));

    // assert
    assert_eq!(
        missing, None,
        "engine docs must state every shipped boundary"
    );
}
