use harness::{run, CliDeps, CliIo};
use harness_providers::{
    mock::MockProvider, Provider, ProviderErrorCategory, ProviderStreamEvent as Stream,
};
use serde_json::json;
use std::{fs, io::Cursor, sync::Arc};

#[path = "prompt_models/subagents.rs"]
mod subagents;

#[path = "prompt_models/templates.rs"]
mod templates;

#[tokio::test]
async fn model_fallback_rebuilds_the_model_prompt_and_keeps_explicit_instructions(
) -> Result<(), Box<dyn std::error::Error>> {
    for explicit in [None, Some("Keep this configured prompt.")] {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("fixture.json"), json!({
            "provider":{"local":{"type":"openai_compatible","models":{
                "gpt-5.6":{}, "gpt-6.1-sol":{}, "claude-sonnet":{}, "glm-5.3":{},
                "aliased-model":{"metadata":{"family":"gpt-6"}}
            }}},
            "model":"quality", "model_profile":{"quality":{"model":"local/gpt-5.6","fallback":[
                {"model":"local/gpt-6.1-sol"},{"model":"local/claude-sonnet"},{"model":"local/glm-5.3"},
                {"model":"local/aliased-model"}
            ]}},
            "agent":{"default":{"tools":["spawn_subagent","eval","list","bash"],"system_prompt":explicit}},
            "instructions":"Keep project instructions.","runtime":{"provider_retry":{"max_retries":0}}
        }).to_string())?;
        let mut script: Vec<_> = (0..4)
            .map(|_| {
                vec![Stream::categorized_error(
                    "try the fallback",
                    ProviderErrorCategory::TransportFailure,
                )]
            })
            .collect();
        script.extend([done("Finished."), done("Resumed.")]);
        let provider = Arc::new(MockProvider::script(script));
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        for resume in [false, true] {
            let mut args = vec![
                "harness",
                "--config",
                "fixture.json",
                "prompt",
                "--text",
                "Continue.",
                "--rules",
                "Keep the command rule.",
            ];
            args.extend(if resume {
                ["--resume", "policy-test"]
            } else {
                ["--session-id", "policy-test"]
            });
            let result = run(
                args,
                &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
                CliDeps::real()
                    .with_current_dir(root.path().into())
                    .without_env("HOME")
                    .without_env("XDG_CONFIG_HOME")
                    .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>),
            );
            assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        }
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 6);
        for (request, model) in requests.iter().zip([
            "gpt-5.6",
            "gpt-6.1-sol",
            "claude-sonnet",
            "glm-5.3",
            "aliased-model",
            "aliased-model",
        ]) {
            assert_eq!(request.model_id, model);
            let system = &request.messages[0].content;
            if let Some(explicit) = explicit {
                assert!(system.starts_with(explicit));
            } else {
                assert!(
                    system.contains(&format!("Active model: local:{model}")),
                    "{system}"
                );
            }
            assert!(system.contains("Keep project instructions."));
            assert_eq!(system.matches("Keep the command rule.").count(), 1);
            let description = request
                .tools
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|tool| tool.tool_id == "eval")
                .and_then(|tool| tool.description.as_deref())
                .ok_or("missing eval description")?;
            // Each model's eval dialect reaches the model exactly once: in the rebuilt system
            // prompt, or in the tool description when a literal prompt replaces the templates.
            let (dialect, other) = if matches!(model, "glm-5.3" | "claude-sonnet") {
                ("<eval_routing>", "Eval routing:")
            } else {
                ("Eval routing:", "<eval_routing>")
            };
            let (carrier, bystander) = if explicit.is_some() {
                (description, system.as_str())
            } else {
                (system.as_str(), description)
            };
            assert_eq!(carrier.matches(dialect).count(), 1, "{model}");
            assert!(
                !bystander.contains(dialect),
                "duplicated eval routing for {model}"
            );
            assert!(!carrier.contains(other), "wrong eval dialect for {model}");
        }
        assert_eq!(requests[0].messages[1..], requests[3].messages[1..]);
    }
    Ok(())
}

