use std::collections::BTreeSet;

use harness_core::config::{harness_schema_pretty_json, harness_tui_schema_pretty_json};

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

    let tui_schema =
        harness_tui_schema_pretty_json().expect("tui schema generation should succeed");
    let tui_schema: serde_json::Value = serde_json::from_str(&tui_schema).expect("tui schema json");
    let tui_keys: BTreeSet<String> = tui_schema["properties"]
        .as_object()
        .expect("tui schema root properties")
        .keys()
        .cloned()
        .collect();

    let doc_path = repo_root().join("docs/config.md");
    let doc = std::fs::read_to_string(&doc_path).expect("read docs/config.md");

    let documented_runtime_keys = documented_table_keys(&doc, "Runtime top-level keys");
    let documented_tui_keys = documented_table_keys(&doc, "TUI top-level keys");

    assert_eq!(
        documented_runtime_keys, runtime_keys,
        "runtime key table drifted"
    );
    assert_eq!(documented_tui_keys, tui_keys, "tui key table drifted");
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
        "experimental upstream-compatible flag",
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

#[test]
fn config_docs_capture_workflow_contract_registry() {
    let root = repo_root();
    let doc = std::fs::read_to_string(root.join("docs/config.md")).expect("read docs/config.md");

    for expected in [
        "### Workflow contract registry",
        "harness-core::workflow_registry",
        "harness-core::command_registry",
        "workflow-run",
        "workflow-status",
        "workflow-signoff",
        "workflow_contract_registry",
        "workflow_context_snapshot",
        "workflow_runtime_config",
        "workflow_simulator",
        "workflow_stale_work_loop",
        "evidence.simulated_tool_result",
        "evidence.plan_consensus",
        "evidence.goal_ledger",
        "prompt-to-artifact completion audit",
        "harness workflow run/status/signoff/cancel/dossier/snapshot/plan-consensus/goal/init",
        "harness workflow goal create/status/checkpoint/list/read",
        "harness workflow plan-consensus",
        "runtime.workflow",
        "init --check",
        "init --apply",
        "harness workflow snapshot write",
        "must not execute shell tools",
        "docs/omx-workflow-slice-spec.md",
    ] {
        assert!(
            doc.contains(expected),
            "docs/config.md missing workflow contract registry anchor: {expected}"
        );
    }
}

#[test]
fn workflow_slice_docs_capture_g001_ssot_and_drift_guard_contract() {
    let root = repo_root();
    let doc = std::fs::read_to_string(root.join("docs/omx-workflow-slice-spec.md"))
        .expect("read docs/omx-workflow-slice-spec.md");

    for expected in [
        "## Workstream J: Setup, doctor, and SSOT verification",
        "Treat first-party workflow commands, aliases, prompts, evidence categories, doctor checks, and docs links as a small single source of truth early in the slice.",
        "Manifest/registry verification tests for first-party commands, aliases, evidence categories, prompts, and doctor/docs links.",
        "workflow commands registered",
        "aliases present or explicitly disabled",
        "First-party command/alias/evidence/doctor/docs SSOT drift guard.",
        "Exit criteria: implementer can state what is reused, what is wrapped with workflow metadata, what is hardened later, and what is deferred.",
    ] {
        assert!(
            doc.contains(expected),
            "docs/omx-workflow-slice-spec.md missing G001 SSOT/drift anchor: {expected}"
        );
    }
}
