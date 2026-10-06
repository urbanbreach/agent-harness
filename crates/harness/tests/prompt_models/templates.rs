use super::*;

#[test]
fn model_aliases_and_bundled_templates_respect_available_capabilities(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::{
        model_resolution::{resolve_model, ModelResolutionInput},
        system_prompt::{PromptContext, PromptSource},
    };
    for (model, metadata, expected) in [
        ("gpt-6.1-sol", Some("gpt-astra"), "gpt-6.1-sol"),
        ("zai-glm-5-3", Some("mistral"), "glm-5.3"),
        ("GLM_5_30", None, "glm"),
        ("opaque", Some("claude-opus-5.5"), "claude-opus-5.5"),
        ("opaque", Some("gpt-astra"), "gpt-6-astra"),
        ("kimi-for-coding-highspeed", None, "kimi-k2.7"),
        ("kimi-for-coding", None, "kimi-k2.8"),
        ("claude-mythos-5-1", None, "claude-fable-5.1"),
        ("deepseek-v4-flash-0731", None, "deepseek-v4-flash-0731"),
        ("mygpt-6clone", None, "default"),
    ] {
        let resolved = resolve_model(ModelResolutionInput {
            provider: "gpt-6-proxy",
            model,
            metadata_family: metadata,
            input_modalities: &[],
            supports_tool_calls: None,
            supports_reasoning_summaries: None,
        });
        assert_eq!(resolved.prompt_preset, expected, "{model}");
    }
    let workspace = tempfile::tempdir()?;
    let tools: Vec<_> = ["eval", "read", "list", "bash", "edit", "spawn_subagent"]
        .into_iter()
        .map(|name| harness_providers::ToolDef {
            tool_id: name.into(),
            function_name: name.into(),
            description: None,
            parameters: json!({"type":"object"}),
        })
        .collect();
    // Exercise the shipped templates, including inheritance, without project overrides.
    for entry in fs::read_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.agent-harness/prompts/models"
    ))? {
        let entry = entry?;
        let name = entry.file_name();
        let preset = name
            .to_str()
            .and_then(|name| name.strip_suffix(".md"))
            .ok_or("model Markdown expected")?;
        for available in [&tools[..], &[][..]] {
            let rendered = PromptSource::default().render(&PromptContext {
                workspace: workspace.path(),
                model: "fixture:model",
                prompt_preset: preset,
                delegation_bias: Default::default(),
                tools: available,
                direct_tools: available,
                current_date: "2026-10-06",
                max_concurrent: 2,
                limit_behavior: Default::default(),
            })?;
            assert_eq!(
                rendered.system.contains("# Delegation"),
                !available.is_empty(),
                "{preset}"
            );
            assert_eq!(
                rendered.eval_guidance.is_empty(),
                available.is_empty(),
                "{preset}"
            );
            if !available.is_empty() {
                assert!(
                    rendered.system.contains(&rendered.eval_guidance),
                    "{preset}"
                );
            }
        }
    }
    Ok(())
}
