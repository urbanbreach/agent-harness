use harness_core::config::ShellAllowlist;
use harness_tools::UnwrapOrAbort;
use harness_tools::{canonical_tool_id_for, coordinator_registry, native_tool_catalog_entries};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[path = "support/baseline_tools_parity_inventory_support.rs"]
mod baseline_tools_parity_inventory;

#[derive(Debug)]
struct DocumentedToolRow {
    permission: String,
    replay_and_artifacts: String,
    notes: String,
}

#[test]
fn coordinator_registry_exposes_single_native_tool_surface() {
    let registry = coordinator_registry(ShellAllowlist::default());

    for tool_id in [
        "bash",
        "ast_grep_replace",
        "ast_grep_search",
        "background_cancel",
        "background_output",
        "batch",
        "lsp.rename",
        "codesearch",
        "edit",
        "github.issue",
        "github.pull_request",
        "glob",
        "grep",
        "invalid",
        "list",
        "lsp",
        "plan_enter",
        "plan_exit",
        "question",
        "read",
        "session_info",
        "session_list",
        "session_read",
        "session_search",
        "skill",
        "shell.run",
        "task",
        "todoread",
        "todowrite",
        "webfetch",
        "websearch",
        "write",
        "apply_patch",
    ] {
        assert!(
            registry.get(tool_id).is_some(),
            "missing canonical tool {tool_id}"
        );
        assert_eq!(canonical_tool_id_for(tool_id), Some(tool_id));
    }

    assert!(registry.get("edit_compat").is_none());
    assert!(registry.get("edit.hashline_apply").is_none());
    assert!(registry.get("edit.hashline_scan").is_none());
    assert!(registry.get("fs.write").is_none());
    assert!(registry.get("patch").is_none());

    for legacy_tool_id in [
        "agent.spawn",
        "code.lsp",
        "code.lsp.rename",
        "fs.glob",
        "fs.grep",
        "fs.ls",
        "fs.read",
        "search.code",
        "search.web",
        "skill.load",
        "todo.read",
        "todo.write",
        "tool.batch",
        "tool.invalid",
        "user.question",
        "web.fetch",
    ] {
        assert!(
            registry.get(legacy_tool_id).is_none(),
            "legacy tool should not be registered: {legacy_tool_id}"
        );
    }

    let catalog_ids = native_tool_catalog_entries(&registry)
        .into_iter()
        .map(|entry| entry.canonical_id)
        .collect::<BTreeSet<_>>();
    let registry_ids = registry.tool_ids().into_iter().collect::<BTreeSet<_>>();
    assert_eq!(
        catalog_ids, registry_ids,
        "native tool catalog must mirror the registered native surface"
    );

    let doc_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/native-tool-catalog.md");
    let doc = std::fs::read_to_string(&doc_path).unwrap_or_abort();
    let doc_ids = documented_tool_ids(&doc);
    assert_eq!(
        doc_ids, registry_ids,
        "docs/native-tool-catalog.md must list every registered native tool id exactly once"
    );
}

#[test]
fn native_tool_catalog_rows_include_permission_alias_and_replay_metadata() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let catalog = native_tool_catalog_entries(&registry);
    let doc_path = repo_path("docs/native-tool-catalog.md");
    let doc = std::fs::read_to_string(&doc_path).unwrap_or_abort();
    let rows = documented_tool_rows(&doc);

    for entry in catalog {
        let row = rows
            .get(&entry.canonical_id)
            .unwrap_or_else(|| panic!("missing doc row for {}", entry.canonical_id));
        match entry.permission_kind.as_deref() {
            Some(permission) => assert!(
                row.permission.contains(&format!("`{permission}`")) || row.permission == permission,
                "{} doc permission `{}` does not mention runtime permission `{permission}`",
                entry.canonical_id,
                row.permission
            ),
            None => assert_eq!(
                row.permission,
                "none",
                "{} should document permission as none",
                entry.canonical_id // act
            ),
        }

        // assert
        assert!(
            !entry.description_summary.trim().is_empty(),
            "{} missing runtime description summary",
            entry.canonical_id
        );
        assert!(
            matches!(entry.schema_status.as_str(), "strict" | "open"),
            "{} has unexpected schema status {}",
            entry.canonical_id,
            entry.schema_status
        );
        assert!(
            !row.replay_and_artifacts.trim().is_empty()
                && !row.replay_and_artifacts.contains("TBD"),
            "{} doc row must describe replay/artifact behavior",
            entry.canonical_id
        );
        for alias in &entry.aliases {
            assert!(
                row.notes.contains(alias),
                "{} runtime alias `{alias}` is missing from docs notes `{}`",
                entry.canonical_id,
                row.notes
            );
        }
    }
}

