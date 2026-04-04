use std::collections::BTreeSet;
use std::path::PathBuf;

use harness_core::config::harness_schema_pretty_json;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn documented_table_keys(doc: &str, heading: &str) -> BTreeSet<String> {
    let mut section = doc
        .split(&format!("## {heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("missing `{heading}` section"));
    if let Some((current, _rest)) = section.split_once("\n## ") {
        section = current;
    }

    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("| `") {
                return None;
            }
            let after_tick = &trimmed[3..];
            let key = after_tick.split('`').next()?;
            Some(key.to_string())
        })
        .collect()
}

#[test]
fn config_docs_top_level_keys_match_generated_schema() {
    let schema = harness_schema_pretty_json().expect("schema generation should succeed");
    let schema: serde_json::Value = serde_json::from_str(&schema).expect("schema json");
    let schema_keys: BTreeSet<String> = schema["properties"]
        .as_object()
        .expect("schema root properties")
        .keys()
        .cloned()
        .collect();

    let doc_path = repo_root().join("docs/config.md");
    let doc = std::fs::read_to_string(&doc_path).expect("read docs/config.md");

    let top_level_keys = documented_table_keys(&doc, "Top-level keys");
    let schema_reference_keys = documented_table_keys(&doc, "`HarnessConfig` schema reference");

    assert_eq!(top_level_keys, schema_keys, "top-level key table drifted");
    assert_eq!(
        schema_reference_keys, schema_keys,
        "schema reference drifted"
    );

    for key in ["hooks", "skills", "lsp"] {
        assert!(
            doc.contains(&format!("\"{key}\":")),
            "expected `{key}` example"
        );
    }
}
