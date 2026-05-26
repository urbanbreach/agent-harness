use std::collections::BTreeSet;

use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, public_config_contract,
    PublicConfigAliasScope, PublicConfigKeyStatus,
};

mod common;

use common::repo_root;

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
fn config_docs_runtime_and_tui_keys_match_generated_schemas() {
    let contract = public_config_contract();
    let runtime_schema =
        harness_schema_pretty_json().expect("runtime schema generation should succeed");
    let runtime_schema: serde_json::Value =
        serde_json::from_str(&runtime_schema).expect("runtime schema json");
    let runtime_keys: BTreeSet<String> = runtime_schema["properties"]
        .as_object()
        .expect("runtime schema root properties")
        .keys()
        .cloned()
        .collect();
    let contract_runtime_keys = contract
        .runtime_schema_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();

    let tui_schema =
        harness_tui_schema_pretty_json().expect("tui schema generation should succeed");
    let tui_schema: serde_json::Value = serde_json::from_str(&tui_schema).expect("tui schema json");
    let tui_keys: BTreeSet<String> = tui_schema["properties"]
        .as_object()
        .expect("tui schema root properties")
        .keys()
        .cloned()
        .collect();
    let contract_tui_keys = contract
        .tui_schema_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();

    let doc_path = repo_root().join("docs/config.md");
    let doc = std::fs::read_to_string(&doc_path).expect("read docs/config.md");

    let documented_runtime_keys = documented_table_keys(&doc, "Runtime top-level keys");
    let documented_tui_keys = documented_table_keys(&doc, "TUI top-level keys");
    let contract_documented_runtime_keys = contract
        .runtime_documented_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let contract_documented_tui_keys = contract
        .tui_documented_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();

    assert_eq!(
        runtime_keys, contract_runtime_keys,
        "runtime schema drifted from public config contract"
    );
    assert_eq!(
        tui_keys, contract_tui_keys,
        "tui schema drifted from public config contract"
    );
    assert_eq!(
        documented_runtime_keys, contract_documented_runtime_keys,
        "runtime key table drifted from public config contract"
    );
    assert_eq!(
        documented_tui_keys, contract_documented_tui_keys,
        "tui key table drifted from public config contract"
    );
}

#[test]
fn config_contract_semantic_metadata_matches_docs() {
    // arrange
    let contract = public_config_contract();
    let root = repo_root();
    let doc = std::fs::read_to_string(root.join("docs/config.md")).expect("read docs/config.md");

    // act
    let runtime_key = contract
        .runtime_top_level_keys
        .iter()
        .find(|key| key.name == "runtime")
        .expect("runtime key metadata");
    assert_eq!(runtime_key.status, PublicConfigKeyStatus::Canonical);
    assert!(doc.contains("| `runtime` | Runtime knobs"));

    let small_model_alias = contract
        .runtime_top_level_keys
        .iter()
        .find(|key| key.name == "smallModel")
        .expect("smallModel alias metadata");
    assert_eq!(
        small_model_alias.status,
        PublicConfigKeyStatus::Compatibility
    );
    assert_eq!(small_model_alias.canonical_name, Some("small_model"));
    assert!(doc.contains("compatibility aliases"));

    let server = contract
        .runtime_top_level_keys
        .iter()
        .find(|key| key.name == "server")
        .expect("server metadata");
    assert_eq!(server.status, PublicConfigKeyStatus::UnsupportedActive);
    assert!(doc.contains(
        "`server`, `command`, `plugin`, `share`, `autoshare`, `autoupdate`, `enterprise`"
    ));

    let bash = contract
        .permission_names
        .iter()
        .find(|permission| permission.name == "bash")
        .expect("bash permission metadata");
    assert!(bash.canonical);
    assert!(bash.schema_property);
    assert!(bash.supports_selectors);
    assert!(doc.contains(
        "`bash`, `edit`, `question`, `task`,\n`webfetch`, `websearch`, `codesearch`, and `lsp`"
    ));

    let compaction = contract
        .compaction_knobs
        .iter()
        .find(|knob| knob.canonical_name == "fallback_input_tokens")
        .expect("fallback_input_tokens metadata");
    assert_eq!(compaction.default_value, "32768");
    assert!(compaction.aliases.contains(&"fallbackInputTokens"));
    assert!(doc.contains("| `fallbackInputTokens` / `fallback_input_tokens` | `32768` |"));

    let compaction_aliases = contract
        .runtime_aliases
        .iter()
        .filter(|alias| alias.scope == PublicConfigAliasScope::RuntimeCompaction)
        .map(|alias| (alias.alias, alias.canonical))
        .collect::<BTreeSet<_>>();

    // assert
    assert!(compaction_aliases.contains(&("fallbackInputTokens", "fallback_input_tokens")));
    assert!(compaction_aliases.contains(&("model", "model_ref")));
}

#[test]
fn config_docs_capture_harness_contract_and_migration_boundary() {
    let doc_path = repo_root().join("docs/config.md");
    let doc = std::fs::read_to_string(&doc_path).expect("read docs/config.md");

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
    let root = repo_root();
    let doc = std::fs::read_to_string(root.join("docs/config.md")).expect("read docs/config.md");
    let example = std::fs::read_to_string(root.join("configs/harness.example.jsonc"))
        .expect("read harness example config");

    for expected in [
        "### Plan operator workflow",
        "stable public runtime surface",
        concat!("experimental ", "Open", "Code", " flag"),
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
