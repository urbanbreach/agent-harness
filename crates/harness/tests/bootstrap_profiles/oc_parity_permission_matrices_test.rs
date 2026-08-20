use harness_core::perm::{PermissionKind, PermissionPolicy, PolicyDecision};

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

#[test]
fn default_agent_keeps_general_runtime_capabilities() {
    // arrange
    let (config, coordinator) = load_example_coordinator();
    let policy = &coordinator.permission_policy;

    // act
    // assert
    assert_policy_allow(policy, "default", PermissionKind::Shell, "bash");
    assert_policy_allow(policy, "default", PermissionKind::EditFs, "edit");
    assert_policy_allow(policy, "default", PermissionKind::WebFetch, "webfetch");
    assert_policy_allow(policy, "default", PermissionKind::Task, "task");

    let profile = &coordinator.agent_profiles["default"];
    for tool_id in ["bash", "edit", "task", "background_output", "todowrite"] {
        assert!(
            profile.toolset.iter().any(|tool| tool == tool_id),
            "default toolset must contain {tool_id}: {:?}",
            profile.toolset
        );
    }

    let global = PermissionPolicy::from_config(&config);
    assert!(matches!(
        global.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        global.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn named_subagents_cannot_redelegate() {
    // arrange
    let (_config, coordinator) = load_example_coordinator();

    // act
    // assert
    for profile_name in ["explore", "general", "librarian"] {
        assert_policy_deny(
            &coordinator.permission_policy,
            profile_name,
            PermissionKind::Task,
            "task",
        );
        let profile = &coordinator.agent_profiles[profile_name];
        assert!(!profile.toolset.iter().any(|tool| tool == "task"));
        assert!(!profile
            .toolset
            .iter()
            .any(|tool| tool == "background_output"));
    }
}

#[test]
fn explore_is_read_only_but_can_search() {
    // arrange
    let (_config, coordinator) = load_example_coordinator();
    let policy = &coordinator.permission_policy;
    let explore = &coordinator.agent_profiles["explore"];

    // act
    // assert
    for tool_id in ["bash", "read", "glob", "grep", "list", "webfetch", "websearch"] {
        assert!(
            explore.toolset.iter().any(|tool| tool == tool_id),
            "explore toolset must contain {tool_id}: {:?}",
            explore.toolset
        );
    }
    assert_policy_allow(policy, "explore", PermissionKind::Shell, "bash");
    assert_policy_allow(policy, "explore", PermissionKind::WebFetch, "webfetch");
    assert_policy_allow(policy, "explore", PermissionKind::WebSearch, "websearch");
    assert_policy_deny(policy, "explore", PermissionKind::EditFs, "edit");
    assert_policy_deny(policy, "explore", PermissionKind::Task, "task");
}

#[test]
fn librarian_is_read_only_and_can_research_externally() {
    // arrange
    let (_config, coordinator) = load_example_coordinator();
    let policy = &coordinator.permission_policy;

    // act
    // assert
    assert_policy_allow(policy, "librarian", PermissionKind::WebFetch, "webfetch");
    assert_policy_allow(policy, "librarian", PermissionKind::WebSearch, "websearch");
    assert_policy_deny(policy, "librarian", PermissionKind::EditFs, "edit");
    assert_policy_deny(policy, "librarian", PermissionKind::Task, "task");
}

#[test]
fn example_config_uses_allow_scalar_without_ask_all() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example_path = repo_root.join("configs/harness.example.jsonc");
    // act
    let raw = fs::read_to_string(&example_path).unwrap_or_abort();
    // assert
    assert!(raw.contains(r#""permission": "allow""#));

    let (config, _) = load_example_coordinator();
    assert_eq!(config.permissions.defaults.edit, PermissionMode::Allow);
    assert_eq!(config.permissions.defaults.shell, PermissionMode::Allow);
}
