use harness_core::config::ShellAllowlist;
use harness_core::event::ActorKind;
use harness_core::perm::{permission_kind_for_tool_call, PermissionKind};
use harness_core::tool::ToolCapability;
use harness_tools::{canonical_tool_id_for, coordinator_registry, worker_registry};
use serde_json::{json, Value};

const LEGACY_TOOL_IDS: &[&str] = &[
    "agent.spawn",
    "code.lsp",
    "code.lsp.rename",
    "edit.hashline_apply",
    "edit.hashline_scan",
    "fs.glob",
    "fs.grep",
    "fs.ls",
    "fs.read",
    "fs.write",
    "search.code",
    "search.web",
    "skill.load",
    "todo.read",
    "todo.write",
    "tool.batch",
    "tool.invalid",
    "user.question",
    "web.fetch",
];

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

    for legacy_tool_id in LEGACY_TOOL_IDS {
        assert!(
            registry.get(legacy_tool_id).is_none(),
            "legacy tool should not be registered: {legacy_tool_id}"
        );
        assert_eq!(
            canonical_tool_id_for(legacy_tool_id),
            Some(*legacy_tool_id),
            "canonical helper is identity-only; aliases must stay unregistered instead of remapping"
        );
    }
}

#[test]
fn native_registry_contract_matrix_covers_permissions_actors_and_schema_strictness() {
    let coordinator = coordinator_registry(ShellAllowlist::default());
    let worker = worker_registry(ShellAllowlist::default());

    let mut coordinator_tool_ids = coordinator.tool_ids();
    coordinator_tool_ids.sort();
    let mut worker_tool_ids = worker.tool_ids();
    worker_tool_ids.sort();
    let mut expected_worker_tool_ids = coordinator
        .filter_for_actor(ActorKind::Worker)
        .into_iter()
        .map(|tool| tool.id().to_string())
        .collect::<Vec<_>>();
    expected_worker_tool_ids.sort();
    assert_eq!(
        worker_tool_ids, expected_worker_tool_ids,
        "worker registry must be the coordinator registry filtered by worker actor eligibility"
    );

    for tool_id in coordinator_tool_ids {
        let tool = coordinator.get(&tool_id).expect("coordinator tool");
        assert_eq!(
            worker.get(&tool_id).is_some(),
            coordinator
                .get_for_actor(ActorKind::Worker, &tool_id)
                .is_some(),
            "worker registry exposure for {tool_id} should follow actor eligibility"
        );
        assert!(
            coordinator
                .get_for_actor(ActorKind::User, &tool_id)
                .is_none(),
            "user actor must never execute native tool {tool_id}"
        );

        let schema = tool.parameters_json_schema();
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&json!(false)),
            "{tool_id} schema must reject unknown top-level fields"
        );
        let nested_schema_violations = strict_schema_violations(&schema, "$".to_string());
        assert!(
            nested_schema_violations.is_empty(),
            "{tool_id} schema must reject unknown fields for every nested object:\n{}",
            nested_schema_violations.join("\n")
        );

        let expected_permission = expected_permission_kind(&tool_id, tool.capability());
        assert_eq!(
            permission_kind_for_tool_call(&tool_id, tool.capability()),
            expected_permission,
            "{tool_id} permission kind should stay aligned with its native capability"
        );
    }
}

fn strict_schema_violations(schema: &Value, path: String) -> Vec<String> {
    let mut violations = Vec::new();
    let Some(object) = schema.as_object() else {
        return violations;
    };

    if object.contains_key("properties")
        && object.get("additionalProperties") != Some(&json!(false))
    {
        violations.push(format!(
            "{path} declares object properties without additionalProperties=false"
        ));
    }

    for (key, value) in object {
        match key.as_str() {
            "properties" | "$defs" | "definitions" => {
                if let Some(children) = value.as_object() {
                    for (child_name, child_schema) in children {
                        violations.extend(strict_schema_violations(
                            child_schema,
                            format!("{path}/{key}/{child_name}"),
                        ));
                    }
                }
            }
            "items" | "additionalProperties" => {
                if value.is_object() {
                    violations.extend(strict_schema_violations(value, format!("{path}/{key}")));
                }
            }
            "allOf" | "anyOf" | "oneOf" => {
                if let Some(children) = value.as_array() {
                    for (index, child_schema) in children.iter().enumerate() {
                        violations.extend(strict_schema_violations(
                            child_schema,
                            format!("{path}/{key}/{index}"),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    violations
}

fn expected_permission_kind(tool_id: &str, capability: ToolCapability) -> Option<PermissionKind> {
    match tool_id {
        "question" | "workflow_question_record" => Some(PermissionKind::Question),
        "task"
        | "background_output"
        | "background_cancel"
        | "plan_enter"
        | "plan_exit"
        | "team_create"
        | "team_delete"
        | "team_list"
        | "team_send_message"
        | "team_shutdown_approve"
        | "team_shutdown_reject"
        | "team_shutdown_request"
        | "team_status"
        | "team_task_create"
        | "team_task_get"
        | "team_task_list"
        | "team_task_update" => Some(PermissionKind::Task),
        "webfetch" => Some(PermissionKind::WebFetch),
        "websearch" => Some(PermissionKind::WebSearch),
        "codesearch" => Some(PermissionKind::CodeSearch),
        "lsp" => Some(PermissionKind::Lsp),
        "lsp.rename" | "workflow_signoff" => Some(PermissionKind::EditFs),
        "bash"
        | "shell.run"
        | "interactive_bash"
        | "terminal_spawn"
        | "terminal_write"
        | "terminal_screenshot"
        | "terminal_resize"
        | "terminal_kill"
        | "terminal_list" => Some(PermissionKind::Shell),
        _ => match capability {
            ToolCapability::ReadFs => None,
            ToolCapability::EditFs => Some(PermissionKind::EditFs),
            ToolCapability::Shell => Some(PermissionKind::Shell),
            ToolCapability::Network => Some(PermissionKind::Network),
            ToolCapability::SpawnAgent => Some(PermissionKind::Task),
        },
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
