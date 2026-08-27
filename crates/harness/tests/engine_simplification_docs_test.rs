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
