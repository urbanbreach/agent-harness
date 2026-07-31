use harness_core::perm::{
    evaluate_ruleset, from_profile_permissions, is_tool_disabled, PermissionAction, PermissionKind,
    PermissionPolicy, PolicyDecision,
};

fn load_example_coordinator() -> (
    harness_core::config::HarnessConfig,
    harness_core::coord::CoordinatorConfig,
) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    (config, coordinator_config)
}

fn assert_policy_allow(
    policy: &PermissionPolicy,
    profile: &str,
    kind: PermissionKind,
    label: &str,
) {
    let decision = policy.evaluate(Some(profile), kind);
    assert!(
        matches!(decision, PolicyDecision::Allow),
        "expected Allow for {label} on {profile}, got {decision:?}"
    );
}

fn assert_policy_deny(
    policy: &PermissionPolicy,
    profile: &str,
    kind: PermissionKind,
    label: &str,
) {
    let decision = policy.evaluate(Some(profile), kind);
    assert!(
        matches!(decision, PolicyDecision::Deny),
        "expected Deny for {label} on {profile}, got {decision:?}"
    );
}

fn assert_policy_ask(policy: &PermissionPolicy, profile: &str, kind: PermissionKind, label: &str) {
    let decision = policy.evaluate(Some(profile), kind);
    assert!(
        matches!(decision, PolicyDecision::Ask { .. }),
        "expected Ask for {label} on {profile}, got {decision:?}"
    );
}

#[test]
fn oc_parity_build_bash_and_edit_allow_without_ask() {
    // arrange
    // act
    // assert
    let (config, coordinator) = load_example_coordinator();
    let policy = &coordinator.permission_policy;

    assert_policy_allow(policy, "build", PermissionKind::Shell, "bash");
    assert_policy_allow(policy, "build", PermissionKind::EditFs, "edit");
    assert_policy_allow(policy, "build", PermissionKind::WebFetch, "webfetch");
    assert_policy_allow(policy, "build", PermissionKind::Question, "question");

    let global = PermissionPolicy::from_config(&config);
    for (kind, label) in [
        (PermissionKind::Shell, "bash"),
        (PermissionKind::EditFs, "edit"),
        (PermissionKind::WebFetch, "webfetch"),
    ] {
        let decision = global.evaluate(None, kind);
        assert!(
            matches!(decision, PolicyDecision::Allow),
            "expected Allow for {label} on default config (no profile), got {decision:?}"
        );
        assert!(
            !matches!(decision, PolicyDecision::Ask { .. }),
            "expected Allow for {label} on default config (not Ask), got {decision:?}"
        );
    }
    assert!(
        matches!(
            global.evaluate(None, PermissionKind::ExternalDirectory),
            PolicyDecision::Ask { .. }
        ),
        "expected Ask for external_directory on example scalar allow defaults"
    );
    assert!(
        matches!(
            global.evaluate(None, PermissionKind::DoomLoop),
            PolicyDecision::Ask { .. }
        ),
        "expected Ask for doom_loop on example scalar allow defaults"
    );
    assert!(
        matches!(
            global.evaluate(None, PermissionKind::Question),
            PolicyDecision::Deny
        ),
        "expected Deny for base question on example scalar allow defaults"
    );

    let build_perms = config
        .agents
        .get("build")
        .and_then(|agent| agent.permissions.as_ref())
        .expect("build shipped profile must define permissions");
    assert_eq!(
        build_perms.question,
        Some(PermissionMode::Allow),
        "expected build profile to re-allow question over base deny, got {:?}",
        build_perms.question
    );
}

#[test]
fn oc_parity_general_task_allow_and_todowrite_deny() {
    // arrange
    // act
    // assert
    let (config, coordinator) = load_example_coordinator();
    let policy = &coordinator.permission_policy;
    let general_profile = &coordinator.agent_profiles["general"];

    assert_policy_allow(policy, "general", PermissionKind::Task, "task");
    assert_policy_deny(policy, "general", PermissionKind::Question, "question");
    assert!(
        general_profile.toolset.iter().any(|t| t == "task"),
        "expected general toolset to include task (reference general can redelegate); toolset={:?}",
        general_profile.toolset
    );
    assert!(
        general_profile
            .toolset
            .iter()
            .any(|t| t == "background_output"),
        "expected general toolset to include background_output with task; toolset={:?}",
        general_profile.toolset
    );

    let general_perms = config
        .agents
        .get("general")
        .and_then(|agent| agent.permissions.as_ref())
        .expect("general shipped profile must define permissions");
    assert_eq!(
        general_perms.task,
        Some(PermissionMode::Allow),
        "expected general task permission Allow (reference policy), got {:?}",
        general_perms.task
    );
    assert_eq!(
        general_perms.question,
        Some(PermissionMode::Deny),
        "expected general question Deny (OC base deny, not re-allowed), got {:?}",
        general_perms.question
    );

    let ruleset = from_profile_permissions(general_perms);
    let todo_eval = evaluate_ruleset("todowrite", "*", [&ruleset]);
    assert_eq!(
        todo_eval.action,
        PermissionAction::Deny,
        "expected Deny for todowrite on general (reference policy denies todowrite independently of task allow), got {:?}",
        todo_eval.action
    );
    assert!(
        is_tool_disabled("todowrite", &ruleset),
        "expected todowrite catch-all deny on general ruleset (visibility + execution), ruleset={ruleset:?}"
    );

    let task_eval = evaluate_ruleset("task", "*", [&ruleset]);
    assert_eq!(
        task_eval.action,
        PermissionAction::Allow,
        "expected Allow for task on general ruleset while todowrite is deny, got {:?}",
        task_eval.action
    );
    assert_ne!(
        task_eval.action, todo_eval.action,
        "task and todowrite must not dual-map from the same scalar on general (OC: task allow, todowrite deny)"
    );
}

