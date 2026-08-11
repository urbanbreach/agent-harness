mod common;

use common::repo_root;
use harness::UnwrapOrAbort;

#[test]
fn config_docs_capture_harness_contract_and_migration_boundary() {
    // arrange
    let doc_path = repo_root().join("docs/configuration/config.md");

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
fn config_docs_capture_generic_agent_and_named_subagents() {
    // arrange
    let root = repo_root();

    // act
    let doc = std::fs::read_to_string(root.join("docs/configuration/config.md")).unwrap_or_abort();
    let example =
        std::fs::read_to_string(root.join("configs/harness.example.jsonc")).unwrap_or_abort();

    // assert
    for expected in [
        "\"agent\": {",
        "\"default\": { \"variant\": \"high\" }",
        "\"explore\": {}",
        "\"general\": {}",
        "\"librarian\": {}",
        ".agent-harness/agents/{default,explore,general,librarian}.md",
    ] {
        assert!(
            doc.contains(expected),
            "docs/config.md missing generic agent contract anchor: {expected}"
        );
    }

    for expected in [
        "task(subagent_type=...)",
        "\"default\": { \"variant\": \"high\" }",
        "\"explore\": {}",
        "\"general\": {}",
        "\"librarian\": {}",
    ] {
        assert!(
            example.contains(expected),
            "configs/harness.example.jsonc missing generic agent contract anchor: {expected}"
        );
    }

    for retired_tool in ["plan_enter", "plan_exit"] {
        assert!(!doc.contains(retired_tool));
        assert!(!example.contains(retired_tool));
    }
}
