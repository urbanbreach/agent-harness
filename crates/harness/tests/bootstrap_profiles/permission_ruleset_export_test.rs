#[test]
fn shipped_profiles_populate_permission_ruleset_before_provider_tool_export() {
    // arrange
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    let explore = &coordinator_config.agent_profiles["explore"];
    let plan = &coordinator_config.agent_profiles["plan"];
    let mut plan_for_permission_visibility = plan.clone();
    plan_for_permission_visibility.model_ref = "mock:model".to_string();

    // act
    let explore_defs = build_provider_tool_defs(explore, coordinator_config.tool_registry.as_ref())
        .unwrap_or_abort();
    let plan_defs = build_provider_tool_defs(
        &plan_for_permission_visibility,
        coordinator_config.tool_registry.as_ref(),
    )
    .unwrap_or_abort();
    let explore_ids: BTreeSet<&str> = explore_defs
        .iter()
        .map(|def| def.tool_id.as_str())
        .collect();
    let plan_ids: BTreeSet<&str> = plan_defs.iter().map(|def| def.tool_id.as_str()).collect();

    // assert
    assert!(
        !explore.permission_ruleset.is_empty(),
        "explore must carry a non-empty permission_ruleset from bootstrap"
    );
    assert!(
        !plan.permission_ruleset.is_empty(),
        "plan must carry a non-empty permission_ruleset from bootstrap"
    );
    assert!(
        plan.toolset.iter().any(|tool| tool == "edit"),
        "plan toolset must include edit before provider export"
    );
    assert!(
        !is_tool_disabled("edit", &plan.permission_ruleset),
        "plan permission_ruleset must not catch-all-disable edit when plan-path allow exists"
    );
    for hidden in ["edit", "write", "task"] {
        assert!(
            !explore_ids.contains(hidden),
            "explore provider defs must omit catch-all denied `{hidden}`; visible={explore_ids:?}"
        );
    }
    assert!(
        explore_ids.contains("bash")
            && explore_ids.contains("webfetch")
            && explore_ids.contains("websearch"),
        "explore provider defs must keep bash/webfetch/websearch: {explore_ids:?}"
    );
    assert!(
        plan_ids.contains("edit"),
        "plan provider defs must keep edit visible under partial plan-path allow: {plan_ids:?}"
    );
}
