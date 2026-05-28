use harness_core::config::ShellAllowlist;
use harness_tools::{canonical_tool_id_for, coordinator_registry, native_tool_catalog_entries};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn coordinator_registry_exposes_single_native_tool_surface() {
    let registry = coordinator_registry(ShellAllowlist::default());

    for tool_id in [
        "bash",
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
        "team_create",
        "team_delete",
        "team_list",
        "team_send_message",
        "team_shutdown_approve",
        "team_shutdown_reject",
        "team_shutdown_request",
        "team_status",
        "team_task_create",
        "team_task_get",
        "team_task_list",
        "team_task_update",
        "todoread",
        "todowrite",
        "webfetch",
        "websearch",
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
    assert!(registry.get("write").is_none());
    assert!(registry.get("apply_patch").is_none());
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
    let doc = std::fs::read_to_string(&doc_path).expect("read native tool catalog doc");
    let doc_ids = documented_tool_ids(&doc);
    assert_eq!(
        doc_ids, registry_ids,
        "docs/native-tool-catalog.md must list every registered native tool id exactly once"
    );
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
