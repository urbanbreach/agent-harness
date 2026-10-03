use super::*;

#[test]
fn subagent_settings_preserve_local_enablement_and_independent_limits(
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config_from_str("{subagents:{max_depth:2,max_concurrent:7,sampling_limit:900,limit_behavior:'invalid'},features:{subagent_model_inheritance:true}}")?;
    let remote = SubagentRemoteConfig {
        enabled: Some(false),
        max_depth: Some(4),
        max_concurrent: Some(9),
        limit_behavior: Some("FAIL".into()),
        active_agent_messages: Some(true),
        ..SubagentRemoteConfig::default()
    };
    let lookup = |name: &str| match name {
        "HARNESS_SUBAGENTS_MAX_DEPTH" => Some("3".into()),
        "HARNESS_MAX_CONCURRENT_SUBAGENTS" => Some("0".into()),
        "HARNESS_SUBAGENT_SAMPLING_LIMIT" => Some("2".into()),
        "HARNESS_SUBAGENT_MODEL_INHERITANCE" => Some("false".into()),
        _ => None,
    };
    let settings = config.subagents.resolve_with_lookup(
        None,
        &config.features,
        &remote,
        &SubagentFeatureRequirements::default(),
        &lookup,
    );
    assert!(settings.enabled);
    assert_eq!(
        (
            settings.max_depth,
            settings.max_concurrent,
            settings.sampling_limit
        ),
        (3, 7, 2)
    );
    assert_eq!(settings.limit_behavior, SubagentLimitBehavior::Fail);
    assert!(settings.messaging_enabled);
    assert!(!settings.model_inheritance);
    let requirements = SubagentFeatureRequirements {
        subagent_model_inheritance: Some(true),
        ..SubagentFeatureRequirements::default()
    };
    let required = config.subagents.resolve_with_lookup(
        Some(false),
        &config.features,
        &remote,
        &requirements,
        &lookup,
    );
    assert!(!required.enabled);
    assert!(required.model_inheritance);
    let bounded = SubagentsConfig {
        max_depth: Some(i64::MAX),
        max_concurrent: Some(-2),
        sampling_limit: Some(900),
        ..SubagentsConfig::default()
    }
    .resolve_with_lookup(
        None,
        &SubagentFeaturesConfig::default(),
        &SubagentRemoteConfig::default(),
        &SubagentFeatureRequirements::default(),
        &|_| Some("invalid".into()),
    );
    assert_eq!(
        (
            bounded.max_depth,
            bounded.max_concurrent,
            bounded.sampling_limit
        ),
        (u32::MAX, 1, 512)
    );
    for input in [
        "{subagents:{max_dept:2}}",
        "{subagents:{roles:{explore:{unknown:true}}}}",
        "{features:{subagent_model_inheritance:'yes'}}",
    ] {
        assert!(load_config_from_str(input).is_err(), "{input}");
    }
    let schema = config_json_schema();
    assert_eq!(
        schema["$defs"]["SubagentsConfig"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["$defs"]["SubagentRole"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["properties"]["subagents"]["default"]["enabled"],
        true
    );
    Ok(())
}

#[test]
fn catalog_policy_is_authoritative_exact_and_schema_hints_do_not_gate_runtime(
) -> Result<(), Box<dyn std::error::Error>> {
    for (authority, families, expected) in [
        (
            SubagentCatalogAuthority::Complete,
            vec![],
            SubagentModelSelection::Selectable,
        ),
        (
            SubagentCatalogAuthority::Provisional,
            vec![Some("xai")],
            SubagentModelSelection::Selectable,
        ),
        (
            SubagentCatalogAuthority::Complete,
            vec![None],
            SubagentModelSelection::Selectable,
        ),
        (
            SubagentCatalogAuthority::Complete,
            vec![Some("")],
            SubagentModelSelection::Selectable,
        ),
        (
            SubagentCatalogAuthority::Complete,
            vec![Some("XAI")],
            SubagentModelSelection::Selectable,
        ),
        (
            SubagentCatalogAuthority::Complete,
            vec![Some("xai"), Some("third")],
            SubagentModelSelection::Selectable,
        ),
        (
            SubagentCatalogAuthority::Complete,
            vec![Some(" xai ")],
            SubagentModelSelection::Inherited,
        ),
    ] {
        let catalog = SubagentModelCatalog {
            authority,
            models: families
                .into_iter()
                .enumerate()
                .map(|(i, family)| SubagentCatalogModel {
                    id: format!("model-{i}"),
                    family: family.map(str::to_owned),
                    picker_eligible: true,
                })
                .collect(),
        };
        assert_eq!(
            SubagentModelPolicy::latch(true, &catalog, None).selection,
            expected
        );
        assert_eq!(
            SubagentModelPolicy::latch(false, &catalog, None).selection,
            SubagentModelSelection::Selectable
        );
        assert_eq!(
            SubagentModelPolicy::latch(false, &catalog, Some(SubagentModelSelection::Inherited))
                .selection,
            SubagentModelSelection::Inherited
        );
    }
    let mut definitions = SubagentDefinitionSnapshot::default();
    for i in 0..65 {
        let name = format!("worker-{i:02}");
        definitions.project.insert(
            name.clone(),
            SubagentDefinition {
                name,
                description: "multi\n  line ".repeat(50),
                source: SubagentDefinitionSource::Project,
                ..SubagentDefinition::default()
            },
        );
    }
    let selectable = definitions.selectable_types(&BTreeMap::new(), None);
    let schema = subagent_type_schema(&selectable).ok_or("schema missing")?;
    assert_eq!(schema["enum"].as_array().ok_or("enum missing")?.len(), 64);
    assert!(!schema["enum"]
        .as_array()
        .ok_or("enum missing")?
        .contains(&serde_json::json!("worker-64")));
    let catalog = catalog(&["parent"]);
    let mut context = parent_context(&definitions, &catalog, &[]);
    let request = SubagentDefinitionRequest {
        subagent_type: "worker-64".into(),
        type_specified: true,
        ..SubagentDefinitionRequest::default()
    };
    assert_eq!(
        resolve_subagent_definition(&SubagentRuntimeConfig::default(), &request, &context)?
            .subagent_type,
        "worker-64"
    );
    let explicit = SubagentDefinitionRequest {
        model: Some("missing".into()),
        ..request
    };
    assert!(matches!(
        resolve_subagent_definition(&SubagentRuntimeConfig::default(), &explicit, &context),
        Err(SubagentResolutionError::InvalidModel { .. })
    ));
    context.catalog = None;
    assert_eq!(
        resolve_subagent_definition(&SubagentRuntimeConfig::default(), &explicit, &context),
        Err(SubagentResolutionError::ValidationUnavailable)
    );
    assert!(resolve_subagent_definition(
        &SubagentRuntimeConfig::default(),
        &SubagentDefinitionRequest {
            resume: true,
            ..explicit
        },
        &context
    )
    .is_ok());
    Ok(())
}
