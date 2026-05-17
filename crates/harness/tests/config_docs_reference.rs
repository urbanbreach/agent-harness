use std::collections::BTreeSet;

use harness_core::command_registry::{CommandAction, CommandRegistry};
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

fn markdown_section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let mut section = doc
        .split(&format!("{heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("missing `{heading}` section"));
    if let Some((current, _rest)) = section.split_once("\n## ") {
        section = current;
    }
    section
}

fn script_usage_modes(script: &str) -> BTreeSet<String> {
    let modes = script
        .split("Modes:\n")
        .nth(1)
        .expect("test lane usage should include Modes section")
        .split("\nOptions:")
        .next()
        .expect("test lane usage should include Options section");

    modes
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mode = trimmed.split_whitespace().next()?;
            if mode == "help" || mode.starts_with("--") {
                return None;
            }
            Some(mode.to_string())
        })
        .collect()
}

fn run_stage_commands(script: &str, function_name: &str) -> BTreeSet<String> {
    let start = script
        .find(&format!("{function_name}() {{"))
        .unwrap_or_else(|| panic!("missing {function_name}"));
    let after_start = &script[start..];
    let body = after_start
        .split_once("\n}")
        .unwrap_or_else(|| panic!("missing {function_name} body terminator"))
        .0;

    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("run_stage ") {
                return None;
            }
            let (_prefix, command) = trimmed
                .split_once("\"$repo_root\"")
                .unwrap_or_else(|| panic!("run_stage missing repo root in line: {trimmed}"));
            let command = command
                .trim()
                .strip_suffix("|| true")
                .unwrap_or(command.trim())
                .trim();
            Some(command.to_string())
        })
        .collect()
}

fn documented_stage_commands(section: &str) -> BTreeSet<String> {
    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let command = trimmed.strip_prefix("- `")?.strip_suffix('`')?;
            if command.starts_with("cargo ")
                || command.starts_with("env ")
                || command.starts_with("python3 ")
            {
                Some(command.to_string())
            } else {
                None
            }
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
        "research-mission",
        "wiki",
        "workflow_contract_registry",
        "workflow_context_snapshot",
        "workflow_runtime_config",
        "workflow_closeout_policy",
        "workflow_closeout_readiness",
        "workflow_simulator",
        "workflow_stale_work_loop",
        "evidence.simulated_tool_result",
        "evidence.plan_consensus",
        "evidence.goal_ledger",
        "prompt-to-artifact completion audit",
        "harness workflow run/status/signoff/cancel/dossier/snapshot/plan-consensus/goal/mission/wiki/init",
        "harness workflow goal create/status/checkpoint/list/read",
        "harness workflow mission init/run/status/read",
        "harness workflow wiki add/read/list/query/lint/refresh/delete",
        "harness workflow plan-consensus",
        "runtime.workflow",
        "runtime.workflow.closeout",
        "workflow.closeout.default",
        "closeout.policies.<policy_id>.requireExportArtifact",
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
fn readme_lists_registered_workflow_slash_commands_and_aliases() {
    let root = repo_root();
    let readme = std::fs::read_to_string(root.join("README.md")).expect("read README.md");

    for command in CommandRegistry::builtins().commands() {
        if !matches!(command.action, CommandAction::WorkflowIntent { .. }) {
            continue;
        }
        let slash_command = format!("/{}", command.name);
        assert!(
            readme.contains(&slash_command),
            "README.md missing workflow slash command {slash_command}"
        );
        for alias in command.aliases {
            let slash_alias = format!("/{alias}");
            assert!(
                readme.contains(&slash_alias),
                "README.md missing workflow slash alias {slash_alias}"
            );
        }
    }

    for expected in [
        "harness workflow run/status/signoff/cancel/dossier/snapshot/plan-consensus/goal/mission/wiki/init",
        "Run Dossier",
        "typed UI intents",
        "rather than rerunning hooks or tools",
    ] {
        assert!(
            readme.contains(expected),
            "README.md missing workflow command docs anchor: {expected}"
        );
    }
}

#[test]
fn testing_docs_and_readme_track_test_lane_runner_modes_and_stage_commands() {
    let root = repo_root();
    let script =
        std::fs::read_to_string(root.join("scripts/test-lanes.sh")).expect("read test-lanes.sh");
    let testing = std::fs::read_to_string(root.join("docs/testing.md")).expect("read testing.md");
    let readme = std::fs::read_to_string(root.join("README.md")).expect("read README.md");

    for mode in script_usage_modes(&script) {
        let command = format!("scripts/test-lanes.sh {mode}");
        assert!(
            testing.contains(&command),
            "docs/testing.md missing test lane command `{command}`"
        );
        assert!(
            readme.contains(&command),
            "README.md missing test lane command `{command}`"
        );
    }

    for (mode, function_name, heading) in [
        ("fast", "run_fast", "## Fast default developer lane"),
        ("integration", "run_integration", "## Integration CI lane"),
        (
            "signoff-pty",
            "run_signoff_pty",
            "## Deterministic signoff PTY lane",
        ),
        (
            "signoff-live",
            "run_signoff_live",
            "## Live provider opt-in lane",
        ),
        (
            "signoff-browser",
            "run_signoff_browser",
            "## Browser/media signoff lane",
        ),
        (
            "signoff-native",
            "run_signoff_native",
            "## Native visual lane",
        ),
    ] {
        let script_commands = run_stage_commands(&script, function_name);
        let doc_commands = documented_stage_commands(markdown_section(&testing, heading));
        assert_eq!(
            doc_commands, script_commands,
            "docs/testing.md stage command list drifted from scripts/test-lanes.sh for {mode}"
        );
    }

    for expected in [
        "harness workflow dossier export --json --run-dir <run>",
        "projection-only",
        "intake restart/replay",
        "HARNESS_LIVE_PROXY=1",
        "HARNESS_NATIVE_VISUAL=1",
        "HARNESS_BROWSER_SIGNOFF=1",
    ] {
        assert!(
            testing.contains(expected),
            "docs/testing.md missing workflow/signoff closeout anchor: {expected}"
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
