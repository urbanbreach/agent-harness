use harness_core::config::ShellAllowlist;
use harness_core::tool::ToolCapability;
use harness_tools::{canonical_tool_id_for, coordinator_registry};

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
        "look_at",
        "lsp",
        "plan_enter",
        "plan_exit",
        "question",
        "read",
        "skill",
        "shell.run",
        "interactive_bash",
        "terminal_spawn",
        "terminal_write",
        "terminal_screenshot",
        "terminal_resize",
        "terminal_kill",
        "terminal_list",
        "task",
        "task_create",
        "task_get",
        "task_list",
        "task_update",
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
        "workflow_dossier_export",
        "workflow_question_record",
        "workflow_signoff",
        "workflow_status",
        "session_info",
        "session_list",
        "session_read",
        "session_search",
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
}

#[test]
fn persistent_task_tools_do_not_use_delegation_capability() {
    let registry = coordinator_registry(ShellAllowlist::default());

    for tool_id in ["task_create", "task_update"] {
        let tool = registry.get(tool_id).expect("persistent task write tool");
        assert_eq!(
            tool.capability(),
            ToolCapability::EditFs,
            "{tool_id} mutates coordinator-owned persistent task state without spawning agents"
        );
    }

    for tool_id in ["task_list", "task_get"] {
        let tool = registry.get(tool_id).expect("persistent task read tool");
        assert_eq!(
            tool.capability(),
            ToolCapability::ReadFs,
            "{tool_id} reads coordinator-owned persistent task projections without spawning agents"
        );
    }
}

#[test]
fn workflow_tools_expose_read_vs_mutation_capabilities() {
    let registry = coordinator_registry(ShellAllowlist::default());

    for tool_id in ["workflow_status", "workflow_dossier_export"] {
        let tool = registry.get(tool_id).expect("workflow read tool");
        assert_eq!(
            tool.capability(),
            ToolCapability::ReadFs,
            "{tool_id} is projection/dossier read-only from the workflow event log"
        );
    }

    let signoff = registry
        .get("workflow_signoff")
        .expect("workflow signoff tool");
    assert_eq!(
        signoff.capability(),
        ToolCapability::EditFs,
        "workflow_signoff mutates coordinator-owned workflow state"
    );

    let question_record = registry
        .get("workflow_question_record")
        .expect("workflow question lifecycle tool");
    assert_eq!(
        question_record.capability(),
        ToolCapability::EditFs,
        "workflow_question_record appends coordinator-owned question lifecycle evidence"
    );
}