#[test]
fn bash_safety_guidance_and_ast_grep_replace_catalog_match_runtime_sources() {
    // arrange
    // act
    let registry = coordinator_registry(ShellAllowlist::default());
    // assert
    assert!(registry.get("ast_grep_search").is_some());
    let replace = registry.get("ast_grep_replace").unwrap_or_abort();
    assert_eq!(
        replace.capability(),
        harness_core::tool::ToolCapability::EditFs
    );

    let doc = std::fs::read_to_string(repo_path("docs/native-tool-catalog.md")).unwrap_or_abort();
    let doctor = [
        std::fs::read_to_string(repo_path("crates/harness/src/doctor.rs")).unwrap_or_abort(),
        std::fs::read_to_string(repo_path("crates/harness/src/doctor/checks.rs")).unwrap_or_abort(),
    ]
    .join("\n");
    let shell_run = std::fs::read_to_string(repo_path("crates/harness-tools/src/shell_run.rs"))
        .unwrap_or_abort();
    let shell_safety =
        std::fs::read_to_string(repo_path("crates/harness-tools/src/shell_safety.rs"))
            .unwrap_or_abort();

    assert!(shell_run.contains("const DEFAULT_SHELL_TIMEOUT_MS: u64 = 120_000;"));
    assert!(shell_run.contains("const SHELL_OUTPUT_INLINE_LINE_LIMIT: usize = 2_000;"));
    assert!(shell_run.contains("const SHELL_OUTPUT_INLINE_BYTE_LIMIT: usize = 51_200;"));
    assert!(shell_safety.contains(
        "const ALLOWED_SHELL_BUILTINS: &[&str] = &[\"echo\", \"false\", \"printf\", \"pwd\", \"test\", \"true\", \"[\"];"
    ));

    for text in [
        &doc,
        &read_agent_prompt("build"),
        &read_agent_prompt("plan"),
    ] {
        assert!(text.contains("120000 ms"));
        assert!(text.contains("2000 lines"));
        assert!(text.contains("51200 bytes"));
        for command in ["find", "grep", "rg", "cat", "head", "tail", "sed", "awk"] {
            assert!(
                text.contains(command),
                "bash guidance is missing command reference `{command}` in:\n{text}"
            );
        }
    }

    assert!(doc.contains("`ast_grep_replace`"));
    assert!(doc.contains("Defaults to dry-run"));
    assert!(documented_tool_ids(&doc).contains("ast_grep_replace"));
    assert!(doctor.contains("\"ast_grep_replace\": \"shipped_edit_safe\""));
    assert!(doc.contains("`ast_grep_replace` maps to `edit`"));
}

