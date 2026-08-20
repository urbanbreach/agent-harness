use harness_core::config::PermissionMode;
use harness_core::perm::{
    is_tool_call_disabled, PermissionAction, PermissionKind, PermissionPolicy, PermissionRule,
    PermissionRuleset, PolicyDecision,
};
use harness_core::tool::ToolCapability;

fn deny(permission: &str) -> PermissionRuleset {
    vec![PermissionRule {
        permission: permission.to_string(),
        pattern: "*".to_string(),
        action: PermissionAction::Deny,
    }]
}

#[test]
fn task_permission_hides_every_task_authorized_tool() {
    // arrange
    let ruleset = deny("task");

    // act
    for tool_id in [
        "task",
        "skill",
        "background_output",
        "background_cancel",
        "todowrite",
        "todoread",
    ] {
        // assert
        assert!(
            is_tool_call_disabled(tool_id, ToolCapability::SpawnAgent, &ruleset),
            "`{tool_id}` must not be advertised when its execution permission is denied"
        );
    }
}

#[test]
fn capability_permission_keeps_alias_tool_visible_when_allowed() {
    // arrange
    let ruleset = vec![
        PermissionRule {
            permission: "*".to_string(),
            pattern: "*".to_string(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "read".to_string(),
            pattern: "*".to_string(),
            action: PermissionAction::Allow,
        },
    ];

    // act
    // assert
    assert!(!is_tool_call_disabled(
        "list",
        ToolCapability::ReadFs,
        &ruleset
    ));
}

#[test]
fn edit_permission_hides_lsp_rename() {
    // arrange
    let ruleset = deny("edit");

    // act
    let disabled = is_tool_call_disabled("lsp.rename", ToolCapability::EditFs, &ruleset);

    // assert
    assert!(disabled);
}

#[test]
fn composed_session_rules_override_static_profile_policy() {
    // arrange
    let policy = PermissionPolicy::allow_all();
    // act
    let ruleset = deny("edit");

    // assert
    assert_eq!(
        policy.evaluate_request_with_ruleset(None, PermissionKind::EditFs, None, &ruleset),
        PolicyDecision::Deny
    );
}

#[test]
fn later_specific_deny_does_not_reenable_fully_denied_tool() {
    // arrange
    let ruleset = vec![
        PermissionRule {
            permission: "edit".to_string(),
            pattern: "*".to_string(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "edit".to_string(),
            pattern: "secrets/*".to_string(),
            action: PermissionAction::Deny,
        },
    ];

    // act
    // assert
    assert!(is_tool_call_disabled(
        "edit",
        ToolCapability::EditFs,
        &ruleset
    ));
}

#[test]
fn later_specific_allow_keeps_partially_authorized_tool_visible() {
    // arrange
    let mut ruleset = deny("edit");
    // act
    ruleset.push(PermissionRule {
        permission: "edit".to_string(),
        pattern: "generated/*".to_string(),
        action: PermissionAction::Allow,
    });

    // assert
    assert!(!is_tool_call_disabled(
        "edit",
        ToolCapability::EditFs,
        &ruleset
    ));
}

#[test]
fn global_policy_deny_hides_tool_without_session_override() {
    // arrange
    let policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Allow,
        PermissionMode::Allow,
    );

    // act
    // assert
    assert!(policy.is_tool_call_fully_denied(Some("default"), "edit", ToolCapability::EditFs, &[]));
}

#[test]
fn composed_session_rules_apply_to_secondary_permission_gates() {
    // arrange
    let policy = PermissionPolicy::allow_all();

    // act
    for kind in [PermissionKind::ExternalDirectory, PermissionKind::DoomLoop] {
        // assert
        assert_eq!(
            policy.evaluate_request_with_ruleset(None, kind, None, &deny(kind.ruleset_name())),
            PolicyDecision::Deny
        );
    }
}