#[test]
fn oc_parity_plan_shell_ask_and_plan_file_edit_matrix() {
    // arrange
    // act
    // assert
    let (_config, coordinator) = load_example_coordinator();
    let policy = &coordinator.permission_policy;

    assert_policy_ask(policy, "plan", PermissionKind::Shell, "bash/shell");

    let plan_path = policy.evaluate_request(
        Some("plan"),
        PermissionKind::EditFs,
        Some(&PermissionRuleRequest::WorkspacePath(
            ".agent-harness/plans/run_demo.md".to_string(),
        )),
    );
    assert!(
        matches!(plan_path, PolicyDecision::Allow),
        "expected Allow for edit on plan file, got {plan_path:?}"
    );

    let non_plan = policy.evaluate_request(
        Some("plan"),
        PermissionKind::EditFs,
        Some(&PermissionRuleRequest::WorkspacePath(
            "src/lib.rs".to_string(),
        )),
    );
    assert!(
        matches!(non_plan, PolicyDecision::Deny),
        "expected Deny for edit on non-plan path, got {non_plan:?}"
    );
}

#[test]
fn oc_parity_explore_read_search_allow_and_edit_task_deny() {
    // arrange
    // act
    // assert
    let (_config, coordinator) = load_example_coordinator();
    let policy = &coordinator.permission_policy;
    let explore = &coordinator.agent_profiles["explore"];

    for tool_id in [
        "bash",
        "read",
        "glob",
        "grep",
        "list",
        "webfetch",
        "websearch",
    ] {
        assert!(
            explore.toolset.iter().any(|t| t == tool_id),
            "expected explore toolset to include `{tool_id}` (Harness explore allow-list); toolset={:?}",
            explore.toolset
        );
        assert!(
            !coordinator_denies_tool_for_profile(&coordinator, "explore", tool_id),
            "expected explore to allow `{tool_id}` under Harness-aligned matrix"
        );
    }

    assert_policy_allow(policy, "explore", PermissionKind::Shell, "bash");
    assert_policy_allow(policy, "explore", PermissionKind::WebFetch, "webfetch");
    assert_policy_allow(policy, "explore", PermissionKind::WebSearch, "websearch");
    assert_policy_deny(policy, "explore", PermissionKind::EditFs, "edit");
    assert_policy_deny(policy, "explore", PermissionKind::Task, "task");

    for denied in ["edit", "task"] {
        assert!(
            coordinator_denies_tool_for_profile(&coordinator, "explore", denied),
            "expected explore to deny `{denied}`"
        );
    }

    assert_policy_deny(policy, "deep", PermissionKind::Task, "category task");
}

#[test]
fn oc_parity_example_config_does_not_force_ask_all_scalar() {
    // arrange
    // act
    // assert
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example_path = repo_root.join("configs/harness.example.jsonc");
    let raw = fs::read_to_string(&example_path).unwrap_or_abort();
    assert!(
        raw.contains(r#""permission": "allow""#),
        "expected configs/harness.example.jsonc to use permission allow (Harness default), not ask-all"
    );

    let (config, _) = load_example_coordinator();
    assert_eq!(
        config.permissions.defaults.edit,
        PermissionMode::Allow,
        "expected example config edit default Allow, got {:?}",
        config.permissions.defaults.edit
    );
    assert_eq!(
        config.permissions.defaults.shell,
        PermissionMode::Allow,
        "expected example config bash default Allow, got {:?}",
        config.permissions.defaults.shell
    );
    let policy = PermissionPolicy::from_config(&config);
    let decision = policy.evaluate(None, PermissionKind::Shell);
    assert!(
        matches!(decision, PolicyDecision::Allow),
        "expected Allow for bash on example config defaults, got {decision:?}"
    );
}
