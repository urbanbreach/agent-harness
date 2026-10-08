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
    let ids = [
        "eval",
        "read",
        "list",
        "grep",
        "glob",
        "edit",
        "write",
        "apply_patch",
        "bash",
        "lsp",
        "ast_grep_search",
        "ast_grep_replace",
        "skill",
        "todowrite",
        "question",
        "spawn_subagent",
        "send_subagent_message",
        "get_command_or_subagent_output",
        "wait_commands_or_subagents",
    ];
    let full: Vec<_> = ids
        .into_iter()
        .map(|name| harness_providers::ToolDef {
            tool_id: name.into(),
            function_name: name.into(),
            description: None,
            parameters: json!({"type":"object"}),
        })
        .collect();
    // Children never receive the user-facing question or todo tools.
    let child: Vec<_> = full
        .iter()
        .filter(|tool| !matches!(tool.tool_id.as_str(), "question" | "todowrite"))
        .cloned()
        .collect();
    let main = PromptSource::default();
    let subagent = PromptSource {
        subagent: true,
        agent_instructions: "Worker agent.".into(),
        ..PromptSource::default()
    };
    let literal = PromptSource {
        configured: Some("Literal prompt.".into()),
        ..PromptSource::default()
    };
    let cases = [
        (&main, &full[..], &full[..]),
        (&main, &full[..], &full[..1]),
        (&subagent, &child[..], &child[..]),
        (&main, &[][..], &[][..]),
    ];
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
        for (source, tools, direct_tools) in cases {
            let render = |source: &PromptSource| {
                source.render(&PromptContext {
                    workspace: workspace.path(),
                    model: "fixture:model",
                    prompt_preset: preset,
                    tools,
                    direct_tools,
                    current_date: "2026-10-06",
                    max_concurrent: 2,
                    limit_behavior: Default::default(),
                    behavior: &harness_core::config::BehaviorSettings::default(),
                })
            };
            let rendered = render(source)?;
            assert!(!rendered.system.contains("${"), "{preset}: template syntax");
            // The literal prompt cannot carry eval routing, so the tool receives all of it.
            let guidance = render(&literal)?.eval_tool_guidance;
            let has_eval = tools.iter().any(|tool| tool.tool_id == "eval");
            assert_eq!(guidance.is_empty(), !has_eval, "{preset}");
            assert!(rendered.eval_tool_guidance.is_empty(), "{preset}");
            if has_eval {
                assert_eq!(rendered.system.matches(&guidance).count(), 1, "{preset}");
            }
            for id in ids {
                if !tools.iter().any(|tool| tool.tool_id == id) {
                    assert!(
                        !rendered.system.contains(&format!("`{id}`")),
                        "{preset} names unavailable tool {id}"
                    );
                }
            }
        }
    }
    Ok(())
}
