mod common;

use common::repo_root;
use harness::UnwrapOrAbort;

#[test]
fn config_docs_capture_harness_contract_and_migration_boundary() {
    // arrange
    let doc_path = repo_root().join("docs/config.md");

    // act
    let doc = std::fs::read_to_string(&doc_path).unwrap_or_abort();

    // assert
    assert!(doc.contains("## Public contract summary"));
    assert!(doc.contains("$XDG_CONFIG_HOME/harness/harness.jsonc"));
    assert!(doc.contains("HARNESS_CONFIG"));
    assert!(doc.contains("HARNESS_CONFIG_CONTENT"));
    assert!(doc.contains("HARNESS_TUI_CONFIG"));
    assert!(doc.contains(".agent-harness/agents"));
    assert!(doc.contains("harness.json"));
    assert!(doc.contains("harness.jsonc"));
    assert!(doc.contains("compatibility inputs"));
    assert!(doc.contains("Unsupported top-level areas"));
}

#[test]
fn config_docs_capture_plan_operator_workflow_and_guardrails() {
    // arrange
    let root = repo_root();

    // act
    let doc = std::fs::read_to_string(root.join("docs/config.md")).unwrap_or_abort();
    let example =
        std::fs::read_to_string(root.join("configs/harness.example.jsonc")).unwrap_or_abort();

    // assert
    for expected in [
        "### Plan operator workflow",
        "stable public runtime surface",
        "experimental compatibility flag",
        "Build call `plan_enter`",
        ".agent-harness/plans/<run>.md",
        "Plan calls `plan_exit`",
        "restricted to `explore`",
        "cannot launch\n   `general`, `build`, or user-defined writer subagents",
        "Approving that prompt switches\n   back to Build",
        "declining leaves the session in Plan",
    ] {
        assert!(
            doc.contains(expected),
            "docs/config.md missing Plan workflow anchor: {expected}"
        );
    }

    for expected in [
        "Stable read-only planning lane",
        ".agent-harness/plans/<run>.md",
        "plan_exit",
        "continuing implementation in Build",
    ] {
        assert!(
            example.contains(expected),
            "configs/harness.example.jsonc missing Plan comment anchor: {expected}"
        );
    }
}
