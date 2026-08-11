#[test]
fn shipped_profiles_filter_provider_tools_from_permission_rulesets() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = repo_root.join("configs/harness.example.jsonc");
    let config = load_config_from_repo_file(&config_path, &repo_root);
    let coordinator_config =
        bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    let explore = &coordinator_config.agent_profiles["explore"];
    let default = &coordinator_config.agent_profiles["default"];

    let explore_defs = build_provider_tool_defs(explore, coordinator_config.tool_registry.as_ref())
        .unwrap_or_abort();
    let default_defs = build_provider_tool_defs(default, coordinator_config.tool_registry.as_ref())
        .unwrap_or_abort();
    let explore_ids: BTreeSet<&str> = explore_defs
        .iter()
        .map(|definition| definition.tool_id.as_str())
        .collect();
    let default_ids: BTreeSet<&str> = default_defs
        .iter()
        .map(|definition| definition.tool_id.as_str())
        .collect();

    assert!(!explore.permission_ruleset.is_empty());
    for hidden in ["edit", "write", "task"] {
        assert!(
            !explore_ids.contains(hidden),
            "explore provider defs must omit denied {hidden}: {explore_ids:?}"
        );
    }
    for visible in ["bash", "webfetch", "websearch"] {
        assert!(
            explore_ids.contains(visible),
            "explore provider defs must retain {visible}: {explore_ids:?}"
        );
    }
    for visible in ["apply_patch", "task"] {
        assert!(
            default_ids.contains(visible),
            "default provider defs must retain {visible}: {default_ids:?}"
        );
    }
}
