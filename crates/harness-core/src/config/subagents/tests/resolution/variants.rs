use super::*;

#[test]
fn variant_precedence_matches_reasoning_effort() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog(&["parent"]);
    let mut definitions = SubagentDefinitionSnapshot::default();
    let mut settings = SubagentRuntimeConfig::default();
    settings.model_variants.insert(
        "parent".into(),
        ["call", "role", "persona", "definition", "parent"]
            .map(str::to_owned)
            .to_vec(),
    );
    for (call, role, persona, definition, parent, expected) in [
        (
            Some("call"),
            Some("role"),
            Some("persona"),
            Some("definition"),
            Some("parent"),
            "call",
        ),
        (
            None,
            Some("role"),
            Some("persona"),
            Some("definition"),
            Some("parent"),
            "role",
        ),
        (
            None,
            None,
            Some("persona"),
            Some("definition"),
            Some("parent"),
            "persona",
        ),
        (
            None,
            None,
            None,
            Some("definition"),
            Some("parent"),
            "definition",
        ),
        (None, None, None, None, Some("parent"), "parent"),
    ] {
        definitions.roles.insert(
            "worker".into(),
            SubagentRole {
                variant: role.map(str::to_owned),
                ..Default::default()
            },
        );
        definitions.personas.insert(
            "reviewer".into(),
            SubagentPersona {
                variant: persona.map(str::to_owned),
                instructions: Some("Review".into()),
                ..Default::default()
            },
        );
        definitions.project.insert(
            "worker".into(),
            SubagentDefinition {
                name: "worker".into(),
                variant: definition.map(str::to_owned),
                ..Default::default()
            },
        );
        let mut context = parent_context(&definitions, &catalog, &[]);
        context.parent_variant = parent;
        let request = SubagentDefinitionRequest {
            subagent_type: "worker".into(),
            persona: Some("reviewer".into()),
            variant: call.map(str::to_owned),
            ..Default::default()
        };
        assert_eq!(
            resolve_subagent_definition(&settings, &request, &context)?
                .variant
                .as_deref(),
            Some(expected)
        );
    }
    Ok(())
}