#[test]
fn changed_tool_schemas_and_docs_reflect_parity_params() {
    // arrange
    // act
    let registry = coordinator_registry(ShellAllowlist::default());
    let doc = std::fs::read_to_string(repo_path("docs/native-tool-catalog.md")).unwrap_or_abort();

    // assert
    let grep_schema = registry
        .get("grep")
        .unwrap_or_abort()
        .parameters_json_schema();
    let grep_props = grep_schema
        .get("properties")
        .expect("grep schema has properties");
    assert!(
        grep_props.get("output_mode").is_some(),
        "grep schema missing output_mode"
    );
    assert!(
        grep_props.get("head_limit").is_some(),
        "grep schema missing head_limit"
    );
    assert!(
        doc.contains("output_mode") && doc.contains("head_limit"),
        "docs missing grep params"
    );

    assert!(
        doc.contains("modification time"),
        "docs missing glob mtime note"
    );

    let bg_cancel_schema = registry
        .get("background_cancel")
        .unwrap_or_abort()
        .parameters_json_schema();
    let bg_cancel_props = bg_cancel_schema
        .get("properties")
        .expect("background_cancel schema has properties");
    assert!(
        bg_cancel_props.get("all").is_some(),
        "background_cancel schema missing all"
    );
    assert!(
        doc.contains("`all: true`"),
        "docs missing background_cancel all param"
    );

    let task_schema = registry
        .get("task")
        .unwrap_or_abort()
        .parameters_json_schema();
    for field in ["description", "run_in_background", "load_skills"] {
        let is_required = task_schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().any(|v| v.as_str() == Some(field)))
            .unwrap_or(false);
        assert!(!is_required, "task schema should not require {field}");
    }
    assert!(
        doc.contains("optional with defaults"),
        "docs missing task optional args note"
    );

    let bg_output_schema = registry
        .get("background_output")
        .unwrap_or_abort()
        .parameters_json_schema();
    let bg_output_props = bg_output_schema
        .get("properties")
        .expect("background_output schema has properties");
    for field in [
        "full_session",
        "include_thinking",
        "message_limit",
        "since_message_id",
        "include_tool_results",
        "thinking_max_chars",
        "from_end",
    ] {
        assert!(
            bg_output_props.get(field).is_some(),
            "background_output schema missing {field}"
        );
    }
    assert!(
        doc.contains("full_session") && doc.contains("include_thinking"),
        "docs missing background_output rich retrieval params"
    );

    let sr_schema = registry
        .get("session_read")
        .unwrap_or_abort()
        .parameters_json_schema();
    let sr_props = sr_schema
        .get("properties")
        .expect("session_read schema has properties");
    assert!(
        sr_props.get("includeTodos").is_some(),
        "session_read schema missing includeTodos"
    );
    assert!(
        sr_props.get("fromEnd").is_some(),
        "session_read schema missing fromEnd"
    );
    assert!(
        doc.contains("include_todos") && doc.contains("from_end"),
        "docs missing session_read params"
    );

    let lsp_schema = registry
        .get("lsp")
        .unwrap_or_abort()
        .parameters_json_schema();
    let lsp_props = lsp_schema
        .get("properties")
        .expect("lsp schema has properties");
    let lsp_op_enum = lsp_props
        .get("operation")
        .and_then(|v| v.get("enum"))
        .and_then(|v| v.as_array())
        .expect("lsp schema has operation enum");
    assert!(
        lsp_op_enum
            .iter()
            .any(|v| v.as_str() == Some("installDecision")),
        "lsp schema missing installDecision operation"
    );
    assert!(
        doc.contains("installDecision"),
        "docs missing lsp installDecision note"
    );
}

fn repo_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn documented_tool_ids(doc: &str) -> BTreeSet<String> {
    doc.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = line.strip_prefix("| `")?;
            let (tool_id, _) = rest.split_once('`')?;
            Some(tool_id.to_string())
        })
        .collect()
}

fn documented_tool_rows(doc: &str) -> BTreeMap<String, DocumentedToolRow> {
    doc.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = line.strip_prefix("| `")?;
            let (tool_id, rest) = rest.split_once('`')?;
            let columns = rest
                .trim_start_matches(" |")
                .trim_end_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>();
            (columns.len() == 4).then(|| {
                (
                    tool_id.to_string(),
                    DocumentedToolRow {
                        permission: columns[0].to_string(),
                        replay_and_artifacts: columns[2].to_string(),
                        notes: columns[3].to_string(),
                    },
                )
            })
        })
        .collect()
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn read_agent_prompt(profile: &str) -> String {
    std::fs::read_to_string(repo_path(&format!(".agent-harness/agents/{profile}.md")))
        .unwrap_or_else(|_| panic!("abort"))
}