#[tokio::test]
async fn editable_model_prompts_obey_precedence_reload_and_reject_bad_files(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let user = root.path().join("user/harness/prompts");
    let project = root.path().join("repo/.agent-harness/prompts");
    fs::create_dir_all(user.join("models"))?;
    fs::create_dir_all(user.join("eval"))?;
    fs::create_dir_all(project.join("models"))?;
    fs::write(user.join("system.md"), "USER_BASE")?;
    fs::write(
        project.join("system.md"),
        "PROJECT_BASE ${% block model_guidance %}${% endblock %} ${{ eval_guidance }}",
    )?;
    fs::write(
        user.join("models/glm-5.3.md"),
        "${% extends \"system.md\" %}${% block model_guidance %}USER_GLM${% endblock %}",
    )?;
    fs::write(user.join("eval/claude.md"), "CUSTOM_EVAL_RULE")?;
    let workspace = root.path().join("repo");
    fs::write(
        workspace.join("fixture.json"),
        json!({
            "provider":{"local":{"type":"openai_compatible","models":{"zai-glm-5-3":{}}}},
            "model":"local/zai-glm-5-3", "permission":"allow",
            "agent":{"default":{"tools":["write","eval"]}},
            "instructions":"Keep the project contract."
        })
        .to_string(),
    )?;
    let updated =
        "${% extends \"system.md\" %}${% block model_guidance %}PROJECT_GLM${% endblock %}";
    let provider = Arc::new(MockProvider::script([
        call(
            "write",
            json!({"path":".agent-harness/prompts/models/glm-5.3.md", "content":updated}),
        ),
        done("Reloaded the prompt."),
        done("Used the ancestor prompt."),
        done("Used the user prompt."),
    ]));
    let deps = CliDeps::real()
        .with_current_dir(workspace.clone())
        .without_env("HOME")
        .with_env(
            "XDG_CONFIG_HOME",
            root.path().join("user").to_string_lossy(),
        )
        .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>);
    let invoke = |deps: CliDeps| {
        let config_path = workspace.join("fixture.json");
        let config_path = config_path.to_string_lossy();
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                config_path.as_ref(),
                "prompt",
                "--text",
                "Continue.",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            deps,
        );
        (result.code, String::from_utf8_lossy(&stderr).into_owned())
    };
    let (code, error) = invoke(deps.clone());
    assert_eq!(code, 0, "{error}");
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 2);
    for (request, marker) in requests.iter().zip(["USER_GLM", "PROJECT_GLM"]) {
        let system = &request.messages[0].content;
        assert!(
            system.contains(&format!("PROJECT_BASE {marker}")),
            "{system}"
        );
        assert!(!system.contains("USER_BASE"));
        assert!(system.contains("Keep the project contract."));
        assert_eq!(system.matches("CUSTOM_EVAL_RULE").count(), 1);
        let description = request
            .tools
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|tool| tool.tool_id == "eval")
            .and_then(|tool| tool.description.as_deref())
            .ok_or("eval description missing")?;
        assert!(!description.contains("CUSTOM_EVAL_RULE"));
    }
    for (content, expected) in [
        ("${% invalid %}".to_owned(), "syntax error"),
        (" ".to_owned(), "prompt must be nonempty"),
        ("x".repeat(256 * 1024 + 1), "at most 256 KiB"),
        (
            "${% include \"../outside.md\" %}".to_owned(),
            "without traversal",
        ),
    ] {
        fs::write(project.join("models/glm-5.3.md"), content)?;
        let (code, error) = invoke(deps.clone());
        assert_ne!(code, 0);
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
    assert_eq!(
        provider.captured_requests().await.len(),
        2,
        "invalid prompts reached the provider"
    );
    #[cfg(unix)]
    {
        fs::remove_file(project.join("models/glm-5.3.md"))?;
        let outside = root.path().join("outside.md");
        fs::write(&outside, "OUTSIDE_PROMPT")?;
        std::os::unix::fs::symlink(outside, project.join("models/glm-5.3.md"))?;
        let (code, error) = invoke(deps.clone());
        assert_ne!(code, 0);
        assert!(error.contains("inside its prompt directory"), "{error}");
        assert_eq!(provider.captured_requests().await.len(), 2);
    }
    fs::remove_file(project.join("models/glm-5.3.md"))?;
    fs::create_dir(workspace.join(".git"))?;
    let nested = workspace.join("nested");
    fs::create_dir(&nested)?;
    let (code, error) = invoke(deps.clone().with_current_dir(nested));
    assert_eq!(code, 0, "{error}");
    assert!(provider.captured_requests().await[2].messages[0]
        .content
        .contains("PROJECT_BASE USER_GLM"));
    fs::remove_file(project.join("system.md"))?;
    let (code, error) = invoke(deps);
    assert_eq!(code, 0, "{error}");
    let requests = provider.captured_requests().await;
    let last = &requests[3];
    assert!(last.messages[0].content.starts_with("USER_BASE"));
    // A base that omits the eval guidance still delivers it through the tool description.
    assert!(!last.messages[0].content.contains("CUSTOM_EVAL_RULE"));
    let description = last
        .tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|tool| tool.tool_id == "eval")
        .and_then(|tool| tool.description.as_deref())
        .ok_or("eval description missing")?;
    assert_eq!(description.matches("CUSTOM_EVAL_RULE").count(), 1);
    Ok(())
}

fn call(name: &str, args: serde_json::Value) -> Vec<Stream> {
    vec![
        Stream::ToolCallComplete {
            tool_call_id: "fixture-call".into(),
            function_name: name.into(),
            arguments_json: args.to_string(),
        },
        Stream::Done { usage: None },
    ]
}
fn done(text: &str) -> Vec<Stream> {
    vec![Stream::TextDelta(text.into()), Stream::Done { usage: None }]
}
