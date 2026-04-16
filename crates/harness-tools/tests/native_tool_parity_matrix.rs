use harness_core::config::ShellAllowlist;
use harness_tools::{canonical_tool_id_for, coordinator_registry};

#[test]
fn coordinator_registry_exposes_single_opencode_style_surface() {
    let registry = coordinator_registry(ShellAllowlist::default());

    for tool_id in [
        "apply_patch",
        "bash",
        "batch",
        "lsp.rename",
        "codesearch",
        "edit",
        "edit.hashline_apply",
        "edit.hashline_scan",
        "github.issue",
        "github.pull_request",
        "glob",
        "grep",
        "invalid",
        "list",
        "lsp",
        "patch",
        "plan_exit",
        "question",
        "read",
        "skill",
        "task",
        "todoread",
        "todowrite",
        "webfetch",
        "websearch",
        "write",
    ] {
        assert!(
            registry.get(tool_id).is_some(),
            "missing canonical tool {tool_id}"
        );
        assert_eq!(canonical_tool_id_for(tool_id), Some(tool_id));
    }

    for legacy_tool_id in [
        "agent.spawn",
        "code.lsp",
        "code.lsp.rename",
        "fs.glob",
        "fs.grep",
        "fs.ls",
        "fs.read",
        "plan.exit",
        "search.code",
        "search.web",
        "shell.run",
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
}
