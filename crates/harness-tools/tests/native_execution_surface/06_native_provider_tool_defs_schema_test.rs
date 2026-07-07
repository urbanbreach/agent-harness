use harness_tools::UnwrapOrAbort;
#[test]
fn native_provider_tool_defs_accept_edit_and_question_export_schemas() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let profile = AgentProfile {
        name: "provider-safe-native-schemas".to_string(),
        category: "test".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "test".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec!["edit".to_string(), "question".to_string()],
    };

    // act
    let defs = build_provider_tool_defs(&profile, &registry)
        .unwrap_or_abort();

    // assert
    assert_eq!(defs.len(), 2);
    for def in defs {
        assert_eq!(def.parameters["type"], json!("object"));
    }
}

#[test]
fn provider_tool_defs_match_current_core_filesystem_input_schemas() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let profile = AgentProfile {
        name: "provider-core-fs-schema-parity".to_string(),
        category: "test".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "test".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![
            "read".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "edit".to_string(),
            "write".to_string(),
        ],
    };

    // act
    let defs = build_provider_tool_defs(&profile, &registry)
        .unwrap_or_abort();

    // assert
    assert_schema_properties(
        &defs,
        "read",
        &["path", "offset", "limit"],
        &["path"],
        &["filePath", "hashlineAnchors", "hashline_anchors"],
    );
    assert_schema_properties(
        &defs,
        "glob",
        &["pattern", "path", "limit"],
        &["pattern"],
        &[],
    );
    assert_schema_properties(
        &defs,
        "grep",
        &["pattern", "path", "include", "limit"],
        &["pattern"],
        &["literal", "context"],
    );
    assert_schema_properties(
        &defs,
        "edit",
        &["path", "oldString", "newString", "replaceAll"],
        &["path", "oldString", "newString"],
        &["filePath", "edits", "delete", "rename"],
    );
    assert_schema_properties(
        &defs,
        "write",
        &["path", "content"],
        &["path", "content"],
        &["filePath"],
    );
}

#[test]
fn provider_tool_defs_do_not_advertise_runtime_only_grep_controls() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let profile = AgentProfile {
        name: "provider-grep-text-parity".to_string(),
        category: "test".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "test".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec!["grep".to_string()],
    };

    // act
    let defs = build_provider_tool_defs(&profile, &registry)
        .unwrap_or_abort();
    let grep = defs
        .iter()
        .find(|def| def.tool_id == "grep")
        .unwrap_or_abort();
    let description = grep
        .description
        .as_deref()
        .unwrap_or_abort();
    let schema_text = grep.parameters.to_string();

    // assert
    for runtime_only in ["literal", "context"] {
        assert!(
            !description.contains(runtime_only),
            "grep provider description should not advertise runtime-only field {runtime_only}: {description}"
        );
        assert!(
            !schema_text.contains(runtime_only),
            "grep provider schema should not advertise runtime-only field {runtime_only}: {schema_text}"
        );
    }
}

#[test]
fn native_provider_tool_defs_gate_patch_tools_like_baseline_registry() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let profile = AgentProfile {
        name: "provider-patch-gating".to_string(),
        category: "test".to_string(),
        model_ref: "openai/gpt-5.5".to_string(),
        model_ref_explicit: true,
        system_prompt: "test".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![
            "apply_patch".to_string(),
            "edit".to_string(),
            "write".to_string(),
            "read".to_string(),
        ],
    };

    // act
    let defs = build_provider_tool_defs(&profile, &registry)
        .unwrap_or_abort();
    let ids = defs
        .iter()
        .map(|def| def.tool_id.as_str())
        .collect::<Vec<_>>();

    // assert
    assert!(ids.contains(&"apply_patch"));
    assert!(ids.contains(&"read"));
    assert!(!ids.contains(&"edit"));
    assert!(!ids.contains(&"write"));
}

#[test]
fn native_provider_tool_defs_gate_edit_write_tools_for_non_patch_models() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let profile = AgentProfile {
        name: "provider-edit-write-gating".to_string(),
        category: "test".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "test".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![
            "apply_patch".to_string(),
            "edit".to_string(),
            "write".to_string(),
            "read".to_string(),
        ],
    };

    // act
    let defs = build_provider_tool_defs(&profile, &registry)
        .unwrap_or_abort();
    let ids = defs
        .iter()
        .map(|def| def.tool_id.as_str())
        .collect::<Vec<_>>();

    // assert
    assert!(!ids.contains(&"apply_patch"));
    assert!(ids.contains(&"edit"));
    assert!(ids.contains(&"write"));
    assert!(ids.contains(&"read"));
}

#[test]
fn native_provider_tool_defs_gate_patch_tools_by_actual_turn_model() {
    // arrange
    let registry = coordinator_registry(ShellAllowlist::default());
    let profile = AgentProfile {
        name: "provider-model-override-gating".to_string(),
        category: "test".to_string(),
        model_ref: "mock:model".to_string(),
        model_ref_explicit: true,
        system_prompt: "test".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(4),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![
            "apply_patch".to_string(),
            "edit".to_string(),
            "write".to_string(),
            "read".to_string(),
        ],
    };

    // act
    let patch_defs = build_provider_tool_defs_for_model(&profile, &registry, "openai/gpt-5.5")
        .unwrap_or_abort();
    let edit_defs = build_provider_tool_defs_for_model(&profile, &registry, "mock:model")
        .unwrap_or_abort();
    let patch_ids = patch_defs
        .iter()
        .map(|def| def.tool_id.as_str())
        .collect::<Vec<_>>();
    let edit_ids = edit_defs
        .iter()
        .map(|def| def.tool_id.as_str())
        .collect::<Vec<_>>();

    // assert
    assert!(patch_ids.contains(&"apply_patch"));
    assert!(!patch_ids.contains(&"edit"));
    assert!(!patch_ids.contains(&"write"));
    assert!(!edit_ids.contains(&"apply_patch"));
    assert!(edit_ids.contains(&"edit"));
    assert!(edit_ids.contains(&"write"));
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_schema_properties(
    defs: &[harness_providers::ToolDef],
    tool_id: &str,
    expected_properties: &[&str],
    expected_required: &[&str],
    absent_properties: &[&str],
) {
    let def = defs
        .iter()
        .find(|def| def.tool_id == tool_id)
        .unwrap_or_else(|| panic!("abort"));
    let properties = def.parameters["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("abort"));
    let mut property_names = properties.keys().map(String::as_str).collect::<Vec<_>>();
    property_names.sort_unstable();
    let mut expected = expected_properties.to_vec();
    expected.sort_unstable();
    assert_eq!(property_names, expected, "{tool_id} provider-visible properties");

    let required = def.parameters["required"]
        .as_array()
        .unwrap_or_else(|| panic!("abort"));
    let mut required = required
        .iter()
        .map(|value| value.as_str().unwrap_or_abort())
        .collect::<Vec<_>>();
    required.sort_unstable();
    let mut expected_required = expected_required.to_vec();
    expected_required.sort_unstable();
    assert_eq!(required, expected_required, "{tool_id} provider-visible required fields");

    for absent in absent_properties {
        assert!(
            !properties.contains_key(*absent),
            "{tool_id} provider schema should not advertise compatibility field {absent}"
        );
    }
}
